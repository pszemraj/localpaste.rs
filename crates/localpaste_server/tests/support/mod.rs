//! Shared integration-test server bootstrap helpers.

use axum::http::{HeaderMap, HeaderName, StatusCode};
use localpaste_server::{serve_router, AppState, Config, Database, PasteLockManager};
use reqwest::Method;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::future::{Future, IntoFuture};
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::sync::oneshot;

/// Builds a test-friendly server config that targets a provided database path.
///
/// # Arguments
/// - `db_path`: Database file location for the test server instance.
///
/// # Returns
/// A [`Config`] with ephemeral port and test-safe defaults.
///
/// # Panics
/// Panics if `db_path` cannot be represented as UTF-8.
pub fn test_config_for_db_path(db_path: &Path) -> Config {
    Config {
        port: 0,
        db_path: db_path.to_str().expect("db path").to_string(),
        max_paste_size: 10_000_000,
        auto_save_interval: 2000,
        auto_backup: false,
        search_case_sensitive: false,
    }
}

/// Real loopback test server with convenience request helpers.
pub struct TestServer {
    base_url: String,
    client: reqwest::Client,
    shutdown_tx: Option<oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<()>>,
    _temp_dir: Option<TempDir>,
}

impl TestServer {
    fn new(config: Config, temp_dir: Option<TempDir>) -> (Self, Arc<PasteLockManager>) {
        let db = Database::new(config.db_path.as_str()).expect("open db");
        let locks = Arc::new(PasteLockManager::default());
        let state = AppState::with_locks(config, db, locks.clone());
        Self::from_state(state, Some(locks), temp_dir)
    }

    fn from_state(
        state: AppState,
        locks: Option<Arc<PasteLockManager>>,
        temp_dir: Option<TempDir>,
    ) -> (Self, Arc<PasteLockManager>) {
        let locks = locks.unwrap_or_else(|| state.locks.clone());
        let std_listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind listener");
        std_listener
            .set_nonblocking(true)
            .expect("set listener non-blocking");
        let listener = tokio::net::TcpListener::from_std(std_listener).expect("tokio listener");
        let addr = listener.local_addr().expect("listener addr");
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            if let Err(err) = serve_router(listener, state, false, async {
                let _ = shutdown_rx.await;
            })
            .await
            {
                panic!("test server failed: {err}");
            }
        });

        (
            Self {
                base_url: format!("http://{}", addr),
                client: reqwest::Client::new(),
                shutdown_tx: Some(shutdown_tx),
                task: Some(task),
                _temp_dir: temp_dir,
            },
            locks,
        )
    }

    /// Return the listener port assigned to this server.
    ///
    /// # Returns
    /// The loopback TCP port assigned by the OS.
    ///
    /// # Panics
    /// Panics if the stored base URL does not contain a numeric port.
    pub fn port(&self) -> u16 {
        self.base_url
            .rsplit_once(':')
            .and_then(|(_, port)| port.parse().ok())
            .expect("base URL contains listener port")
    }

    /// Build a GET request for `path`.
    ///
    /// # Returns
    /// A pending request builder targeting `path`.
    pub fn get(&self, path: &str) -> RequestBuilder {
        self.request(Method::GET, path)
    }

    /// Build a POST request for `path`.
    ///
    /// # Returns
    /// A pending request builder targeting `path`.
    pub fn post(&self, path: &str) -> RequestBuilder {
        self.request(Method::POST, path)
    }

    /// Build a PUT request for `path`.
    ///
    /// # Returns
    /// A pending request builder targeting `path`.
    pub fn put(&self, path: &str) -> RequestBuilder {
        self.request(Method::PUT, path)
    }

    /// Build a DELETE request for `path`.
    ///
    /// # Returns
    /// A pending request builder targeting `path`.
    pub fn delete(&self, path: &str) -> RequestBuilder {
        self.request(Method::DELETE, path)
    }

    /// Stop the server and wait for the listener task to release its resources.
    ///
    /// # Panics
    /// Panics if the server task panicked or could not be joined.
    pub async fn shutdown(mut self) {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }
        if let Some(task) = self.task.take() {
            task.await.expect("test server task should join cleanly");
        }
    }

    fn request(&self, method: Method, path: &str) -> RequestBuilder {
        let path = path.strip_prefix('/').unwrap_or(path);
        let url = format!("{}/{}", self.base_url, path);
        RequestBuilder {
            inner: self.client.request(method, url),
        }
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

/// Pending request against a [`TestServer`].
pub struct RequestBuilder {
    inner: reqwest::RequestBuilder,
}

impl RequestBuilder {
    /// Add a request header.
    ///
    /// # Arguments
    /// - `name`: Header name to add.
    /// - `value`: Header value to send.
    ///
    /// # Returns
    /// The updated request builder.
    pub fn add_header(self, name: &str, value: &str) -> Self {
        Self {
            inner: self.inner.header(name, value),
        }
    }

    /// Send a JSON request body.
    ///
    /// # Returns
    /// The captured test response.
    pub async fn json<T: Serialize + ?Sized>(self, value: &T) -> TestResponse {
        send_request(self.inner.json(value)).await
    }
}

impl IntoFuture for RequestBuilder {
    type Output = TestResponse;
    type IntoFuture = Pin<Box<dyn Future<Output = TestResponse> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move { send_request(self.inner).await })
    }
}

