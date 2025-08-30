#[cfg(feature = "cli")]
use clap::{Parser, Subcommand};
#[cfg(feature = "cli")]
use reqwest;
use serde_json::Value;
use std::io::{self, Read};

#[cfg(feature = "cli")]
#[derive(Parser)]
#[command(name = "lpaste", about = "LocalPaste CLI", version)]
struct Cli {
    #[arg(short, long, default_value = "http://localhost:3030")]
    server: String,

    #[command(subcommand)]
    command: Commands,
}

#[cfg(feature = "cli")]
#[derive(Subcommand)]
enum Commands {
    New {
        #[arg(short, long)]
        file: Option<String>,
        #[arg(short, long)]
        name: Option<String>,
    },
    Get { id: String },
    List {
        #[arg(short, long, default_value = "10")]
        limit: usize,
    },
    Search { query: String },
    Delete { id: String },
}

#[cfg(feature = "cli")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let client = reqwest::Client::new();

    match cli.command {
        Commands::New { file, name } => {
            let content = if let Some(path) = file {
                std::fs::read_to_string(path)?
            } else {
                let mut buffer = String::new();
                io::stdin().read_to_string(&mut buffer)?;
                buffer
            };

            let mut body = serde_json::json!({ "content": content });
            if let Some(n) = name { body["name"] = n.into(); }

            let res = client.post(format!("{}/api/paste", cli.server)).json(&body).send().await?;
            let paste: Value = res.json().await?;
            println!("Created: {} ({})", paste["name"].as_str().unwrap(), paste["id"].as_str().unwrap());
        }
        Commands::Get { id } => {
            let res = client.get(format!("{}/api/paste/{}", cli.server, id)).send().await?;
            let paste: Value = res.json().await?;
            println!("{}", paste["content"].as_str().unwrap());
        }
        Commands::List { limit } => {
            let res = client.get(format!("{}/api/pastes?limit={}", cli.server, limit)).send().await?;
            let pastes: Vec<Value> = res.json().await?;
            for p in pastes {
                println!("{:<24} {:<30}", p["id"].as_str().unwrap(), p["name"].as_str().unwrap());
            }
        }
        Commands::Search { query } => {
            let res = client.get(format!("{}/api/search?q={}", cli.server, query)).send().await?;
            let pastes: Vec<Value> = res.json().await?;
            for p in pastes {
                println!("{:<24} {:<30}", p["id"].as_str().unwrap(), p["name"].as_str().unwrap());
            }
        }
        Commands::Delete { id } => {
            client.delete(format!("{}/api/paste/{}", cli.server, id)).send().await?;
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