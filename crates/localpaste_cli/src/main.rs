//! Command-line client for the LocalPaste API.

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use serde_json::Value;
use std::io::{self, Read};
use std::time::{Duration, Instant};

#[derive(Parser)]
#[command(name = "lpaste", about = "LocalPaste CLI", version)]
struct Cli {
    /// Server URL (can also be set via LP_SERVER env var)
    #[arg(
        short,
        long,
        env = "LP_SERVER",
        default_value = "http://localhost:38411"
    )]
    server: String,

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

    let body = res.text().await.unwrap_or_default();
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
        rows.push(format!("{:<24} {:<30}", id, name));
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

fn normalize_server(server: String) -> String {
    if let Ok(mut url) = reqwest::Url::parse(&server) {
        let should_normalize_localhost =
            url.scheme().eq_ignore_ascii_case("http") && url.host_str() == Some("localhost");
        if should_normalize_localhost {
            let _ = url.set_host(Some("127.0.0.1"));
        }
        let mut normalized = url.to_string();
        while normalized.ends_with('/') {
            normalized.pop();
        }
        return normalized;
    }
    server
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(cli.timeout))
        .build()?;
    let timing = cli.timing;
    let json = cli.json;
    let server = normalize_server(cli.server);
    let command = cli.command;

    match command {
        Commands::Completions { shell } => {
            let mut cmd = Cli::command();
            let name = cmd.get_name().to_string();
            generate(shell, &mut cmd, name, &mut io::stdout());
            return Ok(());
        }
        Commands::New { file, name } => {
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
            let res = client
                .post(format!("{}/api/paste", server))
                .json(&body)
                .send()
                .await?;
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
            let endpoint = match api_url(&server, &["api", "paste", id.as_str()]) {
                Ok(url) => url,
                Err(message) => {
                    eprintln!("Get failed: {}", message);
                    std::process::exit(1);
                }
            };
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
            let request_start = Instant::now();
            let res = client
                .get(format!("{}/api/pastes", server))
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
            let request_start = Instant::now();
            let res = client
                .get(format!("{}/api/search", server))
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
        Commands::Delete { id } => {
            let endpoint = match api_url(&server, &["api", "paste", id.as_str()]) {
                Ok(url) => url,
                Err(message) => {
                    eprintln!("Delete failed: {}", message);
                    std::process::exit(1);
                }
            };
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
        api_url, error_message_for_response, format_delete_output, format_get_output,
        format_summary_output, normalize_server, paste_id_and_name,
    };

    #[test]
    fn normalize_server_rewrites_http_localhost() {
        let normalized = normalize_server("http://localhost:38411".to_string());
        assert_eq!(normalized, "http://127.0.0.1:38411");
    }

    #[test]
    fn normalize_server_preserves_https_localhost() {
        let normalized = normalize_server("https://localhost:38411".to_string());
        assert_eq!(normalized, "https://localhost:38411");
    }

    #[test]
    fn normalize_server_trims_trailing_slash() {
        let normalized = normalize_server("http://127.0.0.1:38411/".to_string());
        assert_eq!(normalized, "http://127.0.0.1:38411");
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
    fn paste_id_and_name_extracts_expected_fields() {
        let paste = serde_json::json!({
            "id": "abc123",
            "name": "demo"
        });

        assert_eq!(paste_id_and_name(&paste), Some(("abc123", "demo")));
    }

    #[test]
    fn paste_id_and_name_rejects_missing_fields() {
        let paste = serde_json::json!({
            "id": "abc123"
        });

        assert_eq!(paste_id_and_name(&paste), None);
    }

    #[test]
    fn summary_output_honors_json_mode() {
        let pastes = vec![serde_json::json!({
            "id": "abc123",
            "name": "demo"
        })];
        let rendered = format_summary_output(&pastes, true).expect("json output should render");
        let parsed: serde_json::Value =
            serde_json::from_str(&rendered).expect("rendered output should be valid json");
        assert_eq!(parsed[0]["id"], "abc123");
        assert_eq!(parsed[0]["name"], "demo");
    }

    #[test]
    fn get_output_honors_json_mode() {
        let paste = serde_json::json!({
            "id": "abc123",
            "name": "demo",
            "content": "hello"
        });
        let rendered = format_get_output(&paste, true).expect("json output should render");
        let parsed: serde_json::Value =
            serde_json::from_str(&rendered).expect("rendered output should be valid json");
        assert_eq!(parsed["content"], "hello");
    }

    #[test]
    fn delete_output_honors_json_mode() {
        let response = serde_json::json!({ "success": true });
        let rendered =
            format_delete_output("abc123", &response, true).expect("json output should render");
        let parsed: serde_json::Value =
            serde_json::from_str(&rendered).expect("rendered output should be valid json");
        assert_eq!(parsed["success"], true);
    }

    #[test]
    fn api_url_encodes_path_segments() {
        let url = api_url(
            "http://127.0.0.1:38411",
            &["api", "paste", "id/with?reserved#chars"],
        )
        .expect("api_url should build");
        assert_eq!(
            url.as_str(),
            "http://127.0.0.1:38411/api/paste/id%2Fwith%3Freserved%23chars"
        );
    }

    #[test]
    fn api_url_appends_segments_to_existing_base_path() {
        let url = api_url("http://127.0.0.1:38411/base", &["api", "paste", "abc123"])
            .expect("api_url should build");
        assert_eq!(url.as_str(), "http://127.0.0.1:38411/base/api/paste/abc123");
    }
}
