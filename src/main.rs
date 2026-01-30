//! Headless API server entrypoint.

use std::{net::SocketAddr, sync::Arc};

use localpaste::{config::Config, db::Database, serve_router, AppState};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "localpaste=info,tower_http=warn".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let args: Vec<String> = std::env::args().collect();

    if args.contains(&"--help".to_string()) {
        print_help();
        return Ok(());
    }

    let config = Config::from_env();

    if args.contains(&"--force-unlock".to_string()) {
        tracing::warn!("Force unlock requested");
        let lock_manager = localpaste::db::lock::LockManager::new(&config.db_path);
        lock_manager.force_unlock()?;
        tracing::info!("Lock removed successfully");
    }

    if args.contains(&"--backup".to_string()) {
        run_backup(&config)?;
        if args.len() <= 2 {
            return Ok(());
        }
    }

    if config.auto_backup && std::path::Path::new(&config.db_path).exists() {
        if let Err(err) = localpaste::db::lock::LockManager::backup_database(&config.db_path) {
            tracing::warn!("Failed to create auto-backup: {}", err);
        }
    }

    let database = Database::new(&config.db_path)?;
    let state = AppState::new(config.clone(), database);

    let allow_public = std::env::var("ALLOW_PUBLIC_ACCESS").is_ok();
    if allow_public {
        tracing::warn!("Public access enabled - server will accept requests from any origin");
    }

    let bind_addr = resolve_bind_address(&config);
    if !bind_addr.ip().is_loopback() {
        tracing::warn!(
            "Binding to non-localhost address: {} - ensure proper security measures are in place",
            bind_addr
        );
    }

    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    tracing::info!("LocalPaste running at http://{}", bind_addr);

    let db = state.db.clone();
    serve_router(listener, state, allow_public, shutdown_signal(db)).await?;

    Ok(())
}

fn print_help() {
    println!("LocalPaste Server\n");
    println!("Usage: localpaste [OPTIONS]\n");
    println!("Options:");
    println!("  --force-unlock    Remove stale database locks");
    println!("  --backup          Create a backup of the database");
    println!("  --help            Show this help message");
    println!("\nEnvironment variables:");
    println!("  DB_PATH           Database path (default: ~/.cache/localpaste/db)");
    println!("  PORT              Server port (default: 38411)");
    println!("  MAX_PASTE_SIZE    Maximum paste size in bytes (default: 10MB)");
    println!("  ALLOW_PUBLIC_ACCESS  Allow CORS from any origin");
    println!("  BIND              Override bind address (e.g. 0.0.0.0:38411)");
}

fn run_backup(config: &Config) -> anyhow::Result<()> {
    if std::path::Path::new(&config.db_path).exists() {
        let temp_db = Database::new(&config.db_path)?;
        temp_db.flush().ok();

        let backup_manager = localpaste::db::backup::BackupManager::new(&config.db_path);
        let backup_path = backup_manager.create_backup(temp_db.db.as_ref())?;
        println!("✅ Database backed up to: {}", backup_path);
    } else {
        println!("ℹ️  No existing database to backup");
    }
    Ok(())
}

fn resolve_bind_address(config: &Config) -> SocketAddr {
    std::env::var("BIND")
        .ok()
        .and_then(|s| s.parse::<SocketAddr>().ok())
        .unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], config.port)))
}

async fn shutdown_signal(db: Arc<Database>) {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("Shutting down gracefully...");

    if let Err(err) = db.flush() {
        tracing::error!("Failed to flush database: {}", err);
    } else {
        tracing::info!("Database flushed successfully");
    }
}
