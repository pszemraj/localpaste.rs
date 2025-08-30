use axum::{
    http::{header, StatusCode, Uri},
    response::{Html, IntoResponse, Response},
    routing::{delete, get, post, put},
    Router,
};
use rust_embed::RustEmbed;
use std::{net::SocketAddr, sync::Arc};
use tower_http::{
    compression::CompressionLayer,
    cors::{Any, CorsLayer},
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
                .unwrap_or_else(|_| "localpaste=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = Arc::new(Config::from_env());

    std::fs::create_dir_all("./data")?;
    let db = Arc::new(Database::new(&config.db_path)?);

    let state = AppState {
        db,
        config: config.clone(),
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
        .route("/api/folder/:id", delete(handlers::folder::delete_folder))
        .fallback(static_handler)
        .layer(
            tower::ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                .layer(CompressionLayer::new())
                .layer(
                    CorsLayer::new()
                        .allow_origin(Any)
                        .allow_methods(Any)
                        .allow_headers(Any),
                ),
        )
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], config.port));
    let listener = tokio::net::TcpListener::bind(addr).await?;

    tracing::info!("🚀 LocalPaste running at http://{}", addr);
    tracing::info!("📦 Single binary, zero runtime dependencies!");

    axum::serve(listener, app).await?;

    Ok(())
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
