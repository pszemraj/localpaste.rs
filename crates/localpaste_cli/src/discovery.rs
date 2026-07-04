//! CLI server URL construction and endpoint discovery helpers.

use localpaste_core::{DEFAULT_CLI_SERVER_URL, LOCALPASTE_SERVER_HEADER, LOCALPASTE_SERVER_VALUE};
use std::io::{Read, Write};
use std::net::ToSocketAddrs;
use std::time::Duration;

fn api_url(server: &str, segments: &[&str]) -> Result<reqwest::Url, String> {
    let mut url = reqwest::Url::parse(server)
        .map_err(|err| format!("Invalid server URL '{}': {}", server, err))?;
    let mut path = url
        .path_segments_mut()
        .map_err(|_| "Server URL cannot be used as an API base".to_string())?;
    path.pop_if_empty();
    for segment in segments {
        path.push(segment);
    }
    drop(path);
    Ok(url)
}

/// Builds an API URL or exits with the action-specific CLI error prefix.
///
/// # Arguments
/// - `server`: Server base URL.
/// - `action`: User-facing action label for the error prefix.
/// - `segments`: Path segments to append to the base URL.
///
/// # Returns
/// A valid API URL.
pub(super) fn api_url_or_exit(server: &str, action: &str, segments: &[&str]) -> reqwest::Url {
    match api_url(server, segments) {
        Ok(url) => url,
        Err(message) => {
            eprintln!("{} failed: {}", action, message);
            std::process::exit(1);
        }
    }
}

/// Normalizes equivalent localhost server strings to a stable no-trailing-slash form.
///
/// # Returns
/// A normalized server URL string when parsing succeeds, otherwise the original input.
pub(super) fn normalize_server(server: String) -> String {
    if let Ok(mut url) = reqwest::Url::parse(&server) {
        let should_normalize_localhost =
            url.scheme().eq_ignore_ascii_case("http") && url.host_str() == Some("localhost");
        if should_normalize_localhost && url.set_host(Some("127.0.0.1")).is_err() {
            return server;
        }
        let mut normalized = url.to_string();
        while normalized.ends_with('/') {
            normalized.pop();
        }
        return normalized;
    }
    server
}

const DISCOVERY_PROBE_MAX_HEADER_BYTES: usize = 16 * 1024;

fn discovery_probe_response_looks_like_localpaste(response: &[u8]) -> bool {
    let Some(headers_end) = response.windows(4).position(|window| window == b"\r\n\r\n") else {
        return false;
    };
    let headers = String::from_utf8_lossy(&response[..headers_end]);
    let mut lines = headers.split("\r\n");
    let status = lines.next().unwrap_or_default();
    if !(status.starts_with("HTTP/1.1 200") || status.starts_with("HTTP/1.0 200")) {
        return false;
    }

    let mut has_json_content_type = false;
    let mut has_nosniff = false;
    let mut has_frame_deny = false;
    let mut has_localpaste_server = false;
    for line in lines {
        if line.is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim().to_ascii_lowercase();
        match name.as_str() {
            "content-type" => {
                if value.contains("application/json") {
                    has_json_content_type = true;
                }
            }
            "x-content-type-options" => {
                if value == "nosniff" {
                    has_nosniff = true;
                }
            }
            "x-frame-options" => {
                if value == "deny" {
                    has_frame_deny = true;
                }
            }
            header if header == LOCALPASTE_SERVER_HEADER => {
                if value == LOCALPASTE_SERVER_VALUE {
                    has_localpaste_server = true;
                }
            }
            _ => {}
        }
    }

    has_json_content_type && has_nosniff && has_frame_deny && has_localpaste_server
}

fn discovery_probe_host_header(host: &str, port: u16, scheme: &str) -> String {
    let default_port = match scheme {
        "http" => 80,
        "https" => 443,
        _ => return host.to_string(),
    };
    let host = if host.contains(':') && !host.starts_with('[') && !host.ends_with(']') {
        format!("[{}]", host)
    } else {
        host.to_string()
    };
    if port == default_port {
        host
    } else {
        format!("{}:{}", host, port)
    }
}

