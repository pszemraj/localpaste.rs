use axum::{
    extract::DefaultBodyLimit,
    http::{header, HeaderValue, Method, StatusCode, Uri},
    response::{Html, IntoResponse, Response},
    routing::{delete, get, post, put},
    Router,
};
use rust_embed::RustEmbed;
use std::{net::SocketAddr, sync::Arc};
use tower::ServiceBuilder;
use tower_http::{
    compression::CompressionLayer, cors::CorsLayer, set_header::SetResponseHeaderLayer,
    trace::TraceLayer,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod config;
mod db;
mod error;
mod handlers;
mod models;
mod naming;

use crate::{config::Config, db::Database};

#[derive(RustEmbed)]
#[folder = "src/static/"]
struct Assets;

#[derive(Clone)]
pub struct AppState {
    db: Arc<Database>,
    config: Arc<Config>,
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

    let config = Arc::new(Config::from_env());

    // Handle command-line arguments for database management
    let args: Vec<String> = std::env::args().collect();

    if args.contains(&"--help".to_string()) {
        println!("LocalPaste Server\n");
        println!("Usage: localpaste [OPTIONS]\n");
        println!("Options:");
        println!("  --force-unlock    Remove stale database locks");
        println!("  --backup          Create a backup of the database");
        println!("  --help            Show this help message");
        println!("\nEnvironment variables:");
        println!("  DB_PATH           Database path (default: ~/.cache/localpaste/db)");
        println!("  PORT              Server port (default: 3030)");
        println!("  MAX_PASTE_SIZE    Maximum paste size in bytes (default: 10MB)");
        return Ok(());
    }

    if args.contains(&"--force-unlock".to_string()) {
        tracing::warn!("Force unlock requested");
        let lock_manager = crate::db::lock::LockManager::new(&config.db_path);
        lock_manager.force_unlock()?;
        tracing::info!("Lock removed successfully");
    }

    if args.contains(&"--backup".to_string()) {
        if std::path::Path::new(&config.db_path).exists() {
            // Flush database before backup if it's open
            if let Ok(temp_db) = Database::new(&config.db_path) {
                temp_db.flush().ok();
            }

            let backup_manager = crate::db::backup::BackupManager::new(&config.db_path);
            let backup_path = backup_manager.create_backup(&sled::open(&config.db_path)?)?;
            println!("✅ Database backed up to: {}", backup_path);
        } else {
            println!("ℹ️  No existing database to backup");
        }
        // Exit after backup unless other non-flag arguments are present
        if args.len() <= 2 {
            // program name + --backup
            return Ok(());
        }
    }

    // Auto-backup on startup if explicitly enabled and database exists
    if config.auto_backup && std::path::Path::new(&config.db_path).exists() {
        match crate::db::lock::LockManager::backup_database(&config.db_path) {
            Ok(backup_path) if !backup_path.is_empty() => {
                tracing::debug!("Auto-backup created at: {}", backup_path);
            }
            Err(e) => {
                tracing::warn!("Failed to create auto-backup: {}", e);
                // Continue anyway - backup failure shouldn't prevent startup
            }
            _ => {}
        }
    }

    let db = Arc::new(Database::new(&config.db_path)?);

    let state = AppState {
        db: db.clone(),
        config: config.clone(),
    };

    // Configure CORS - restrict to localhost by default
    let cors = if std::env::var("ALLOW_PUBLIC_ACCESS").is_ok() {
        tracing::warn!("⚠️  Public access enabled - server will accept requests from any origin");
        CorsLayer::new()
            .allow_origin(tower_http::cors::Any)
            .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
            .allow_headers(tower_http::cors::Any)
    } else {
        CorsLayer::new()
            .allow_origin([
                "http://localhost:3030".parse::<HeaderValue>().unwrap(),
                "http://127.0.0.1:3030".parse::<HeaderValue>().unwrap(),
                format!("http://localhost:{}", config.port)
                    .parse::<HeaderValue>()
                    .unwrap(),
                format!("http://127.0.0.1:{}", config.port)
                    .parse::<HeaderValue>()
                    .unwrap(),
            ])
            .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
            .allow_headers(tower_http::cors::Any)
    };

    let app = Router::new()
        .route("/api/paste", post(handlers::paste::create_paste))
        .route("/api/paste/:id", get(handlers::paste::get_paste))
        .route("/api/paste/:id", put(handlers::paste::update_paste))
        .route("/api/paste/:id", delete(handlers::paste::delete_paste))
        .route("/api/pastes", get(handlers::paste::list_pastes))
        .route("/api/search", get(handlers::paste::search_pastes))
        .route("/api/folder", post(handlers::folder::create_folder))
        .route("/api/folders", get(handlers::folder::list_folders))
        .route("/api/folder/:id", put(handlers::folder::update_folder))
        .route("/api/folder/:id", delete(handlers::folder::delete_folder))
        .fallback(static_handler)
        .layer(
            ServiceBuilder::new()
                // Request body size limit
                .layer(DefaultBodyLimit::max(config.max_paste_size))
                // Tracing
                .layer(TraceLayer::new_for_http())
                // Compression
                .layer(CompressionLayer::new())
                // Security headers
                .layer(SetResponseHeaderLayer::if_not_present(
                    header::CONTENT_SECURITY_POLICY,
                    HeaderValue::from_static("default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'self'; frame-ancestors 'none';"),
                ))
                .layer(SetResponseHeaderLayer::if_not_present(
                    header::X_CONTENT_TYPE_OPTIONS,
                    HeaderValue::from_static("nosniff"),
                ))
                .layer(SetResponseHeaderLayer::if_not_present(
                    header::X_FRAME_OPTIONS,
                    HeaderValue::from_static("DENY"),
                ))
                .layer(SetResponseHeaderLayer::if_not_present(
                    header::REFERRER_POLICY,
                    HeaderValue::from_static("no-referrer"),
                ))
                // CORS
                .layer(cors),
        )
        .with_state(state);

    // Bind to localhost by default, allow override via BIND env var
    let bind_addr = std::env::var("BIND")
        .ok()
        .and_then(|s| s.parse::<SocketAddr>().ok())
        .unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], config.port)));

    // Warn if binding to non-localhost
    if !bind_addr.ip().is_loopback() {
        tracing::warn!("⚠️  Binding to non-localhost address: {} - ensure proper security measures are in place", bind_addr);
    }

    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    tracing::info!("🚀 LocalPaste running at http://{}", bind_addr);

    // Setup graceful shutdown
    let server = axum::serve(listener, app).with_graceful_shutdown(shutdown_signal(db));

    server.await?;

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

    // Flush the database
    if let Err(e) = db.flush() {
        tracing::error!("Failed to flush database: {}", e);
    } else {
        tracing::info!("Database flushed successfully");
    }
}

async fn static_handler(uri: Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');

    if path.is_empty() {
        return serve_asset("index.html");
    }

    serve_asset(path)
}

fn serve_asset(path: &str) -> Response {
    match Assets::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            ([(header::CONTENT_TYPE, mime.as_ref())], content.data).into_response()
        }
        None => match Assets::get("index.html") {
            Some(content) => Html(content.data).into_response(),
            None => (StatusCode::NOT_FOUND, "Not found").into_response(),
        },
    }
}
