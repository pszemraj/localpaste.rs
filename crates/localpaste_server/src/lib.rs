//! HTTP server wiring for LocalPaste (API, handlers, and shared state).

/// Embedded server helper for GUI integration.
pub mod embedded;
/// HTTP error mapping for API handlers.
pub mod error;
/// HTTP handlers for paste and folder endpoints.
pub mod handlers;
/// In-memory paste locks shared between GUI and API handlers.
pub mod locks;

pub use embedded::EmbeddedServer;
pub use localpaste_core::{config, db, models, naming, AppError, Config, Database, DEFAULT_PORT};
pub use locks::PasteLockManager;

use axum::{
    extract::DefaultBodyLimit,
    http::header,
    routing::{delete, get, post, put},
    Router,
};
use hyper::HeaderMap;
use std::future::Future;
use std::sync::Arc;
use std::net::SocketAddr;
use tower_http::{
    compression::CompressionLayer, cors::CorsLayer, set_header::SetResponseHeaderLayer,
    trace::TraceLayer,
};

/// Shared state passed to HTTP handlers.
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Database>,
    pub config: Arc<Config>,
    pub locks: Arc<PasteLockManager>,
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
        let locks = Arc::new(PasteLockManager::default());
        Self::with_locks(config, db, locks)
    }

    /// Construct shared application state with a pre-configured lock manager.
    ///
    /// # Arguments
    /// - `config`: Loaded configuration.
    /// - `db`: Open database handle.
    /// - `locks`: Shared paste lock manager.
    ///
    /// # Returns
    /// A new [`AppState`] wired to the provided lock manager.
    pub fn with_locks(config: Config, db: Database, locks: Arc<PasteLockManager>) -> Self {
        Self {
            db: Arc::new(db),
            config: Arc::new(config),
            locks,
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
    let cors_port = state.config.port;
    create_app_with_cors_port(state, allow_public_access, cors_port)
}

/// Resolve the listener address from env var overrides and security policy.
///
/// # Arguments
/// - `config`: Server configuration containing the configured `port`.
/// - `allow_public_access`: Whether non-loopback bind targets are permitted.
///
/// # Returns
/// A validated socket address that enforces loopback when public access is disabled.
pub fn resolve_bind_address(config: &Config, allow_public_access: bool) -> SocketAddr {
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

    if allow_public_access || requested.ip().is_loopback() {
        return requested;
    }

    tracing::warn!(
        "Non-loopback bind {} requested without ALLOW_PUBLIC_ACCESS; forcing 127.0.0.1",
        requested
    );
    SocketAddr::from(([127, 0, 0, 1], requested.port()))
}

fn create_app_with_cors_port(state: AppState, allow_public_access: bool, cors_port: u16) -> Router {
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
        CorsLayer::new()
            .allow_origin([
                format!("http://localhost:{}", cors_port).parse().unwrap(),
                format!("http://127.0.0.1:{}", cors_port).parse().unwrap(),
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
        .route("/api/pastes/meta", get(handlers::paste::list_pastes_meta))
        .route("/api/search", get(handlers::paste::search_pastes))
        .route("/api/search/meta", get(handlers::paste::search_pastes_meta))
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

fn listener_cors_port(listener: &tokio::net::TcpListener, fallback_port: u16) -> u16 {
    listener
        .local_addr()
        .map(|addr| addr.port())
        .unwrap_or(fallback_port)
}

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
    let cors_port = listener_cors_port(&listener, state.config.port);
    let app = create_app_with_cors_port(state, allow_public_access, cors_port);
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal)
        .await
}

#[cfg(test)]
mod tests {
    use super::listener_cors_port;
    use super::resolve_bind_address;
    use localpaste_core::DEFAULT_PORT;
    use localpaste_core::Config;
    use std::net::SocketAddr;

    #[tokio::test]
    async fn listener_cors_port_uses_bound_listener_port() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let expected = listener.local_addr().expect("listener addr").port();
        let resolved = listener_cors_port(&listener, DEFAULT_PORT);
        assert_eq!(resolved, expected);
    }

    #[test]
    fn resolve_bind_address_enforces_loopback_when_public_access_disabled() {
        let config = Config {
            db_path: String::from("/tmp/localpaste-db"),
            port: 4040,
            max_paste_size: 1024,
            auto_save_interval: 2000,
            auto_backup: false,
        };
        unsafe {
            std::env::set_var("BIND", "0.0.0.0:4040");
        }
        let resolved = resolve_bind_address(&config, false);
        assert_eq!(resolved.ip().to_string(), "127.0.0.1");
        assert_eq!(resolved.port(), 4040);
        unsafe {
            std::env::remove_var("BIND");
        }
    }

    #[test]
    fn resolve_bind_address_allows_loopback_and_invalid_fallback() {
        let config = Config {
            db_path: String::from("/tmp/localpaste-db"),
            port: 4041,
            max_paste_size: 1024,
            auto_save_interval: 2000,
            auto_backup: false,
        };
        let loopback = resolve_bind_address(&config, false);
        assert_eq!(loopback, SocketAddr::from(([127, 0, 0, 1], 4041)));

        unsafe {
            std::env::set_var("BIND", "bad:host");
        }
        let fallback = resolve_bind_address(&config, false);
        assert_eq!(fallback, SocketAddr::from(([127, 0, 0, 1], 4041)));
        unsafe {
            std::env::remove_var("BIND");
        }
    }
}
