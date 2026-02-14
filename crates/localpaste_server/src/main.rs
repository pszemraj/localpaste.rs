//! Headless API server entrypoint.

use localpaste_core::DEFAULT_PORT;
use localpaste_server::{config::Config, db::Database, serve_router, AppState};
use std::net::SocketAddr;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct CliFlags {
    help: bool,
    backup: bool,
}

fn parse_cli_flags(args: &[String]) -> anyhow::Result<CliFlags> {
    let mut flags = CliFlags::default();
    for arg in args.iter().skip(1) {
        match arg.as_str() {
            "--help" => flags.help = true,
            "--backup" => flags.backup = true,
            value if value.starts_with('-') => {
                anyhow::bail!(
                    "Unknown option: '{}'. Use --help to see supported options.",
                    value
                );
            }
            value => {
                anyhow::bail!(
                    "Unexpected positional argument: '{}'. Use --help to see supported options.",
                    value
                );
            }
        }
    }
    Ok(flags)
}

fn runs_maintenance_mode(flags: CliFlags) -> bool {
    flags.backup
}

fn validate_bind_override(allow_public_access: bool) -> anyhow::Result<()> {
    let Ok(raw) = std::env::var("BIND") else {
        return Ok(());
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        anyhow::bail!("BIND is set but empty");
    }
    let parsed: SocketAddr = trimmed
        .parse()
        .map_err(|err| anyhow::anyhow!("Invalid BIND='{}': {}", raw, err))?;
    if !allow_public_access && !parsed.ip().is_loopback() {
        anyhow::bail!(
            "BIND='{}' requires ALLOW_PUBLIC_ACCESS=1 for non-loopback addresses",
            raw
        );
    }
    Ok(())
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
    let cli_flags = parse_cli_flags(&args)?;

    if cli_flags.help {
        print_help();
        return Ok(());
    }

    let config = Config::from_env_strict().map_err(anyhow::Error::msg)?;
    let db_exists_before_open = std::path::Path::new(&config.db_path).exists();

    if cli_flags.backup {
        run_backup(&config)?;
    }

    if runs_maintenance_mode(cli_flags) {
        return Ok(());
    }

    let database = Database::new(&config.db_path)?;

    if config.auto_backup && db_exists_before_open {
        let backup_manager = localpaste_server::db::backup::BackupManager::new(&config.db_path);
        if let Err(err) = backup_manager.create_backup(database.db.as_ref()) {
            tracing::warn!("Failed to create auto-backup: {}", err);
        }
    }

    let state = AppState::new(config.clone(), database);

    let allow_public =
        localpaste_server::config::parse_bool_env_strict("ALLOW_PUBLIC_ACCESS", false)
            .map_err(anyhow::Error::msg)?;
    validate_bind_override(allow_public)?;
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

    let serve_result = serve_router(listener, state, allow_public, shutdown_signal()).await;

    serve_result?;

    Ok(())
}

fn print_help() {
    println!("LocalPaste Server\n");
    println!("Usage: localpaste [OPTIONS]\n");
    println!("Options:");
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
    println!(
        "  AUTO_BACKUP       Create backup at startup when DB already exists (1/0/true/false)"
    );
    println!("  ALLOW_PUBLIC_ACCESS  Allow CORS from any origin");
    println!(
        "  BIND              Override bind address (e.g. 0.0.0.0:{})",
        DEFAULT_PORT
    );
    println!("  (malformed env values fail startup instead of silently defaulting)");
    println!("\nSide effects:");
    println!("  --backup          Writes a consistent backup copy of data.redb");
}

fn run_backup(config: &Config) -> anyhow::Result<()> {
    if std::path::Path::new(&config.db_path).exists() {
        let temp_db = Database::new(&config.db_path)?;

        let backup_manager = localpaste_server::db::backup::BackupManager::new(&config.db_path);
        let backup_path = backup_manager.create_backup(temp_db.db.as_ref())?;
        println!("Database backed up to: {}", backup_path);
    } else {
        println!("No existing database to backup");
    }
    Ok(())
}

async fn shutdown_signal() {
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
}

#[cfg(test)]
mod tests {
    use super::{parse_cli_flags, runs_maintenance_mode, validate_bind_override, CliFlags};
    use localpaste_core::env::{env_lock, EnvGuard};

    #[test]
    fn parse_cli_flags_rejects_unknown_and_positional_arguments() {
        let cases = [
            (
                vec!["localpaste".to_string(), "--force-unlock".to_string()],
                "Unknown option",
            ),
            (
                vec!["localpaste".to_string(), "backup".to_string()],
                "Unexpected positional argument",
            ),
        ];

        for (args, expected_fragment) in cases {
            let err = parse_cli_flags(&args).expect_err("invalid args should be rejected");
            assert!(err.to_string().contains(expected_fragment));
        }
    }

    #[test]
    fn parse_cli_flags_accepts_supported_options() {
        let args = vec!["localpaste".to_string(), "--backup".to_string()];
        let flags = parse_cli_flags(&args).expect("known options should parse");
        assert_eq!(
            flags,
            CliFlags {
                help: false,
                backup: true,
            }
        );
    }

    #[test]
    fn maintenance_flags_enable_maintenance_mode() {
        let backup_only = CliFlags {
            backup: true,
            ..CliFlags::default()
        };
        let none = CliFlags::default();
        assert!(runs_maintenance_mode(backup_only));
        assert!(!runs_maintenance_mode(none));
    }

    #[test]
    fn validate_bind_override_rejects_invalid_and_non_loopback_without_public_access() {
        let _lock = env_lock().lock().expect("env lock");

        let _bind = EnvGuard::set("BIND", "not-an-addr");
        let err = validate_bind_override(false).expect_err("invalid bind should fail");
        assert!(err.to_string().contains("Invalid BIND"));
        drop(_bind);

        let _bind = EnvGuard::set("BIND", "0.0.0.0:38411");
        let err = validate_bind_override(false).expect_err("non-loopback bind should fail");
        assert!(err.to_string().contains("ALLOW_PUBLIC_ACCESS=1"));

        validate_bind_override(true).expect("public access should allow non-loopback bind");
    }
}
