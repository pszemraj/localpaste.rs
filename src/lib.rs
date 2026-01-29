//! Core library wiring for LocalPaste: config, storage, and HTTP routing.

/// HTTP error mapping for API handlers.
pub mod error;
#[cfg(feature = "gui")]
/// egui desktop UI (feature-gated).
pub mod gui;
/// HTTP handlers for paste and folder endpoints.
pub mod handlers;
pub use localpaste_core::{config, db, models, naming, AppError, Config, Database};

use axum::{
    extract::DefaultBodyLimit,
    http::header,
    routing::{delete, get, post, put},
    Router,
};
use hyper::HeaderMap;
use std::sync::Arc;
use tower_http::{
    compression::CompressionLayer, cors::CorsLayer, set_header::SetResponseHeaderLayer,
    trace::TraceLayer,
};

/// Shared state passed to HTTP handlers.
#[derive(Clone)]
pub struct AppState {
    pub db: std::sync::Arc<Database>,
    pub config: std::sync::Arc<Config>,
}

impl AppState {
    /// Construct shared application state.
    ///
    /// # Arguments
    /// - `config`: Loaded configuration.
    /// - `db`: Open database handle.
    ///
    /// # Returns
    /// A new [`AppState`].
    pub fn new(config: Config, db: Database) -> Self {
        Self {
            db: Arc::new(db),
            config: Arc::new(config),
        }
    }
}

/// Create the application router with all routes and middleware.
///
/// # Arguments
/// - `state`: Shared application state.
/// - `allow_public_access`: Whether to allow cross-origin requests from any origin.
///
/// # Returns
/// Configured `axum::Router`.
///
/// # Panics
/// Panics if static header values fail to parse (should not happen).
pub fn create_app(state: AppState, allow_public_access: bool) -> Router {
    // Configure security headers
    let mut default_headers = HeaderMap::new();
    default_headers.insert(header::X_CONTENT_TYPE_OPTIONS, "nosniff".parse().unwrap());
    default_headers.insert(header::X_FRAME_OPTIONS, "DENY".parse().unwrap());
    default_headers.insert(
        header::CONTENT_SECURITY_POLICY,
        "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'self'; form-action 'self'"
            .parse()
            .unwrap(),
    );

    // Configure CORS - optionally allow public access
    let cors = if allow_public_access {
        CorsLayer::new()
            .allow_origin(tower_http::cors::Any)
            .allow_methods([
                axum::http::Method::GET,
                axum::http::Method::POST,
                axum::http::Method::PUT,
                axum::http::Method::DELETE,
            ])
            .allow_headers(tower_http::cors::Any)
    } else {
        let port = state.config.port;
        CorsLayer::new()
            .allow_origin([
                format!("http://localhost:{}", port).parse().unwrap(),
                format!("http://127.0.0.1:{}", port).parse().unwrap(),
            ])
            .allow_methods([
                axum::http::Method::GET,
                axum::http::Method::POST,
                axum::http::Method::PUT,
                axum::http::Method::DELETE,
            ])
            .allow_headers([header::CONTENT_TYPE, header::ACCEPT])
    };

    Router::new()
        // API routes
        .route("/api/paste", post(handlers::paste::create_paste))
        .route("/api/paste/:id", get(handlers::paste::get_paste))
        .route("/api/paste/:id", put(handlers::paste::update_paste))
        .route("/api/paste/:id", delete(handlers::paste::delete_paste))
        .route("/api/pastes", get(handlers::paste::list_pastes))
        .route("/api/search", get(handlers::paste::search_pastes))
        .route("/api/folder", post(handlers::folder::create_folder))
        .route("/api/folder/:id", put(handlers::folder::update_folder))
        .route("/api/folder/:id", delete(handlers::folder::delete_folder))
        .route("/api/folders", get(handlers::folder::list_folders))
        // Note: Static files are not included in the library version
        // Main.rs handles static files with RustEmbed
        // Apply state
        .with_state(state.clone())
        // Apply middleware
        .layer(
            tower::ServiceBuilder::new()
                .layer(DefaultBodyLimit::max(state.config.max_paste_size))
                .layer(TraceLayer::new_for_http())
                .layer(CompressionLayer::new())
                .layer(cors)
                .layer(SetResponseHeaderLayer::overriding(
                    header::CONTENT_SECURITY_POLICY,
                    default_headers
                        .get(header::CONTENT_SECURITY_POLICY)
                        .unwrap()
                        .clone(),
                ))
                .layer(SetResponseHeaderLayer::overriding(
                    header::X_CONTENT_TYPE_OPTIONS,
                    default_headers
                        .get(header::X_CONTENT_TYPE_OPTIONS)
                        .unwrap()
                        .clone(),
                ))
                .layer(SetResponseHeaderLayer::overriding(
                    header::X_FRAME_OPTIONS,
                    default_headers
                        .get(header::X_FRAME_OPTIONS)
                        .unwrap()
                        .clone(),
                )),
        )
}

use std::future::Future;

/// Run the Axum server with graceful shutdown support.
///
/// # Arguments
/// - `listener`: Bound TCP listener for the server.
/// - `state`: Shared application state.
/// - `allow_public_access`: Whether to allow cross-origin requests from any origin.
/// - `shutdown_signal`: Future that resolves when shutdown should start.
///
/// # Returns
/// `Ok(())` when the server exits cleanly.
///
/// # Errors
/// Returns any I/O error produced by `axum::serve`.
pub async fn serve_router(
    listener: tokio::net::TcpListener,
    state: AppState,
    allow_public_access: bool,
    shutdown_signal: impl Future<Output = ()> + Send + 'static,
) -> Result<(), std::io::Error> {
    let app = create_app(state, allow_public_access);
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal)
        .await
}
