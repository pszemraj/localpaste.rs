//! Regression tests for explicit language handling on paste creation.

use axum::http::StatusCode;
use axum_test::TestServer;
use localpaste_server::{create_app, AppState, Config, Database, PasteLockManager};
use serde_json::json;
use std::sync::Arc;
use tempfile::TempDir;

fn setup_test_server() -> (TestServer, TempDir) {
    let temp_dir = TempDir::new().expect("temp dir");
    let db_path = temp_dir.path().join("test.db");
    let config = Config {
        port: 0,
        db_path: db_path.to_string_lossy().to_string(),
        max_paste_size: 10_000_000,
        auto_save_interval: 2000,
        auto_backup: false,
    };
    let db = Database::new(&config.db_path).expect("open db");
    let locks = Arc::new(PasteLockManager::default());
    let state = AppState::with_locks(config, db, locks);
    let app = create_app(state, false);
    let server = TestServer::new(app).expect("server");
    (server, temp_dir)
}

#[tokio::test]
async fn test_create_paste_respects_explicit_language_even_when_content_differs() {
    let (server, _temp) = setup_test_server();

    let create_response = server
        .post("/api/paste")
        .json(&json!({
            "content": "fn main() { println!(\"hello\"); }",
            "name": "manual-language",
            "language": "python",
            "language_is_manual": true
        }))
        .await;

    assert_eq!(create_response.status_code(), StatusCode::OK);
    let paste: serde_json::Value = create_response.json();
    assert_eq!(paste["language"], "python");
    assert_eq!(paste["language_is_manual"], true);
}
