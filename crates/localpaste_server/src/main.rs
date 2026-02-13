//! Headless API server entrypoint.

use std::{net::SocketAddr, sync::Arc};

use localpaste_server::{config::Config, db::Database, serve_router, AppState};
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
        if localpaste_server::db::is_localpaste_running() {
            anyhow::bail!(
                "Refusing --force-unlock while a LocalPaste process appears to be running"
            );
        }

        tracing::warn!("Force unlock requested");
        if std::path::Path::new(&config.db_path).exists() {
            let backup_path =
                localpaste_server::db::lock::LockManager::backup_database(&config.db_path)?;
            if !backup_path.is_empty() {
                tracing::info!("Database backup created at {}", backup_path);
            }
        }
        let lock_manager = localpaste_server::db::lock::LockManager::new(&config.db_path);
        let removed_count = lock_manager.force_unlock()?;
        if removed_count == 0 {
            tracing::info!("No known lock files found");
        } else {
            tracing::info!("Removed {} lock file(s)", removed_count);
        }
        if args.len() <= 2 {
            return Ok(());
        }
    }

    if args.contains(&"--backup".to_string()) {
        run_backup(&config)?;
        if args.len() <= 2 {
            return Ok(());
        }
    }

    if config.auto_backup && std::path::Path::new(&config.db_path).exists() {
        if let Err(err) = localpaste_server::db::lock::LockManager::backup_database(&config.db_path)
        {
            tracing::warn!("Failed to create auto-backup: {}", err);
        }
    }

    let database = Database::new(&config.db_path)?;
    let state = AppState::new(config.clone(), database);

    let allow_public = localpaste_server::config::env_flag_enabled("ALLOW_PUBLIC_ACCESS");
    if allow_public {
        tracing::warn!("Public access enabled - server will accept requests from any origin");
    }

    let bind_addr = resolve_bind_address(&config, allow_public);
    if !bind_addr.ip().is_loopback() {
        tracing::warn!(
            "Binding to non-localhost address: {} - ensure proper security measures are in place",
            bind_addr
        );
    }

    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    let actual_addr = listener.local_addr().unwrap_or(bind_addr);
    tracing::info!("LocalPaste running at http://{}", actual_addr);

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

        let backup_manager = localpaste_server::db::backup::BackupManager::new(&config.db_path);
        let backup_path = backup_manager.create_backup(temp_db.db.as_ref())?;
        println!("✅ Database backed up to: {}", backup_path);
    } else {
        println!("ℹ️  No existing database to backup");
    }
    Ok(())
}

fn resolve_bind_address(config: &Config, allow_public: bool) -> SocketAddr {
    let default_bind = SocketAddr::from(([127, 0, 0, 1], config.port));
    let requested = match std::env::var("BIND") {
        Ok(value) => match value.trim().parse::<SocketAddr>() {
            Ok(addr) => addr,
            Err(err) => {
                tracing::warn!(
                    "Invalid BIND='{}': {}. Falling back to {}",
                    value,
                    err,
                    default_bind
                );
                default_bind
            }
        },
        Err(_) => default_bind,
    };

    if allow_public || requested.ip().is_loopback() {
        return requested;
    }

    tracing::warn!(
        "Non-loopback bind {} requested without ALLOW_PUBLIC_ACCESS; forcing 127.0.0.1",
        requested
    );
    SocketAddr::from(([127, 0, 0, 1], requested.port()))
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
