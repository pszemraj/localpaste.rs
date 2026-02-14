//! Command-line client for the LocalPaste API.

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use localpaste_core::DEFAULT_CLI_SERVER_URL;
use serde_json::Value;
use std::io::{self, Read, Write};
use std::net::ToSocketAddrs;
use std::time::{Duration, Instant};

#[derive(Parser)]
#[command(name = "lpaste", about = "LocalPaste CLI", version)]
struct Cli {
    /// Server URL (can also be set via LP_SERVER env var).
    ///
    /// Resolution order when unset: discovered `.api-addr` endpoint (unless
    /// `--no-discovery`) then the default local endpoint.
    #[arg(short, long, env = "LP_SERVER")]
    server: Option<String>,

    /// Disable `.api-addr` discovery/probing fallback.
    ///
    /// When set, `lpaste` uses only `--server`/`LP_SERVER` or the default
    /// endpoint and performs no discovery network probe.
    #[arg(long, global = true, default_value_t = false)]
    no_discovery: bool,

    /// Output in JSON format
    #[arg(short, long, global = true)]
    json: bool,

    /// Print timing for API requests
    #[arg(long, global = true)]
    timing: bool,

    /// Request timeout in seconds
    #[arg(short = 't', long, default_value = "30")]
    timeout: u64,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },
    New {
        #[arg(short, long)]
        file: Option<String>,
        #[arg(short, long)]
        name: Option<String>,
    },
    Get {
        id: String,
    },
    List {
        #[arg(short, long, default_value = "10")]
        limit: usize,
    },
    Search {
        query: String,
    },
    SearchMeta {
        query: String,
    },
    Delete {
        id: String,
    },
}

fn log_timing(timing: bool, label: &str, duration: Duration) {
    if timing {
        eprintln!(
            "[timing] {}: {:.1} ms",
            label,
            duration.as_secs_f64() * 1000.0
        );
    }
}

fn log_timing_parts(timing: bool, label: &str, request: Duration, parse: Option<Duration>) {
    if !timing {
        return;
    }
    if let Some(parse) = parse {
        let total = request + parse;
        eprintln!(
            "[timing] {}: request {:.1} ms, parse {:.1} ms, total {:.1} ms",
            label,
            request.as_secs_f64() * 1000.0,
            parse.as_secs_f64() * 1000.0,
            total.as_secs_f64() * 1000.0
        );
    } else {
        log_timing(timing, label, request);
    }
}

fn error_message_for_response(status: reqwest::StatusCode, body: &str) -> String {
    if body.trim().is_empty() {
        return status
            .canonical_reason()
            .unwrap_or("Request failed")
            .to_string();
    }

    if let Ok(value) = serde_json::from_str::<Value>(body) {
        return value
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or(body)
            .to_string();
    }

    body.to_string()
}

async fn ensure_success_or_exit(res: reqwest::Response, action: &str) -> reqwest::Response {
    let status = res.status();
    if status.is_success() {
        return res;
    }

    let body = match res.text().await {
        Ok(body) => body,
        Err(err) => format!("failed to read error response body: {}", err),
    };
    let message = error_message_for_response(status, &body);
    eprintln!("{} failed ({}): {}", action, status, message);
    std::process::exit(1);
}

fn paste_id_and_name(paste: &Value) -> Option<(&str, &str)> {
    let id = paste.get("id").and_then(Value::as_str)?;
    let name = paste.get("name").and_then(Value::as_str)?;
    Some((id, name))
}

fn format_summary_output(pastes: &[Value], json: bool) -> Result<String, String> {
    if json {
        return serde_json::to_string_pretty(pastes)
            .map_err(|err| format!("response encoding error: {}", err));
    }

    let mut rows = Vec::with_capacity(pastes.len());
    for (index, p) in pastes.iter().enumerate() {
        let Some((id, name)) = paste_id_and_name(p) else {
            return Err(format!(
                "response item {} missing 'id' or 'name' field",
                index
            ));
        };
        rows.push(format!("{:<36} {:<30}", id, name));
    }

    Ok(rows.join("\n"))
}

