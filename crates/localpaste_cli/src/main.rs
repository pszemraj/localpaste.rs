//! Command-line client for the LocalPaste API.

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use localpaste_core::diff::{DiffRef, DiffRequest, DiffResponse, EqualResponse};
use serde_json::Value;
use std::io::{self, Read};
use std::num::NonZeroU64;
use std::time::{Duration, Instant};

mod discovery;
mod output;

use discovery::{
    api_url_or_exit, normalize_server, resolve_server_with_source, send_or_exit,
    validate_server_base_or_exit,
};
#[cfg(test)]
use discovery::{resolve_server, ServerResolutionSource};
use output::{
    ensure_success_or_exit, format_delete_output, format_diff_output, format_equal_output,
    format_get_output, format_summary_output, format_versions_output, paste_id_and_name,
};

#[derive(Parser)]
#[command(name = "lpaste", about = "LocalPaste CLI", version)]
struct Cli {
    /// Server URL (can also be set via LP_SERVER env var).
    ///
    /// Resolution order when unset: discovered `.api-addr` endpoint (unless
    /// `--no-discovery`) then the default local endpoint.
    #[arg(short, long, global = true, env = "LP_SERVER")]
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

    /// Request timeout in seconds (must be greater than zero)
    #[arg(short = 't', long, global = true, default_value = "30")]
    timeout: NonZeroU64,

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
    /// Create a new paste from stdin or a file.
    New {
        /// Read paste content from a file instead of stdin.
        #[arg(short, long)]
        file: Option<String>,
        /// Optional paste name. When omitted, the server generates one.
        #[arg(short, long)]
        name: Option<String>,
    },
    /// Fetch a paste by id and print its content.
    Get {
        /// Paste id to read.
        id: String,
    },
    /// List recent paste metadata.
    List {
        /// Maximum number of rows to return.
        #[arg(short, long, default_value = "10")]
        limit: usize,
    },
    /// Search pastes by full content.
    Search {
        /// Preserve case while matching the search query.
        #[arg(long, conflicts_with = "case_insensitive")]
        case_sensitive: bool,
        /// Ignore case while matching the search query.
        #[arg(long, conflicts_with = "case_sensitive")]
        case_insensitive: bool,
        /// Search query text.
        query: String,
    },
    /// Search persisted metadata only (name, tags, language, derived terms).
    SearchMeta {
        /// Preserve case while matching the search query.
        #[arg(long, conflicts_with = "case_insensitive")]
        case_sensitive: bool,
        /// Ignore case while matching the search query.
        #[arg(long, conflicts_with = "case_sensitive")]
        case_insensitive: bool,
        /// Search query text.
        query: String,
    },
    /// Delete a paste by id.
    Delete {
        /// Paste id to delete.
        id: String,
    },
    /// List stored historical versions for a paste.
    Versions {
        /// Paste id whose version history should be listed.
        id: String,
        /// Maximum number of historical versions to return.
        #[arg(short, long, default_value = "50")]
        limit: usize,
    },
    /// Fetch one stored historical version by paste id and version id.
    GetVersion {
        /// Paste id whose version should be read.
        id: String,
        /// Historical version timestamp id in milliseconds.
        version_id_ms: u64,
    },
    /// Diff two paste refs, optionally pinning each side to a historical version.
    Diff {
        /// Left-hand paste id.
        left_id: String,
        /// Right-hand paste id.
        right_id: String,
        /// Optional historical version id for the left-hand paste.
        #[arg(long)]
        left_version: Option<u64>,
        /// Optional historical version id for the right-hand paste.
        #[arg(long)]
        right_version: Option<u64>,
    },
    /// Compare two paste refs and return a success exit code only when equal.
    Equal {
        /// Left-hand paste id.
        left_id: String,
        /// Right-hand paste id.
        right_id: String,
        /// Optional historical version id for the left-hand paste.
        #[arg(long)]
        left_version: Option<u64>,
        /// Optional historical version id for the right-hand paste.
        #[arg(long)]
        right_version: Option<u64>,
    },
    /// Reset a paste to a stored historical version and discard newer history.
    ResetHard {
        /// Paste id to reset.
        id: String,
        /// Historical version timestamp id in milliseconds.
        version_id_ms: u64,
        /// Required acknowledgement for the destructive reset.
        #[arg(long, action = clap::ArgAction::SetTrue, required = true)]
        yes: bool,
    },
    /// Create a new paste from a stored historical version.
    DuplicateVersion {
        /// Paste id whose historical version should be duplicated.
        id: String,
        /// Historical version timestamp id in milliseconds.
        version_id_ms: u64,
        /// Optional name for the duplicated paste.
        #[arg(short, long)]
        name: Option<String>,
    },
}

