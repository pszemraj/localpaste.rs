#[cfg(feature = "cli")]
use clap::{CommandFactory, Parser, Subcommand};
#[cfg(feature = "cli")]
use clap_complete::{generate, Shell};
#[cfg(feature = "cli")]
use reqwest;
#[cfg(feature = "cli")]
use serde_json::Value;
#[cfg(feature = "cli")]
use std::io::{self, Read};

#[cfg(feature = "cli")]
#[derive(Parser)]
#[command(name = "lpaste", about = "LocalPaste CLI", version)]
struct Cli {
    /// Server URL (can also be set via LP_SERVER env var)
    #[arg(short, long, env = "LP_SERVER", default_value = "http://localhost:3030")]
    server: String,

    /// Output in JSON format
    #[arg(short, long, global = true)]
    json: bool,

    /// Request timeout in seconds
    #[arg(short = 't', long, default_value = "30")]
    timeout: u64,

    #[command(subcommand)]
    command: Commands,
}

#[cfg(feature = "cli")]
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

#[cfg(feature = "cli")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(cli.timeout))
        .build()?;

    match cli.command {
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

            let res = client
                .post(format!("{}/api/paste", cli.server))
                .json(&body)
                .send()
                .await?;
            let paste: Value = res.json().await?;
            if cli.json {
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
            let res = client
                .get(format!("{}/api/paste/{}", cli.server, id))
                .send()
                .await?;
            let paste: Value = res.json().await?;
            println!("{}", paste["content"].as_str().unwrap());
        }
        Commands::List { limit } => {
            let res = client
                .get(format!("{}/api/pastes?limit={}", cli.server, limit))
                .send()
                .await?;
            let pastes: Vec<Value> = res.json().await?;
            for p in pastes {
                println!(
                    "{:<24} {:<30}",
                    p["id"].as_str().unwrap(),
                    p["name"].as_str().unwrap()
                );
            }
        }
        Commands::Search { query } => {
            let res = client
                .get(format!("{}/api/search?q={}", cli.server, query))
                .send()
                .await?;
            let pastes: Vec<Value> = res.json().await?;
            for p in pastes {
                println!(
                    "{:<24} {:<30}",
                    p["id"].as_str().unwrap(),
                    p["name"].as_str().unwrap()
                );
            }
        }
        Commands::Delete { id } => {
            client
                .delete(format!("{}/api/paste/{}", cli.server, id))
                .send()
                .await?;
            println!("Deleted paste: {}", id);
        }
    }

    Ok(())
}

#[cfg(not(feature = "cli"))]
fn main() {
    eprintln!("lpaste CLI requires building with --features cli");
    std::process::exit(1);
}
