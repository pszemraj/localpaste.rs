//! Headless API server entrypoint.

use std::sync::Arc;

use localpaste_core::DEFAULT_PORT;
use localpaste_server::db::ProcessProbeResult;
use localpaste_server::{config::Config, db::Database, serve_router, AppState};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|arg| arg == flag)
}

fn guard_force_unlock_probe(result: ProcessProbeResult) -> anyhow::Result<()> {
    match result {
        ProcessProbeResult::Running => {
            anyhow::bail!(
                "Refusing --force-unlock while a LocalPaste process appears to be running"
            );
        }
        // Uncertain owner detection is treated as unsafe by default.
        ProcessProbeResult::Unknown => {
            anyhow::bail!(
                "Refusing --force-unlock because process ownership could not be verified"
            );
        }
        ProcessProbeResult::NotRunning => Ok(()),
    }
}

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

    if has_flag(&args, "--help") {
        print_help();
        return Ok(());
    }

    let config = Config::from_env();
    let db_exists_before_open = std::path::Path::new(&config.db_path).exists();

    if has_flag(&args, "--force-unlock") {
        guard_force_unlock_probe(localpaste_server::db::localpaste_process_probe())?;
        // Hold owner lock for the full unlock operation to avoid TOCTOU races.
        let _owner_lock_guard =
            localpaste_server::db::lock::acquire_owner_lock_for_lifetime(&config.db_path)?;

        tracing::warn!("Force unlock requested");
        if db_exists_before_open {
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

    if has_flag(&args, "--backup") {
        run_backup(&config)?;
        if args.len() <= 2 {
            return Ok(());
        }
    }

    let database = Database::new(&config.db_path)?;

    if config.auto_backup && db_exists_before_open {
        if let Err(err) = database.flush() {
            tracing::warn!("Failed to flush database before auto-backup: {}", err);
        }
        let backup_manager = localpaste_server::db::backup::BackupManager::new(&config.db_path);
        if let Err(err) = backup_manager.create_backup(database.db.as_ref()) {
            tracing::warn!("Failed to create auto-backup: {}", err);
        }
    }

    let state = AppState::new(config.clone(), database);

    let allow_public = localpaste_server::config::env_flag_enabled("ALLOW_PUBLIC_ACCESS");
    if allow_public {
        tracing::warn!("Public access enabled - server will accept requests from any origin");
    }

    let bind_addr = localpaste_server::resolve_bind_address(&config, allow_public);
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
    println!(
        "  DB_PATH           Database path (default: platform cache dir; Windows: %LOCALAPPDATA%\\\\localpaste\\\\db, Unix: ~/.cache/localpaste/db)"
    );
    println!(
        "  PORT              Server port (default: {})",
        DEFAULT_PORT
    );
    println!("  MAX_PASTE_SIZE    Maximum paste size in bytes (default: 10MB)");
    println!("  ALLOW_PUBLIC_ACCESS  Allow CORS from any origin");
    println!(
        "  BIND              Override bind address (e.g. 0.0.0.0:{})",
        DEFAULT_PORT
    );
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

#[cfg(test)]
mod tests {
    use super::{guard_force_unlock_probe, ProcessProbeResult};

    #[test]
    fn force_unlock_guard_rejects_unknown_probe_results() {
        let err = guard_force_unlock_probe(ProcessProbeResult::Unknown)
            .expect_err("unknown probe should reject force unlock");
        assert!(err.to_string().contains("could not be verified"));
    }

    #[test]
    fn force_unlock_guard_rejects_running_probe_results() {
        let err = guard_force_unlock_probe(ProcessProbeResult::Running)
            .expect_err("running probe should reject force unlock");
        assert!(err.to_string().contains("appears to be running"));
    }

    #[test]
    fn force_unlock_guard_allows_not_running_probe_results() {
        guard_force_unlock_probe(ProcessProbeResult::NotRunning)
            .expect("not-running probe should allow force unlock");
    }
}