fn discovery_server_is_localpaste(url: &reqwest::Url) -> bool {
    if !url.scheme().eq_ignore_ascii_case("http") {
        return false;
    }

    let Ok(mut probe_url) = api_url(url.as_str(), &["api", "pastes", "meta"]) else {
        return false;
    };
    probe_url.query_pairs_mut().append_pair("limit", "1");

    let Some(host) = url.host_str() else {
        return false;
    };
    if !localpaste_core::text::is_loopback_host(host) {
        return false;
    }
    let Some(port) = url.port_or_known_default() else {
        return false;
    };

    let mut request_target = probe_url.path().to_string();
    if request_target.is_empty() {
        request_target.push('/');
    }
    if let Some(query) = probe_url.query() {
        request_target.push('?');
        request_target.push_str(query);
    }
    let host_header = discovery_probe_host_header(host, port, url.scheme());
    let probe_request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nAccept: application/json\r\nConnection: close\r\n\r\n",
        request_target, host_header
    );

    let timeout = Duration::from_millis(250);
    let Ok(addrs) = (host, port).to_socket_addrs() else {
        return false;
    };
    for addr in addrs {
        let Ok(mut stream) = std::net::TcpStream::connect_timeout(&addr, timeout) else {
            continue;
        };
        let _ = stream.set_read_timeout(Some(timeout));
        let _ = stream.set_write_timeout(Some(timeout));
        if stream.write_all(probe_request.as_bytes()).is_err() {
            continue;
        }
        let mut response = Vec::with_capacity(1024);
        let mut chunk = [0_u8; 1024];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => {
                    response.extend_from_slice(&chunk[..read]);
                    if response.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                    if response.len() >= DISCOVERY_PROBE_MAX_HEADER_BYTES {
                        response.clear();
                        break;
                    }
                }
                Err(err)
                    if err.kind() == std::io::ErrorKind::WouldBlock
                        || err.kind() == std::io::ErrorKind::TimedOut =>
                {
                    response.clear();
                    break;
                }
                Err(_) => {
                    response.clear();
                    break;
                }
            }
        }
        if !response.is_empty() && discovery_probe_response_looks_like_localpaste(&response) {
            return true;
        }
    }
    false
}

fn discovered_server_from_file_with_reachability<F>(is_reachable: F) -> Option<String>
where
    F: Fn(&reqwest::Url) -> bool,
{
    let path = localpaste_core::config::api_addr_file_path_from_env_or_default();
    let raw = std::fs::read_to_string(path).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Treat stale or hijacked discovery entries as absent so the CLI can
    // fall back to the default endpoint unless the discovered service
    // positively identifies as a LocalPaste API.
    let url = reqwest::Url::parse(trimmed).ok()?;
    if !is_reachable(&url) {
        return None;
    }
    Some(trimmed.to_string())
}

fn discovered_server_from_file() -> Option<String> {
    discovered_server_from_file_with_reachability(discovery_server_is_localpaste)
}

/// Describes how the CLI selected its server endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ServerResolutionSource {
    /// The user supplied `--server` or `LP_SERVER`.
    Explicit,
    /// The endpoint came from a validated discovery file.
    Discovery,
    /// The built-in default endpoint was used.
    Default,
}

impl ServerResolutionSource {
    /// Returns the stable diagnostic label for this resolution source.
    ///
    /// # Returns
    /// A stable diagnostic label.
    pub(super) fn as_str(self) -> &'static str {
        match self {
            ServerResolutionSource::Explicit => "explicit-or-env",
            ServerResolutionSource::Discovery => "discovery-file",
            ServerResolutionSource::Default => "default",
        }
    }
}

fn default_resolution_connect_hint(source: ServerResolutionSource) -> Option<&'static str> {
    match source {
        ServerResolutionSource::Default => Some(
            "Hint: CLI/server default endpoint mismatch is possible across mixed versions. Set --server (or LP_SERVER) explicitly.",
        ),
        ServerResolutionSource::Explicit | ServerResolutionSource::Discovery => None,
    }
}