/// HTTP response captured by the test harness.
pub struct TestResponse {
    status: StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
}

impl TestResponse {
    /// Return the HTTP status code.
    ///
    /// # Returns
    /// The response status code.
    pub fn status_code(&self) -> StatusCode {
        self.status
    }

    /// Decode the response body as JSON.
    ///
    /// # Returns
    /// The deserialized response payload.
    ///
    /// # Panics
    /// Panics if the response body is not valid JSON for `T`.
    pub fn json<T: DeserializeOwned>(&self) -> T {
        serde_json::from_slice(&self.body).expect("response body should be valid JSON")
    }

    /// Assert that a header exactly matches `expected`.
    ///
    /// # Arguments
    /// - `name`: Header name to inspect.
    /// - `expected`: Expected UTF-8 header value.
    ///
    /// # Panics
    /// Panics if the header is missing, not UTF-8, or does not equal `expected`.
    pub fn assert_header(&self, name: &str, expected: &str) {
        let actual = self
            .headers
            .get(name)
            .unwrap_or_else(|| panic!("missing response header {name}"));
        assert_eq!(actual.to_str().expect("header should be UTF-8"), expected);
    }

    /// Assert that a header is present.
    ///
    /// # Panics
    /// Panics if the header is absent.
    pub fn assert_contains_header(&self, name: &str) {
        assert!(self.contains_header(name), "missing response header {name}");
    }

    /// Return whether a header is present.
    ///
    /// # Returns
    /// `true` when `name` parses as an HTTP header name and is present in the response.
    pub fn contains_header(&self, name: &str) -> bool {
        let Ok(name) = HeaderName::from_bytes(name.as_bytes()) else {
            return false;
        };
        self.headers.contains_key(name)
    }
}

async fn send_request(request: reqwest::RequestBuilder) -> TestResponse {
    let response = request.send().await.expect("test request should complete");
    let status = response.status();
    let headers = response.headers().clone();
    let body = response
        .bytes()
        .await
        .expect("test response body should be readable")
        .to_vec();
    TestResponse {
        status,
        headers,
        body,
    }
}

/// Starts an in-process test server from an explicit config.
///
/// # Arguments
/// - `config`: Server configuration to boot with.
///
/// # Returns
/// A ready [`TestServer`] and shared lock manager handle.
pub fn test_server_for_config(config: Config) -> (TestServer, Arc<PasteLockManager>) {
    TestServer::new(config, None)
}

/// Starts a real loopback test server from already-prepared shared state.
///
/// # Arguments
/// - `state`: Server state, including any deliberately seeded database rows.
///
/// # Returns
/// A ready [`TestServer`] and shared lock manager handle.
pub fn test_server_for_state(state: AppState) -> (TestServer, Arc<PasteLockManager>) {
    TestServer::from_state(state, None, None)
}

/// Creates a temporary database and boots a test server bound to it.
///
/// # Returns
/// A running [`TestServer`] and lock manager handle.
///
/// # Panics
/// Panics if the temporary directory, database, or listener cannot be created.
pub fn setup_test_server() -> (TestServer, Arc<PasteLockManager>) {
    let temp_dir = TempDir::new().expect("temp dir");
    let db_path = temp_dir.path().join("test.db");
    let config = test_config_for_db_path(&db_path);
    TestServer::new(config, Some(temp_dir))
}
