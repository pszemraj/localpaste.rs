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
pub use locks::{LockOwnerId, PasteLockError, PasteLockManager, PasteMutationGuard};

use axum::{
    extract::DefaultBodyLimit,
    http::{header, HeaderName, HeaderValue},
    routing::{delete, get, post, put},
    Router,
};
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use tower_http::{
    compression::CompressionLayer,
    cors::{AllowOrigin, CorsLayer},
    set_header::SetResponseHeaderLayer,
    trace::TraceLayer,
};

const JSON_BODY_OVERHEAD_BYTES: usize = 16 * 1024;
const JSON_STRING_ESCAPE_EXPANSION_FACTOR: usize = 6;
const MAX_JSON_REQUEST_BODY_BYTES: usize = 256 * 1024 * 1024;
const CSP_HEADER_VALUE: &str = "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'self'; form-action 'self'";
const X_CONTENT_TYPE_OPTIONS_NOSNIFF: &str = "nosniff";
const X_FRAME_OPTIONS_DENY: &str = "DENY";
const X_LOCALPASTE_SERVER_HEADER: &str = "x-localpaste-server";
const X_LOCALPASTE_SERVER_VALUE: &str = "1";

fn uncapped_request_body_limit(max_paste_size: usize) -> usize {
    max_paste_size
        // Worst-case JSON string expansion is \u00XX (6 bytes) per decoded byte.
        .saturating_mul(JSON_STRING_ESCAPE_EXPANSION_FACTOR)
        .saturating_add(JSON_BODY_OVERHEAD_BYTES)
}

fn request_body_limit(max_paste_size: usize) -> usize {
    uncapped_request_body_limit(max_paste_size).min(MAX_JSON_REQUEST_BODY_BYTES)
}

fn parse_http_origin_uri(origin: &HeaderValue) -> Option<axum::http::Uri> {
    let origin = match origin.to_str() {
        Ok(value) => value,
        Err(_) => return None,
    };
    let uri = match origin.parse::<axum::http::Uri>() {
        Ok(value) => value,
        Err(_) => return None,
    };
    let scheme = uri.scheme_str()?;
    if scheme != "http" && scheme != "https" {
        return None;
    }
    Some(uri)
}

fn is_loopback_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    let normalized_host = host
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(host);
    normalized_host
        .parse::<IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false)
}

fn origin_port(uri: &axum::http::Uri) -> Option<u16> {
    if let Some(port) = uri.port_u16() {
        return Some(port);
    }
    match uri.scheme_str() {
        Some("http") => Some(80),
        Some("https") => Some(443),
        _ => None,
    }
}

fn is_loopback_origin(origin: &HeaderValue) -> bool {
    let uri = match parse_http_origin_uri(origin) {
        Some(value) => value,
        None => return false,
    };
    let host = match uri.host() {
        Some(value) => value,
        None => return false,
    };
    is_loopback_host(host)
}

fn is_loopback_origin_for_listener_port(origin: &HeaderValue, listener_port: u16) -> bool {
    if !is_loopback_origin(origin) {
        return false;
    }
    let uri = match parse_http_origin_uri(origin) {
        Some(value) => value,
        None => return false,
    };
    origin_port(&uri) == Some(listener_port)
}

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
pub fn create_app(state: AppState, allow_public_access: bool) -> Router {
    let listener_port = state.config.port;
    create_app_with_cors(state, allow_public_access, listener_port)
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

fn create_app_with_cors(state: AppState, allow_public_access: bool, listener_port: u16) -> Router {
    let uncapped_body_limit = uncapped_request_body_limit(state.config.max_paste_size);
    let body_limit = request_body_limit(state.config.max_paste_size);
    if body_limit < uncapped_body_limit {
        tracing::warn!(
            configured_max_paste_size = state.config.max_paste_size,
            body_limit_bytes = body_limit,
            uncapped_limit_bytes = uncapped_body_limit,
            hard_limit_bytes = MAX_JSON_REQUEST_BODY_BYTES,
            "Configured max_paste_size implies an excessive transport body limit; applying safety cap",
        );
    }

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
            .allow_origin(AllowOrigin::predicate(move |origin, _| {
                is_loopback_origin_for_listener_port(origin, listener_port)
            }))
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
                // Body limit allows for worst-case JSON escaping. Decoded content bytes
                // are validated separately in handlers against `max_paste_size`.
                .layer(DefaultBodyLimit::max(body_limit))
                .layer(TraceLayer::new_for_http())
                .layer(CompressionLayer::new())
                .layer(cors)
                .layer(SetResponseHeaderLayer::overriding(
                    header::CONTENT_SECURITY_POLICY,
                    HeaderValue::from_static(CSP_HEADER_VALUE),
                ))
                .layer(SetResponseHeaderLayer::overriding(
                    header::X_CONTENT_TYPE_OPTIONS,
                    HeaderValue::from_static(X_CONTENT_TYPE_OPTIONS_NOSNIFF),
                ))
                .layer(SetResponseHeaderLayer::overriding(
                    header::X_FRAME_OPTIONS,
                    HeaderValue::from_static(X_FRAME_OPTIONS_DENY),
                ))
                .layer(SetResponseHeaderLayer::overriding(
                    HeaderName::from_static(X_LOCALPASTE_SERVER_HEADER),
                    HeaderValue::from_static(X_LOCALPASTE_SERVER_VALUE),
                )),
        )
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
    let listener_port = listener
        .local_addr()
        .map(|addr| addr.port())
        .unwrap_or(state.config.port);
    let app = create_app_with_cors(state, allow_public_access, listener_port);
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal)
        .await
}

