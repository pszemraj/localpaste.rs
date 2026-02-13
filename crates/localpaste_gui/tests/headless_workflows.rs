//! Headless integration tests for GUI/backend workflows against the embedded API.

use crossbeam_channel::Receiver;
use localpaste_core::{models::paste::Paste, Config, Database};
use localpaste_gui::backend::{spawn_backend, CoreCmd, CoreEvent};
use localpaste_server::{AppState, EmbeddedServer, PasteLockManager};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::TempDir;

fn recv_event(rx: &Receiver<CoreEvent>) -> CoreEvent {
    rx.recv_timeout(Duration::from_secs(2))
        .expect("expected backend event")
}

fn expect_error_contains(rx: &Receiver<CoreEvent>, expected_fragment: &str) {
    match recv_event(rx) {
        CoreEvent::Error { message, .. } => {
            assert!(
                message.contains(expected_fragment),
                "expected error containing '{}', got '{}'",
                expected_fragment,
                message
            );
        }
        other => panic!("expected error event, got {:?}", other),
    }
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
        .send(CoreCmd::ListPastes {
            limit: 10,
            folder_id: None,
        })
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

#[test]
fn metadata_update_persists_and_manual_auto_language_transitions_work() {
    let dir = TempDir::new().expect("temp dir");
    let db_path = dir.path().join("db");
    let db_path_str = db_path.to_string_lossy().to_string();
    let db = Database::new(&db_path_str).expect("db");
    let locks = Arc::new(PasteLockManager::default());
    let server_db = db.share().expect("share db");
    let state = AppState::with_locks(test_config(&db_path_str), server_db, locks);
    let _server = EmbeddedServer::start(state, false).expect("server");
    let backend = spawn_backend(db);

    backend
        .cmd_tx
        .send(CoreCmd::CreateFolder {
            name: "Scripts".to_string(),
            parent_id: None,
        })
        .expect("create folder");
    let folder_id = match recv_event(&backend.evt_rx) {
        CoreEvent::FolderSaved { folder } => folder.id,
        other => panic!("unexpected event: {:?}", other),
    };

    backend
        .cmd_tx
        .send(CoreCmd::CreatePaste {
            content: "print('hello')".to_string(),
        })
        .expect("create paste");
    let paste_id = match recv_event(&backend.evt_rx) {
        CoreEvent::PasteCreated { paste } => paste.id,
        other => panic!("unexpected event: {:?}", other),
    };

    backend
        .cmd_tx
        .send(CoreCmd::UpdatePasteMeta {
            id: paste_id.clone(),
            name: Some("script-one".to_string()),
            language: Some("python".to_string()),
            language_is_manual: Some(true),
            folder_id: Some(folder_id.clone()),
            tags: Some(vec!["tooling".to_string(), "python".to_string()]),
        })
        .expect("update metadata manual");
    match recv_event(&backend.evt_rx) {
        CoreEvent::PasteMetaSaved { paste } => {
            assert_eq!(paste.name, "script-one");
            assert_eq!(paste.language.as_deref(), Some("python"));
            assert!(paste.language_is_manual);
            assert_eq!(paste.folder_id.as_deref(), Some(folder_id.as_str()));
            assert_eq!(
                paste.tags,
                vec!["tooling".to_string(), "python".to_string()]
            );
        }
        other => panic!("unexpected event: {:?}", other),
    }

    backend
        .cmd_tx
        .send(CoreCmd::UpdatePasteMeta {
            id: paste_id.clone(),
            name: Some("script-one".to_string()),
            language: None,
            language_is_manual: Some(false),
            folder_id: Some(folder_id.clone()),
            tags: Some(vec!["tooling".to_string()]),
        })
        .expect("update metadata auto");
    match recv_event(&backend.evt_rx) {
        CoreEvent::PasteMetaSaved { paste } => {
            assert!(!paste.language_is_manual);
            assert!(paste.language.is_none());
            assert_eq!(paste.tags, vec!["tooling".to_string()]);
        }
        other => panic!("unexpected event: {:?}", other),
    }

    backend
        .cmd_tx
        .send(CoreCmd::SearchPastes {
            query: "script".to_string(),
            limit: 10,
            folder_id: Some(folder_id),
            language: None,
        })
        .expect("search");
    match recv_event(&backend.evt_rx) {
        CoreEvent::SearchResults { items, .. } => {
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].id, paste_id);
        }
        other => panic!("unexpected event: {:?}", other),
    }
}

