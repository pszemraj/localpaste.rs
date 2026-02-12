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

            let parse_start = Instant::now();
            let paste: Value = res.json().await?;
            let parse_elapsed = parse_start.elapsed();

            log_timing_parts(timing, "new", request_elapsed, Some(parse_elapsed));
            if json {
                println!("{}", serde_json::to_string_pretty(&paste)?);
            } else {
                println!(
                    "Created: {} ({})",
                    paste["name"].as_str().unwrap(),
                    paste["id"].as_str().unwrap()
                );
            }
        }
        Commands::Get { id } => {
            let request_start = Instant::now();
            let res = client
                .get(format!("{}/api/paste/{}", server, id))
                .send()
                .await?;
            let request_elapsed = request_start.elapsed();

            let parse_start = Instant::now();
            let paste: Value = res.json().await?;
            let parse_elapsed = parse_start.elapsed();

            log_timing_parts(timing, "get", request_elapsed, Some(parse_elapsed));
            println!("{}", paste["content"].as_str().unwrap());
        }
        Commands::List { limit } => {
            let request_start = Instant::now();
            let res = client
                .get(format!("{}/api/pastes?limit={}", server, limit))
                .send()
                .await?;
            let request_elapsed = request_start.elapsed();

            let parse_start = Instant::now();
            let pastes: Vec<Value> = res.json().await?;
            let parse_elapsed = parse_start.elapsed();

            log_timing_parts(timing, "list", request_elapsed, Some(parse_elapsed));
            for p in pastes {
                println!(
                    "{:<24} {:<30}",
                    p["id"].as_str().unwrap(),
                    p["name"].as_str().unwrap()
                );
            }
        }
        Commands::Search { query } => {
            let request_start = Instant::now();
            let res = client
                .get(format!("{}/api/search?q={}", server, query))
                .send()
                .await?;
            let request_elapsed = request_start.elapsed();

            let parse_start = Instant::now();
            let pastes: Vec<Value> = res.json().await?;
            let parse_elapsed = parse_start.elapsed();

            log_timing_parts(timing, "search", request_elapsed, Some(parse_elapsed));
            for p in pastes {
                println!(
                    "{:<24} {:<30}",
                    p["id"].as_str().unwrap(),
                    p["name"].as_str().unwrap()
                );
            }
        }
        Commands::Delete { id } => {
            let request_start = Instant::now();
            let res = client
                .delete(format!("{}/api/paste/{}", server, id))
                .send()
                .await?;
            let request_elapsed = request_start.elapsed();
            log_timing(timing, "delete", request_elapsed);
            if res.status().is_success() {
                println!("Deleted paste: {}", id);
            } else {
                let status = res.status();
                let body = res.text().await.unwrap_or_default();
                let message = if body.trim().is_empty() {
                    status
                        .canonical_reason()
                        .unwrap_or("Request failed")
                        .to_string()
                } else if let Ok(value) = serde_json::from_str::<Value>(&body) {
                    value
                        .get("error")
                        .and_then(|v| v.as_str())
                        .unwrap_or(body.as_str())
                        .to_string()
                } else {
                    body
                };

                eprintln!("Delete failed ({}): {}", status, message);
                std::process::exit(1);
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::normalize_server;

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
}