/// Sends a request or exits with connection diagnostics that include endpoint resolution source.
///
/// # Arguments
/// - `request`: Request builder ready to send.
/// - `action`: User-facing action label for diagnostics.
/// - `source`: Source used to resolve the server endpoint.
/// - `server`: Resolved server endpoint string.
///
/// # Returns
/// The HTTP response when the request sends successfully.
pub(super) async fn send_or_exit(
    request: reqwest::RequestBuilder,
    action: &str,
    source: ServerResolutionSource,
    server: &str,
) -> reqwest::Response {
    match request.send().await {
        Ok(response) => response,
        Err(err) => {
            eprintln!("{} failed: {}", action, err);
            if err.is_connect() {
                eprintln!(
                    "{} failed: could not connect to '{}' (resolved via {}).",
                    action,
                    server,
                    source.as_str()
                );
                if let Some(hint) = default_resolution_connect_hint(source) {
                    eprintln!("{}", hint);
                }
            }
            std::process::exit(1);
        }
    }
}

/// Resolves the server endpoint together with the source used to choose it.
///
/// # Arguments
/// - `server`: Explicit server value from CLI/env resolution, when present.
/// - `allow_discovery`: Whether the `.api-addr` discovery file may be probed.
///
/// # Returns
/// The resolved server URL and the source used to choose it.
pub(super) fn resolve_server_with_source(
    server: Option<String>,
    allow_discovery: bool,
) -> (String, ServerResolutionSource) {
    if let Some(explicit) = localpaste_core::text::normalize_optional_nonempty(server) {
        return (explicit, ServerResolutionSource::Explicit);
    }
    if allow_discovery {
        if let Some(discovered) = discovered_server_from_file() {
            return (discovered, ServerResolutionSource::Discovery);
        }
    }
    (
        DEFAULT_CLI_SERVER_URL.to_string(),
        ServerResolutionSource::Default,
    )
}

#[cfg(test)]
/// Resolves the server endpoint with discovery enabled for legacy unit tests.
///
/// # Returns
/// The resolved server URL.
pub(super) fn resolve_server(server: Option<String>) -> String {
    resolve_server_with_source(server, true).0
}

/// Validates that a resolved server can be used as a base URL, exiting on failure.
pub(super) fn validate_server_base_or_exit(server: &str) {
    if let Err(message) = api_url(server, &[]) {
        eprintln!("Server resolution failed: {}", message);
        std::process::exit(1);
    }
}

#[cfg(test)]
/// Test-only accessors for private discovery helpers.
pub(crate) mod test_exports {
    use super::ServerResolutionSource;

    /// Builds an API URL by appending path segments to a validated server base URL.
    ///
    /// # Arguments
    /// - `server`: Server base URL.
    /// - `segments`: Path segments to append to the base URL.
    ///
    /// # Returns
    /// A valid API URL.
    ///
    /// # Errors
    /// Returns an error when the server base URL is invalid or cannot accept path segments.
    pub(crate) fn api_url(server: &str, segments: &[&str]) -> Result<reqwest::Url, String> {
        super::api_url(server, segments)
    }

    /// Checks whether a raw HTTP probe response positively identifies a LocalPaste API.
    ///
    /// # Returns
    /// `true` when the response has the expected status and LocalPaste headers.
    pub(crate) fn discovery_probe_response_looks_like_localpaste(response: &[u8]) -> bool {
        super::discovery_probe_response_looks_like_localpaste(response)
    }

    /// Reads the discovery file and returns its URL only when the supplied reachability check accepts it.
    ///
    /// # Arguments
    /// - `is_reachable`: Reachability predicate used to validate the discovered URL.
    ///
    /// # Returns
    /// The discovered URL when the file exists, is non-empty, parses, and passes the predicate.
    pub(crate) fn discovered_server_from_file_with_reachability<F>(
        is_reachable: F,
    ) -> Option<String>
    where
        F: Fn(&reqwest::Url) -> bool,
    {
        super::discovered_server_from_file_with_reachability(is_reachable)
    }

    /// Returns the default-endpoint connection hint when a resolution source benefits from it.
    ///
    /// # Returns
    /// A CLI hint for default resolution, or `None` for explicit/discovered endpoints.
    pub(crate) fn default_resolution_connect_hint(
        source: ServerResolutionSource,
    ) -> Option<&'static str> {
        super::default_resolution_connect_hint(source)
    }
}