#[test]
fn folder_crud_move_cycle_reject_and_delete_migration_hold_end_to_end() {
    let dir = TempDir::new().expect("temp dir");
    let db_path = dir.path().join("db");
    let db_path_str = db_path.to_string_lossy().to_string();
    let db = Database::new(&db_path_str).expect("db");
    let locks = Arc::new(PasteLockManager::default());
    let server_db = db.share().expect("share db");
    let state = AppState::with_locks(test_config(&db_path_str), server_db, locks);
    let _server = EmbeddedServer::start(state, false).expect("server");
    let backend = spawn_backend(db);

    backend
        .cmd_tx
        .send(CoreCmd::CreateFolder {
            name: "Root".to_string(),
            parent_id: None,
        })
        .expect("create root");
    let root_id = match recv_event(&backend.evt_rx) {
        CoreEvent::FolderSaved { folder } => folder.id,
        other => panic!("unexpected event: {:?}", other),
    };

    backend
        .cmd_tx
        .send(CoreCmd::CreateFolder {
            name: "Child".to_string(),
            parent_id: Some(root_id.clone()),
        })
        .expect("create child");
    let child_id = match recv_event(&backend.evt_rx) {
        CoreEvent::FolderSaved { folder } => folder.id,
        other => panic!("unexpected event: {:?}", other),
    };

    backend
        .cmd_tx
        .send(CoreCmd::CreatePaste {
            content: "child-owned".to_string(),
        })
        .expect("create paste");
    let paste_id = match recv_event(&backend.evt_rx) {
        CoreEvent::PasteCreated { paste } => paste.id,
        other => panic!("unexpected event: {:?}", other),
    };

    backend
        .cmd_tx
        .send(CoreCmd::UpdatePasteMeta {
            id: paste_id.clone(),
            name: Some("child-owned".to_string()),
            language: None,
            language_is_manual: Some(false),
            folder_id: Some(child_id.clone()),
            tags: Some(Vec::new()),
        })
        .expect("move paste");
    match recv_event(&backend.evt_rx) {
        CoreEvent::PasteMetaSaved { paste } => {
            assert_eq!(paste.folder_id.as_deref(), Some(child_id.as_str()));
        }
        other => panic!("unexpected event: {:?}", other),
    }

    backend
        .cmd_tx
        .send(CoreCmd::ListPastes {
            limit: 10,
            folder_id: Some(child_id.clone()),
        })
        .expect("list child");
    match recv_event(&backend.evt_rx) {
        CoreEvent::PasteList { items } => {
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].id, paste_id);
        }
        other => panic!("unexpected event: {:?}", other),
    }

    backend
        .cmd_tx
        .send(CoreCmd::UpdateFolder {
            id: root_id.clone(),
            name: "Root".to_string(),
            parent_id: Some(child_id),
        })
        .expect("cycle update");
    expect_error_contains(&backend.evt_rx, "would create cycle");

    backend
        .cmd_tx
        .send(CoreCmd::DeleteFolder {
            id: root_id.clone(),
        })
        .expect("delete root");
    match recv_event(&backend.evt_rx) {
        CoreEvent::FolderDeleted { id } => assert_eq!(id, root_id),
        other => panic!("unexpected event: {:?}", other),
    }

    backend
        .cmd_tx
        .send(CoreCmd::GetPaste { id: paste_id })
        .expect("get migrated paste");
    match recv_event(&backend.evt_rx) {
        CoreEvent::PasteLoaded { paste } => {
            assert!(paste.folder_id.is_none());
        }
        other => panic!("unexpected event: {:?}", other),
    }

    backend
        .cmd_tx
        .send(CoreCmd::ListFolders)
        .expect("list folders");
    match recv_event(&backend.evt_rx) {
        CoreEvent::FoldersLoaded { items } => assert!(items.is_empty()),
        other => panic!("unexpected event: {:?}", other),
    }
}

#[test]
fn list_and_search_latency_stay_within_reasonable_headless_budget() {
    let dir = TempDir::new().expect("temp dir");
    let db_path = dir.path().join("db");
    let db_path_str = db_path.to_string_lossy().to_string();
    let db = Database::new(&db_path_str).expect("db");

    for idx in 0..1500 {
        let content = if idx % 250 == 0 {
            format!("payload {} needle marker", idx)
        } else {
            format!("payload {} filler", idx)
        };
        let name = if idx % 250 == 0 {
            format!("needle-item-{}", idx)
        } else {
            format!("item-{}", idx)
        };
        let paste = Paste::new(content, name);
        db.pastes.create(&paste).expect("seed paste");
    }

    let backend = spawn_backend(db);

    let list_start = Instant::now();
    backend
        .cmd_tx
        .send(CoreCmd::ListPastes {
            limit: 512,
            folder_id: None,
        })
        .expect("send list");
    let list_elapsed = list_start.elapsed();
    match recv_event(&backend.evt_rx) {
        CoreEvent::PasteList { items } => assert_eq!(items.len(), 512),
        other => panic!("unexpected event: {:?}", other),
    }

    let search_start = Instant::now();
    backend
        .cmd_tx
        .send(CoreCmd::SearchPastes {
            query: "needle".to_string(),
            limit: 32,
            folder_id: None,
            language: None,
        })
        .expect("send search");
    let search_elapsed = search_start.elapsed();
    match recv_event(&backend.evt_rx) {
        CoreEvent::SearchResults { items, .. } => {
            assert!(!items.is_empty());
            assert!(items.len() <= 32);
        }
        other => panic!("unexpected event: {:?}", other),
    }

    assert!(
        list_elapsed < Duration::from_secs(5),
        "list exceeded budget: {:?}",
        list_elapsed
    );
    assert!(
        search_elapsed < Duration::from_secs(5),
        "search exceeded budget: {:?}",
        search_elapsed
    );
}