fn format_get_output(paste: &Value, json: bool) -> Result<String, String> {
    if json {
        return serde_json::to_string_pretty(paste)
            .map_err(|err| format!("response encoding error: {}", err));
    }

    paste
        .get("content")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "response missing 'content' field".to_string())
}

fn format_delete_output(id: &str, response: &Value, json: bool) -> Result<String, String> {
    if json {
        return serde_json::to_string_pretty(response)
            .map_err(|err| format!("response encoding error: {}", err));
    }

    Ok(format!("Deleted paste: {}", id))
}

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

fn api_url_or_exit(server: &str, action: &str, segments: &[&str]) -> reqwest::Url {
    match api_url(server, segments) {
        Ok(url) => url,
        Err(message) => {
            eprintln!("{} failed: {}", action, message);
            std::process::exit(1);
        }
    }
}

fn normalize_server(server: String) -> String {
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
            "x-localpaste-server" => {
                if value == "1" {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServerResolutionSource {
    Explicit,
    Discovery,
    Default,
}

impl ServerResolutionSource {
    fn as_str(self) -> &'static str {
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

async fn send_or_exit(
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

fn resolve_server_with_source(
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
fn resolve_server(server: Option<String>) -> String {
    resolve_server_with_source(server, true).0
}

fn validate_server_base_or_exit(server: &str) {
    if let Err(message) = api_url(server, &[]) {
        eprintln!("Server resolution failed: {}", message);
        std::process::exit(1);
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Cli {
        server,
        no_discovery,
        json,
        timing,
        timeout,
        command,
    } = Cli::parse();

    if let Commands::Completions { shell } = &command {
        let mut cmd = Cli::command();
        let name = cmd.get_name().to_string();
        generate(*shell, &mut cmd, name, &mut io::stdout());
        return Ok(());
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout))
        .build()?;
    let (resolved_server, source) = resolve_server_with_source(server, !no_discovery);
    let server = normalize_server(resolved_server);
    validate_server_base_or_exit(server.as_str());
    if timing {
        eprintln!("[server] resolved via {}", source.as_str());
    }

    match command {
        Commands::Completions { .. } => unreachable!("completions handled before client setup"),
        Commands::New { file, name } => {
            let endpoint = api_url_or_exit(&server, "New", &["api", "paste"]);
            let content = if let Some(path) = file {
                std::fs::read_to_string(path)?
            } else {
                let mut buffer = String::new();
                io::stdin().read_to_string(&mut buffer)?;
                buffer
            };

            let mut body = serde_json::json!({ "content": content });
            if let Some(n) = name {
                body["name"] = n.into();
            }

            let request_start = Instant::now();
            let res = send_or_exit(
                client.post(endpoint).json(&body),
                "New",
                source,
                server.as_str(),
            )
            .await;
            let request_elapsed = request_start.elapsed();
            let res = ensure_success_or_exit(res, "New").await;

            let parse_start = Instant::now();
            let paste: Value = res.json().await?;
            let parse_elapsed = parse_start.elapsed();

            log_timing_parts(timing, "new", request_elapsed, Some(parse_elapsed));
            if json {
                println!("{}", serde_json::to_string_pretty(&paste)?);
            } else {
                let Some((id, name)) = paste_id_and_name(&paste) else {
                    eprintln!("New failed: response missing 'id' or 'name' field");
                    std::process::exit(1);
                };
                println!("Created: {} ({})", name, id);
            }
        }
        Commands::Get { id } => {
            let endpoint = api_url_or_exit(&server, "Get", &["api", "paste", id.as_str()]);
            let request_start = Instant::now();
            let res = send_or_exit(client.get(endpoint), "Get", source, server.as_str()).await;
            let request_elapsed = request_start.elapsed();
            let res = ensure_success_or_exit(res, "Get").await;

            let parse_start = Instant::now();
            let paste: Value = res.json().await?;
            let parse_elapsed = parse_start.elapsed();

            log_timing_parts(timing, "get", request_elapsed, Some(parse_elapsed));
            let output = match format_get_output(&paste, json) {
                Ok(output) => output,
                Err(message) => {
                    eprintln!("Get failed: {}", message);
                    std::process::exit(1);
                }
            };
            println!("{}", output);
        }
        Commands::List { limit } => {
            let endpoint = api_url_or_exit(&server, "List", &["api", "pastes", "meta"]);
            let request_start = Instant::now();
            let res = send_or_exit(
                client.get(endpoint).query(&[("limit", limit)]),
                "List",
                source,
                server.as_str(),
            )
            .await;
            let request_elapsed = request_start.elapsed();
            let res = ensure_success_or_exit(res, "List").await;

            let parse_start = Instant::now();
            let pastes: Vec<Value> = res.json().await?;
            let parse_elapsed = parse_start.elapsed();

            log_timing_parts(timing, "list", request_elapsed, Some(parse_elapsed));
            let output = match format_summary_output(&pastes, json) {
                Ok(output) => output,
                Err(message) => {
                    eprintln!("List failed: {}", message);
                    std::process::exit(1);
                }
            };
            if !output.is_empty() {
                println!("{}", output);
            }
        }
        Commands::Search { query } => {
            let endpoint = api_url_or_exit(&server, "Search", &["api", "search"]);
            let request_start = Instant::now();
            let res = send_or_exit(
                client.get(endpoint).query(&[("q", query.as_str())]),
                "Search",
                source,
                server.as_str(),
            )
            .await;
            let request_elapsed = request_start.elapsed();
            let res = ensure_success_or_exit(res, "Search").await;

            let parse_start = Instant::now();
            let pastes: Vec<Value> = res.json().await?;
            let parse_elapsed = parse_start.elapsed();

            log_timing_parts(timing, "search", request_elapsed, Some(parse_elapsed));
            let output = match format_summary_output(&pastes, json) {
                Ok(output) => output,
                Err(message) => {
                    eprintln!("Search failed: {}", message);
                    std::process::exit(1);
                }
            };
            if !output.is_empty() {
                println!("{}", output);
            }
        }
        Commands::SearchMeta { query } => {
            let endpoint = api_url_or_exit(&server, "Search metadata", &["api", "search", "meta"]);
            let request_start = Instant::now();
            let res = send_or_exit(
                client.get(endpoint).query(&[("q", query.as_str())]),
                "Search metadata",
                source,
                server.as_str(),
            )
            .await;
            let request_elapsed = request_start.elapsed();
            let res = ensure_success_or_exit(res, "Search metadata").await;

            let parse_start = Instant::now();
            let pastes: Vec<Value> = res.json().await?;
            let parse_elapsed = parse_start.elapsed();

            log_timing_parts(timing, "search-meta", request_elapsed, Some(parse_elapsed));
            let output = match format_summary_output(&pastes, json) {
                Ok(output) => output,
                Err(message) => {
                    eprintln!("Search metadata failed: {}", message);
                    std::process::exit(1);
                }
            };
            if !output.is_empty() {
                println!("{}", output);
            }
        }
        Commands::Delete { id } => {
            let endpoint = api_url_or_exit(&server, "Delete", &["api", "paste", id.as_str()]);
            let request_start = Instant::now();
            let res =
                send_or_exit(client.delete(endpoint), "Delete", source, server.as_str()).await;
            let request_elapsed = request_start.elapsed();
            let res = ensure_success_or_exit(res, "Delete").await;
            let parse_start = Instant::now();
            let response: Value = res.json().await?;
            let parse_elapsed = parse_start.elapsed();
            log_timing_parts(timing, "delete", request_elapsed, Some(parse_elapsed));

            let output = match format_delete_output(&id, &response, json) {
                Ok(output) => output,
                Err(message) => {
                    eprintln!("Delete failed: {}", message);
                    std::process::exit(1);
                }
            };
            println!("{}", output);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests;