#[cfg(test)]
mod tests {
    use super::is_loopback_origin;
    use super::is_loopback_origin_for_listener_port;
    use super::request_body_limit;
    use super::resolve_bind_address;
    use super::JSON_BODY_OVERHEAD_BYTES;
    use super::JSON_STRING_ESCAPE_EXPANSION_FACTOR;
    use super::MAX_JSON_REQUEST_BODY_BYTES;
    use axum::http::HeaderValue;
    use localpaste_core::env::{env_lock, EnvGuard};
    use localpaste_core::Config;
    use std::net::SocketAddr;

    #[test]
    fn request_body_limit_accounts_for_json_escape_worst_case() {
        let max_paste_size = 128usize;
        let expected = max_paste_size
            .saturating_mul(JSON_STRING_ESCAPE_EXPANSION_FACTOR)
            .saturating_add(JSON_BODY_OVERHEAD_BYTES);
        assert_eq!(request_body_limit(max_paste_size), expected);
    }

    #[test]
    fn request_body_limit_applies_safety_cap() {
        assert_eq!(request_body_limit(usize::MAX), MAX_JSON_REQUEST_BODY_BYTES);
    }

    #[test]
    fn loopback_origin_detection_matrix_covers_valid_loopback_and_rejections() {
        let cases = [
            ("http://localhost:3000", true),
            ("http://127.0.0.2:3000", true),
            ("http://[::1]:3000", true),
            ("http://example.com:3000", false),
            ("null", false),
            ("not-a-uri", false),
        ];

        for (origin, expected) in cases {
            assert_eq!(
                is_loopback_origin(&HeaderValue::from_static(origin)),
                expected,
                "origin: {}",
                origin
            );
        }
    }

    #[test]
    fn strict_loopback_origin_requires_listener_port_match() {
        let listener_port = 3055;
        let cases = [
            ("http://localhost:3055", true),
            ("http://127.0.0.1:3055", true),
            ("http://[::1]:3055", true),
            ("http://localhost:3000", false),
            ("http://127.0.0.1:3000", false),
            ("https://localhost:3055", true),
            ("https://localhost", false),
            ("http://localhost", false),
            ("http://example.com:3055", false),
            ("null", false),
        ];

        for (origin, expected) in cases {
            assert_eq!(
                is_loopback_origin_for_listener_port(
                    &HeaderValue::from_static(origin),
                    listener_port
                ),
                expected,
                "origin: {}",
                origin
            );
        }
    }

    #[test]
    fn strict_loopback_origin_uses_default_ports_when_omitted() {
        assert!(is_loopback_origin_for_listener_port(
            &HeaderValue::from_static("http://localhost"),
            80
        ));
        assert!(is_loopback_origin_for_listener_port(
            &HeaderValue::from_static("https://localhost"),
            443
        ));
    }

    #[test]
    fn resolve_bind_address_enforces_loopback_when_public_access_disabled() {
        let _lock = env_lock().lock().expect("env lock");
        let config = Config {
            db_path: String::from("/tmp/localpaste-db"),
            port: 4040,
            max_paste_size: 1024,
            auto_save_interval: 2000,
            auto_backup: false,
        };
        let _bind = EnvGuard::set("BIND", "0.0.0.0:4040");
        let resolved = resolve_bind_address(&config, false);
        assert_eq!(resolved.ip().to_string(), "127.0.0.1");
        assert_eq!(resolved.port(), 4040);
    }

    #[test]
    fn resolve_bind_address_allows_loopback_and_invalid_fallback() {
        let _lock = env_lock().lock().expect("env lock");
        let config = Config {
            db_path: String::from("/tmp/localpaste-db"),
            port: 4041,
            max_paste_size: 1024,
            auto_save_interval: 2000,
            auto_backup: false,
        };
        let loopback = resolve_bind_address(&config, false);
        assert_eq!(loopback, SocketAddr::from(([127, 0, 0, 1], 4041)));

        let _bind = EnvGuard::set("BIND", "bad:host");
        let fallback = resolve_bind_address(&config, false);
        assert_eq!(fallback, SocketAddr::from(([127, 0, 0, 1], 4041)));
    }
}
