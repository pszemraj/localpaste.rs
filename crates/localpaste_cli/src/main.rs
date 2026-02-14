//! Command-line client for the LocalPaste API.

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use localpaste_core::DEFAULT_CLI_SERVER_URL;
use serde_json::Value;
use std::io::{self, Read};
use std::net::ToSocketAddrs;
use std::time::{Duration, Instant};

#[derive(Parser)]
#[command(name = "lpaste", about = "LocalPaste CLI", version)]
struct Cli {
    /// Server URL (can also be set via LP_SERVER env var)
    #[arg(short, long, env = "LP_SERVER")]
    server: Option<String>,

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

fn discovery_server_is_reachable(url: &reqwest::Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    let Some(port) = url.port_or_known_default() else {
        return false;
    };

    let timeout = Duration::from_millis(250);
    let Ok(addrs) = (host, port).to_socket_addrs() else {
        return false;
    };
    for addr in addrs {
        if std::net::TcpStream::connect_timeout(&addr, timeout).is_ok() {
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
    // Treat stale discovery entries as absent so the CLI can fall back
    // to the default endpoint when the discovered server is no longer up.
    let url = reqwest::Url::parse(trimmed).ok()?;
    if !is_reachable(&url) {
        return None;
    }
    Some(trimmed.to_string())
}

fn discovered_server_from_file() -> Option<String> {
    discovered_server_from_file_with_reachability(discovery_server_is_reachable)
}

fn explicit_server_override(server: Option<String>) -> Option<String> {
    server.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn resolve_server(server: Option<String>) -> String {
    explicit_server_override(server)
        .or_else(discovered_server_from_file)
        .unwrap_or_else(|| DEFAULT_CLI_SERVER_URL.to_string())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Cli {
        server,
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
    let server = normalize_server(resolve_server(server));

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
            let res = client.post(endpoint).json(&body).send().await?;
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
            let res = client.get(endpoint).send().await?;
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
            let res = client
                .get(endpoint)
                .query(&[("limit", limit)])
                .send()
                .await?;
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
            let res = client
                .get(endpoint)
                .query(&[("q", query.as_str())])
                .send()
                .await?;
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
            let res = client
                .get(endpoint)
                .query(&[("q", query.as_str())])
                .send()
                .await?;
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
            let res = client.delete(endpoint).send().await?;
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
mod tests {
    use super::{
        api_url, discovered_server_from_file_with_reachability, error_message_for_response,
        format_delete_output, format_get_output, format_summary_output, normalize_server,
        paste_id_and_name, resolve_server,
    };
    use super::{Cli, Commands};
    use clap::Parser;
    use localpaste_core::config::api_addr_file_path_from_env_or_default;
    use localpaste_core::env::{env_lock, EnvGuard};
    use localpaste_core::{DEFAULT_CLI_SERVER_URL, DEFAULT_PORT};
    use std::net::TcpListener;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct DiscoveryTestEnv {
        _db_path_guard: EnvGuard,
        _lp_server_guard: EnvGuard,
        discovery_path: std::path::PathBuf,
    }

    impl DiscoveryTestEnv {
        fn new(label: &str, lp_server: Option<&str>) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos();
            let db_path = std::env::temp_dir().join(format!("lpaste-cli-{}-{}", label, nonce));
            let db_path = db_path.join("db");
            let db_path_string = db_path.to_string_lossy().to_string();
            let db_path_guard = EnvGuard::set("DB_PATH", db_path_string.as_str());
            let lp_server_guard = match lp_server {
                Some(value) => EnvGuard::set("LP_SERVER", value),
                None => EnvGuard::remove("LP_SERVER"),
            };

            let discovery_path = api_addr_file_path_from_env_or_default();
            if let Some(parent) = discovery_path.parent() {
                std::fs::create_dir_all(parent).expect("create discovery dir");
            }

            Self {
                _db_path_guard: db_path_guard,
                _lp_server_guard: lp_server_guard,
                discovery_path,
            }
        }

        fn write_discovery(&self, value: &str) {
            std::fs::write(&self.discovery_path, value).expect("write discovery");
        }
    }

    impl Drop for DiscoveryTestEnv {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.discovery_path);
        }
    }

    fn with_discovery_env<T>(
        label: &str,
        lp_server: Option<&str>,
        test: impl FnOnce(&DiscoveryTestEnv) -> T,
    ) -> T {
        let _lock = env_lock().lock().expect("env lock");
        let env = DiscoveryTestEnv::new(label, lp_server);
        test(&env)
    }

    fn bind_reachable_discovery(env: &DiscoveryTestEnv) -> (String, TcpListener) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let discovered = format!("http://{}", listener.local_addr().expect("listener addr"));
        env.write_discovery(discovered.as_str());
        (discovered, listener)
    }

    fn assert_resolve_server_falls_back_to_default(label: &str, discovery: &str) {
        with_discovery_env(label, None, |env| {
            env.write_discovery(discovery);
            assert_eq!(resolve_server(None), DEFAULT_CLI_SERVER_URL);
        });
    }

    #[test]
    fn normalize_server_matrix() {
        let cases = [
            (
                DEFAULT_CLI_SERVER_URL.to_string(),
                format!("http://127.0.0.1:{}", DEFAULT_PORT),
            ),
            (
                format!("https://localhost:{}", DEFAULT_PORT),
                format!("https://localhost:{}", DEFAULT_PORT),
            ),
            (
                format!("http://127.0.0.1:{}/", DEFAULT_PORT),
                format!("http://127.0.0.1:{}", DEFAULT_PORT),
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(normalize_server(input), expected);
        }
    }

    #[test]
    fn default_cli_server_url_uses_default_port_constant() {
        assert_eq!(
            DEFAULT_CLI_SERVER_URL,
            format!("http://localhost:{}", DEFAULT_PORT)
        );
    }

    #[test]
    fn error_message_for_response_prefers_json_error_field() {
        let status = reqwest::StatusCode::NOT_FOUND;
        let message = error_message_for_response(status, r#"{"error":"Not found"}"#);
        assert_eq!(message, "Not found");
    }

    #[test]
    fn error_message_for_response_uses_reason_for_empty_body() {
        let status = reqwest::StatusCode::BAD_REQUEST;
        let message = error_message_for_response(status, "   ");
        assert_eq!(message, "Bad Request");
    }

    #[test]
    fn paste_id_and_name_requires_both_fields() {
        let cases = [
            (
                serde_json::json!({
                    "id": "abc123",
                    "name": "demo"
                }),
                Some(("abc123", "demo")),
            ),
            (
                serde_json::json!({
                    "id": "abc123"
                }),
                None,
            ),
        ];
        for (paste, expected) in cases {
            assert_eq!(paste_id_and_name(&paste), expected);
        }
    }

    #[test]
    fn json_output_helpers_preserve_payload_shape() {
        let pastes = vec![serde_json::json!({
            "id": "abc123",
            "name": "demo"
        })];
        let paste = serde_json::json!({
            "id": "abc123",
            "name": "demo",
            "content": "hello"
        });
        let response = serde_json::json!({ "success": true });

        let summary_rendered =
            format_summary_output(&pastes, true).expect("summary json output should render");
        let summary_parsed: serde_json::Value =
            serde_json::from_str(&summary_rendered).expect("rendered summary should be valid json");
        assert_eq!(summary_parsed[0]["id"], "abc123");
        assert_eq!(summary_parsed[0]["name"], "demo");

        let get_rendered = format_get_output(&paste, true).expect("get json output should render");
        let get_parsed: serde_json::Value =
            serde_json::from_str(&get_rendered).expect("rendered get should be valid json");
        assert_eq!(get_parsed["content"], "hello");

        let delete_rendered = format_delete_output("abc123", &response, true)
            .expect("delete json output should render");
        let delete_parsed: serde_json::Value =
            serde_json::from_str(&delete_rendered).expect("rendered delete should be valid json");
        assert_eq!(delete_parsed["success"], true);
    }

    #[test]
    fn api_url_encodes_path_segments() {
        let url = api_url(
            &format!("http://127.0.0.1:{}", DEFAULT_PORT),
            &["api", "paste", "id/with?reserved#chars"],
        )
        .expect("api_url should build");
        assert_eq!(
            url.as_str(),
            &format!(
                "http://127.0.0.1:{}/api/paste/id%2Fwith%3Freserved%23chars",
                DEFAULT_PORT
            )
        );
    }

    #[test]
    fn api_url_appends_segments_to_existing_base_path() {
        let url = api_url(
            &format!("http://127.0.0.1:{}/base", DEFAULT_PORT),
            &["api", "paste", "abc123"],
        )
        .expect("api_url should build");
        assert_eq!(
            url.as_str(),
            &format!("http://127.0.0.1:{}/base/api/paste/abc123", DEFAULT_PORT)
        );
    }

    #[test]
    fn cli_parses_search_meta_subcommand() {
        let cli = Cli::try_parse_from(["lpaste", "search-meta", "needle"])
            .expect("cli should parse search-meta");
        match cli.command {
            Commands::SearchMeta { query } => assert_eq!(query, "needle"),
            _ => panic!("expected search-meta command"),
        }
    }

    #[test]
    fn resolve_server_prefers_explicit_over_discovery() {
        with_discovery_env("explicit", None, |env| {
            env.write_discovery("http://127.0.0.1:45555");
            assert_eq!(
                resolve_server(Some("http://127.0.0.1:45556".to_string())),
                "http://127.0.0.1:45556"
            );
        });
    }

    #[test]
    fn resolve_server_uses_discovery_when_explicit_missing() {
        with_discovery_env("discovery", None, |env| {
            let (discovered, _listener) = bind_reachable_discovery(env);
            assert_eq!(resolve_server(None), discovered);
        });
    }

    #[test]
    fn resolve_server_falls_back_to_default_when_discovery_invalid() {
        assert_resolve_server_falls_back_to_default("invalid", "not a url");
    }

    #[test]
    fn resolve_server_falls_back_to_default_when_discovery_unreachable() {
        // Unknown scheme is parseable but has no known default port, so reachability
        // check always treats it as unavailable.
        assert_resolve_server_falls_back_to_default("stale", "custom-scheme://discovery-host");
    }

    #[test]
    fn discovered_server_file_returns_none_when_reachability_check_fails() {
        with_discovery_env("stub-unreachable", None, |env| {
            env.write_discovery("http://127.0.0.1:45555");
            let discovered = discovered_server_from_file_with_reachability(|_| false);
            assert!(discovered.is_none());
        });
    }

    #[test]
    fn resolve_server_treats_blank_explicit_override_as_absent() {
        with_discovery_env("blank-explicit", None, |env| {
            let (discovered, _listener) = bind_reachable_discovery(env);
            assert_eq!(resolve_server(Some("   ".to_string())), discovered);
        });
    }

    #[test]
    fn lp_server_env_value_beats_discovery_file() {
        with_discovery_env("env", Some("http://127.0.0.1:47777"), |env| {
            env.write_discovery("http://127.0.0.1:48888");
            let cli = Cli::parse_from(["lpaste", "list"]);
            assert_eq!(resolve_server(cli.server), "http://127.0.0.1:47777");
        });
    }
}
