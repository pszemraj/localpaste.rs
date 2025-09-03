pub mod config;
pub mod db;
pub mod error;
pub mod handlers;
pub mod models;
pub mod naming;

pub use config::Config;
pub use db::Database;
pub use error::AppError;

use axum::{
    extract::DefaultBodyLimit,
    http::header,
    Router,
    routing::{get, post, put, delete},
};
use tower_http::{
    compression::CompressionLayer,
    cors::CorsLayer,
    set_header::SetResponseHeaderLayer,
};
use hyper::HeaderMap;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub db: std::sync::Arc<Database>,
    pub config: std::sync::Arc<Config>,
}

impl AppState {
    pub fn new(config: Config, db: Database) -> Self {
        Self {
            db: Arc::new(db),
            config: Arc::new(config),
        }
    }
}

/// Create the application router with all routes and middleware
pub fn create_app(state: AppState) -> Router {
    // Configure security headers
    let mut default_headers = HeaderMap::new();
    default_headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        "nosniff".parse().unwrap(),
    );
    default_headers.insert(
        header::X_FRAME_OPTIONS,
        "DENY".parse().unwrap(),
    );
    default_headers.insert(
        header::CONTENT_SECURITY_POLICY,
        "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'self'; form-action 'self'"
            .parse()
            .unwrap(),
    );
    
    // Configure CORS - only allow localhost origins
    let cors = CorsLayer::new()
        .allow_origin([
            "http://localhost:3000".parse().unwrap(),
            "http://localhost:3030".parse().unwrap(),
            "http://localhost:8080".parse().unwrap(),
            "http://127.0.0.1:3000".parse().unwrap(),
            "http://127.0.0.1:3030".parse().unwrap(),
            "http://127.0.0.1:8080".parse().unwrap(),
        ])
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::PUT,
            axum::http::Method::DELETE,
        ])
        .allow_headers([
            header::CONTENT_TYPE,
            header::ACCEPT,
        ]);
    
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
                .layer(CompressionLayer::new())
                .layer(cors)
                .layer(SetResponseHeaderLayer::overriding(
                    header::CONTENT_SECURITY_POLICY,
                    default_headers.get(header::CONTENT_SECURITY_POLICY).unwrap().clone(),
                ))
                .layer(SetResponseHeaderLayer::overriding(
                    header::X_CONTENT_TYPE_OPTIONS,
                    default_headers.get(header::X_CONTENT_TYPE_OPTIONS).unwrap().clone(),
                ))
                .layer(SetResponseHeaderLayer::overriding(
                    header::X_FRAME_OPTIONS,
                    default_headers.get(header::X_FRAME_OPTIONS).unwrap().clone(),
                ))
        )
}
