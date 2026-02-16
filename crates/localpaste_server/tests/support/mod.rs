//! Shared integration-test server bootstrap helpers.

use axum_test::TestServer;
use localpaste_server::{create_app, AppState, Config, Database, PasteLockManager};
use std::path::Path;
use std::sync::Arc;
use tempfile::TempDir;

pub(crate) fn test_config_for_db_path(db_path: &Path) -> Config {
    Config {
        port: 0,
        db_path: db_path.to_str().expect("db path").to_string(),
        max_paste_size: 10_000_000,
        auto_save_interval: 2000,
        auto_backup: false,
    }
}

pub(crate) fn test_server_for_config(config: Config) -> (TestServer, Arc<PasteLockManager>) {
    let db = Database::new(config.db_path.as_str()).expect("open db");
    let locks = Arc::new(PasteLockManager::default());
    let state = AppState::with_locks(config, db, locks.clone());
    let app = create_app(state, false);
    let server = TestServer::new(app).expect("server");
    (server, locks)
}

pub(crate) fn setup_test_server() -> (TestServer, TempDir, Arc<PasteLockManager>) {
    let temp_dir = TempDir::new().expect("temp dir");
    let db_path = temp_dir.path().join("test.db");
    let config = test_config_for_db_path(&db_path);
    let (server, locks) = test_server_for_config(config);
    (server, temp_dir, locks)
}
