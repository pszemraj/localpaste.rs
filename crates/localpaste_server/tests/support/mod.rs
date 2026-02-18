//! Shared integration-test server bootstrap helpers.

use axum_test::TestServer;
use localpaste_server::{create_app, AppState, Config, Database, PasteLockManager};
use std::path::Path;
use std::sync::Arc;
use tempfile::TempDir;

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
pub(crate) fn test_config_for_db_path(db_path: &Path) -> Config {
    Config {
        port: 0,
        db_path: db_path.to_str().expect("db path").to_string(),
        max_paste_size: 10_000_000,
        auto_save_interval: 2000,
        auto_backup: false,
    }
}

/// Starts an in-process test server from an explicit config.
///
/// # Arguments
/// - `config`: Server configuration to boot with.
///
/// # Returns
/// A ready [`TestServer`] and shared lock manager handle.
///
/// # Panics
/// Panics if the database cannot be opened or the test server cannot start.
pub(crate) fn test_server_for_config(config: Config) -> (TestServer, Arc<PasteLockManager>) {
    let db = Database::new(config.db_path.as_str()).expect("open db");
    let locks = Arc::new(PasteLockManager::default());
    let state = AppState::with_locks(config, db, locks.clone());
    let app = create_app(state, false);
    let server = TestServer::new(app).expect("server");
    (server, locks)
}

/// Creates a temporary database and boots a test server bound to it.
///
/// # Returns
/// A running [`TestServer`], owning [`TempDir`], and lock manager handle.
///
/// # Panics
/// Panics if temporary directory creation or server bootstrap fails.
pub(crate) fn setup_test_server() -> (TestServer, TempDir, Arc<PasteLockManager>) {
    let temp_dir = TempDir::new().expect("temp dir");
    let db_path = temp_dir.path().join("test.db");
    let config = test_config_for_db_path(&db_path);
    let (server, locks) = test_server_for_config(config);
    (server, temp_dir, locks)
}