enum ApiCommand {
    New {
        file: Option<String>,
        name: Option<String>,
    },
    Get {
        id: String,
    },
    List {
        limit: usize,
    },
    Search {
        query: String,
        case_sensitive: Option<bool>,
    },
    SearchMeta {
        query: String,
        case_sensitive: Option<bool>,
    },
    Delete {
        id: String,
    },
    Versions {
        id: String,
        limit: usize,
    },
    GetVersion {
        id: String,
        version_id_ms: u64,
    },
    Diff {
        left_id: String,
        right_id: String,
        left_version: Option<u64>,
        right_version: Option<u64>,
    },
    Equal {
        left_id: String,
        right_id: String,
        left_version: Option<u64>,
        right_version: Option<u64>,
    },
    ResetHard {
        id: String,
        version_id_ms: u64,
    },
    DuplicateVersion {
        id: String,
        version_id_ms: u64,
        name: Option<String>,
    },
}

fn case_sensitive_override(case_sensitive: bool, case_insensitive: bool) -> Option<bool> {
    if case_sensitive {
        Some(true)
    } else if case_insensitive {
        Some(false)
    } else {
        None
    }
}

fn classify_command(command: Commands) -> Result<ApiCommand, Shell> {
    match command {
        Commands::Completions { shell } => Err(shell),
        Commands::New { file, name } => Ok(ApiCommand::New { file, name }),
        Commands::Get { id } => Ok(ApiCommand::Get { id }),
        Commands::List { limit } => Ok(ApiCommand::List { limit }),
        Commands::Search {
            query,
            case_sensitive,
            case_insensitive,
        } => Ok(ApiCommand::Search {
            query,
            case_sensitive: case_sensitive_override(case_sensitive, case_insensitive),
        }),
        Commands::SearchMeta {
            query,
            case_sensitive,
            case_insensitive,
        } => Ok(ApiCommand::SearchMeta {
            query,
            case_sensitive: case_sensitive_override(case_sensitive, case_insensitive),
        }),
        Commands::Delete { id } => Ok(ApiCommand::Delete { id }),
        Commands::Versions { id, limit } => Ok(ApiCommand::Versions { id, limit }),
        Commands::GetVersion { id, version_id_ms } => {
            Ok(ApiCommand::GetVersion { id, version_id_ms })
        }
        Commands::Diff {
            left_id,
            right_id,
            left_version,
            right_version,
        } => Ok(ApiCommand::Diff {
            left_id,
            right_id,
            left_version,
            right_version,
        }),
        Commands::Equal {
            left_id,
            right_id,
            left_version,
            right_version,
        } => Ok(ApiCommand::Equal {
            left_id,
            right_id,
            left_version,
            right_version,
        }),
        Commands::ResetHard {
            id, version_id_ms, ..
        } => Ok(ApiCommand::ResetHard { id, version_id_ms }),
        Commands::DuplicateVersion {
            id,
            version_id_ms,
            name,
        } => Ok(ApiCommand::DuplicateVersion {
            id,
            version_id_ms,
            name,
        }),
    }
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

    let command = match classify_command(command) {
        Err(shell) => {
            let mut cmd = Cli::command();
            let name = cmd.get_name().to_string();
            generate(shell, &mut cmd, name, &mut io::stdout());
            return Ok(());
        }
        Ok(command) => command,
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout.get()))
        .build()?;
    let (resolved_server, source) = resolve_server_with_source(server, !no_discovery);
    let server = normalize_server(resolved_server);
    validate_server_base_or_exit(server.as_str());
    if timing {
        eprintln!("[server] resolved via {}", source.as_str());
    }

    match command {
        ApiCommand::New { file, name } => {
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
        ApiCommand::Get { id } => {
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
        ApiCommand::List { limit } => {
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
        ApiCommand::Search {
            query,
            case_sensitive,
        } => {
            let endpoint = api_url_or_exit(&server, "Search", &["api", "search"]);
            let mut request = client.get(endpoint).query(&[("q", query.as_str())]);
            if let Some(case_sensitive) = case_sensitive {
                request = request.query(&[("case_sensitive", case_sensitive)]);
            }
            let request_start = Instant::now();
            let res = send_or_exit(request, "Search", source, server.as_str()).await;
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
        ApiCommand::SearchMeta {
            query,
            case_sensitive,
        } => {
            let endpoint = api_url_or_exit(&server, "Search metadata", &["api", "search", "meta"]);
            let mut request = client.get(endpoint).query(&[("q", query.as_str())]);
            if let Some(case_sensitive) = case_sensitive {
                request = request.query(&[("case_sensitive", case_sensitive)]);
            }
            let request_start = Instant::now();
            let res = send_or_exit(request, "Search metadata", source, server.as_str()).await;
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
        ApiCommand::Delete { id } => {
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
        ApiCommand::Versions { id, limit } => {
            let endpoint = api_url_or_exit(
                &server,
                "Versions",
                &["api", "paste", id.as_str(), "versions"],
            );
            let request_start = Instant::now();
            let res = send_or_exit(
                client.get(endpoint).query(&[("limit", limit)]),
                "Versions",
                source,
                server.as_str(),
            )
            .await;
            let request_elapsed = request_start.elapsed();
            let res = ensure_success_or_exit(res, "Versions").await;

            let parse_start = Instant::now();
            let versions: Vec<Value> = res.json().await?;
            let parse_elapsed = parse_start.elapsed();
            log_timing_parts(timing, "versions", request_elapsed, Some(parse_elapsed));

            let output = match format_versions_output(&versions, json) {
                Ok(output) => output,
                Err(message) => {
                    eprintln!("Versions failed: {}", message);
                    std::process::exit(1);
                }
            };
            if !output.is_empty() {
                println!("{}", output);
            }
        }
        ApiCommand::GetVersion { id, version_id_ms } => {
            let version_segment = version_id_ms.to_string();
            let endpoint = api_url_or_exit(
                &server,
                "Get version",
                &[
                    "api",
                    "paste",
                    id.as_str(),
                    "versions",
                    version_segment.as_str(),
                ],
            );
            let request_start = Instant::now();
            let res =
                send_or_exit(client.get(endpoint), "Get version", source, server.as_str()).await;
            let request_elapsed = request_start.elapsed();
            let res = ensure_success_or_exit(res, "Get version").await;

            let parse_start = Instant::now();
            let snapshot: Value = res.json().await?;
            let parse_elapsed = parse_start.elapsed();
            log_timing_parts(timing, "get-version", request_elapsed, Some(parse_elapsed));

            let output = match format_get_output(&snapshot, json) {
                Ok(output) => output,
                Err(message) => {
                    eprintln!("Get version failed: {}", message);
                    std::process::exit(1);
                }
            };
            println!("{}", output);
        }
        ApiCommand::Diff {
            left_id,
            right_id,
            left_version,
            right_version,
        } => {
            let endpoint = api_url_or_exit(&server, "Diff", &["api", "diff"]);
            let body = DiffRequest {
                left: DiffRef {
                    paste_id: left_id,
                    version_id_ms: left_version,
                },
                right: DiffRef {
                    paste_id: right_id,
                    version_id_ms: right_version,
                },
            };
            let request_start = Instant::now();
            let res = send_or_exit(
                client.post(endpoint).json(&body),
                "Diff",
                source,
                server.as_str(),
            )
            .await;
            let request_elapsed = request_start.elapsed();
            let res = ensure_success_or_exit(res, "Diff").await;

            let parse_start = Instant::now();
            let diff: DiffResponse = res.json().await?;
            let parse_elapsed = parse_start.elapsed();
            log_timing_parts(timing, "diff", request_elapsed, Some(parse_elapsed));

            let output = match format_diff_output(&diff, json) {
                Ok(output) => output,
                Err(message) => {
                    eprintln!("Diff failed: {}", message);
                    std::process::exit(1);
                }
            };
            if !output.is_empty() {
                println!("{}", output);
            }
        }
        ApiCommand::Equal {
            left_id,
            right_id,
            left_version,
            right_version,
        } => {
            let endpoint = api_url_or_exit(&server, "Equal", &["api", "equal"]);
            let body = DiffRequest {
                left: DiffRef {
                    paste_id: left_id,
                    version_id_ms: left_version,
                },
                right: DiffRef {
                    paste_id: right_id,
                    version_id_ms: right_version,
                },
            };
            let request_start = Instant::now();
            let res = send_or_exit(
                client.post(endpoint).json(&body),
                "Equal",
                source,
                server.as_str(),
            )
            .await;
            let request_elapsed = request_start.elapsed();
            let res = ensure_success_or_exit(res, "Equal").await;

            let parse_start = Instant::now();
            let equal: EqualResponse = res.json().await?;
            let parse_elapsed = parse_start.elapsed();
            log_timing_parts(timing, "equal", request_elapsed, Some(parse_elapsed));
            let output = match format_equal_output(&equal, json) {
                Ok(output) => output,
                Err(message) => {
                    eprintln!("Equal failed: {}", message);
                    std::process::exit(1);
                }
            };
            println!("{}", output);
            std::process::exit(if equal.equal { 0 } else { 1 });
        }
        ApiCommand::ResetHard { id, version_id_ms } => {
            let version_segment = version_id_ms.to_string();
            let endpoint = api_url_or_exit(
                &server,
                "Reset hard",
                &[
                    "api",
                    "paste",
                    id.as_str(),
                    "versions",
                    version_segment.as_str(),
                    "reset-hard",
                ],
            );
            let request_start = Instant::now();
            let res =
                send_or_exit(client.post(endpoint), "Reset hard", source, server.as_str()).await;
            let request_elapsed = request_start.elapsed();
            let res = ensure_success_or_exit(res, "Reset hard").await;

            let parse_start = Instant::now();
            let paste: Value = res.json().await?;
            let parse_elapsed = parse_start.elapsed();
            log_timing_parts(timing, "reset-hard", request_elapsed, Some(parse_elapsed));

            if json {
                println!("{}", serde_json::to_string_pretty(&paste)?);
            } else {
                println!("Reset paste {} to version {}.", id, version_id_ms);
            }
        }
        ApiCommand::DuplicateVersion {
            id,
            version_id_ms,
            name,
        } => {
            let version_segment = version_id_ms.to_string();
            let endpoint = api_url_or_exit(
                &server,
                "Duplicate version",
                &[
                    "api",
                    "paste",
                    id.as_str(),
                    "versions",
                    version_segment.as_str(),
                    "duplicate",
                ],
            );
            let body = serde_json::json!({ "name": name });
            let request_start = Instant::now();
            let res = send_or_exit(
                client.post(endpoint).json(&body),
                "Duplicate version",
                source,
                server.as_str(),
            )
            .await;
            let request_elapsed = request_start.elapsed();
            let res = ensure_success_or_exit(res, "Duplicate version").await;

            let parse_start = Instant::now();
            let paste: Value = res.json().await?;
            let parse_elapsed = parse_start.elapsed();
            log_timing_parts(
                timing,
                "duplicate-version",
                request_elapsed,
                Some(parse_elapsed),
            );
            if json {
                println!("{}", serde_json::to_string_pretty(&paste)?);
            } else {
                let Some((new_id, new_name)) = paste_id_and_name(&paste) else {
                    eprintln!("Duplicate version failed: response missing 'id' or 'name' field");
                    std::process::exit(1);
                };
                println!("Created: {} ({})", new_name, new_id);
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests;
