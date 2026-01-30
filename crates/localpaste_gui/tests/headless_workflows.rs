//! Headless integration tests for GUI/backend workflows against the embedded API.

use crossbeam_channel::Receiver;
use localpaste_core::{models::paste::Paste, Config, Database};
use localpaste_gui::backend::{spawn_backend, CoreCmd, CoreEvent};
use localpaste_server::{AppState, EmbeddedServer, PasteLockManager};
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;

fn recv_event(rx: &Receiver<CoreEvent>) -> CoreEvent {
    rx.recv_timeout(Duration::from_secs(2))
        .expect("expected backend event")
}

fn test_config(db_path: &str) -> Config {
    Config {
        db_path: db_path.to_string(),
        port: 0,
        max_paste_size: 10 * 1024 * 1024,
        auto_save_interval: 2000,
        auto_backup: false,
    }
}

#[test]
fn api_updates_are_visible_to_backend_list() {
    let dir = TempDir::new().expect("temp dir");
    let db_path = dir.path().join("db");
    let db_path_str = db_path.to_string_lossy().to_string();
    let db = Database::new(&db_path_str).expect("db");
    let locks = Arc::new(PasteLockManager::default());
    let server_db = db.share().expect("share db");
    let state = AppState::with_locks(test_config(&db_path_str), server_db, locks);
    let server = EmbeddedServer::start(state, false).expect("server");

    let client = reqwest::blocking::Client::new();
    let url = format!("http://{}/api/paste", server.addr());
    let created: Paste = client
        .post(&url)
        .json(&serde_json::json!({ "content": "hello from api" }))
        .send()
        .expect("create request")
        .json()
        .expect("parse response");

    let backend = spawn_backend(db);
    backend
        .cmd_tx
        .send(CoreCmd::ListAll { limit: 10 })
        .expect("send list");

    match recv_event(&backend.evt_rx) {
        CoreEvent::PasteList { items } => {
            assert!(items.iter().any(|item| item.id == created.id));
        }
        other => panic!("unexpected event: {:?}", other),
    }

    drop(server);
}

#[test]
fn locked_paste_blocks_api_delete() {
    let dir = TempDir::new().expect("temp dir");
    let db_path = dir.path().join("db");
    let db_path_str = db_path.to_string_lossy().to_string();
    let db = Database::new(&db_path_str).expect("db");
    let locks = Arc::new(PasteLockManager::default());
    let server_db = db.share().expect("share db");
    let state = AppState::with_locks(test_config(&db_path_str), server_db, locks.clone());
    let server = EmbeddedServer::start(state, false).expect("server");

    let paste = Paste::new("locked content".to_string(), "locked".to_string());
    let paste_id = paste.id.clone();
    db.pastes.create(&paste).expect("create paste");

    locks.lock(&paste_id);

    let client = reqwest::blocking::Client::new();
    let url = format!("http://{}/api/paste/{}", server.addr(), paste_id);
    let resp = client.delete(&url).send().expect("delete request");
    assert_eq!(resp.status(), reqwest::StatusCode::LOCKED);

    locks.unlock(&paste_id);

    let resp = client.delete(&url).send().expect("delete request");
    assert!(resp.status().is_success());
}
