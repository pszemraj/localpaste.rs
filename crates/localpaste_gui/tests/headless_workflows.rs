//! Headless integration tests for GUI/backend workflows against the embedded API.

use crossbeam_channel::Receiver;
use localpaste_core::{
    db::TransactionOps, models::folder::Folder, models::paste::Paste, Config, Database,
};
use localpaste_gui::backend::{
    spawn_backend, spawn_backend_with_locks, BackendHandle, CoreCmd, CoreEvent,
};
use localpaste_server::{AppState, EmbeddedServer, LockOwnerId, PasteLockManager};
use ropey::Rope;
use serde_json::json;
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::TempDir;

const TEST_MAX_PASTE_SIZE: usize = 10 * 1024 * 1024;

fn recv_event(rx: &Receiver<CoreEvent>) -> CoreEvent {
    rx.recv_timeout(Duration::from_secs(2))
        .expect("expected backend event")
}

fn test_config(db_path: &str) -> Config {
    Config {
        db_path: db_path.to_string(),
        port: 0,
        max_paste_size: TEST_MAX_PASTE_SIZE,
        auto_save_interval: 2000,
        auto_backup: false,
    }
}

struct TestEnv {
    _dir: TempDir,
    db_path: String,
    db: Database,
}

impl TestEnv {
    fn new() -> Self {
        let dir = TempDir::new().expect("temp dir");
        let db_path = dir.path().join("db");
        let db_path_str = db_path.to_string_lossy().to_string();
        let db = Database::new(&db_path_str).expect("db");
        Self {
            _dir: dir,
            db_path: db_path_str,
            db,
        }
    }

    fn start_server(&self, locks: Arc<PasteLockManager>) -> EmbeddedServer {
        let state = AppState::with_locks(
            test_config(&self.db_path),
            self.db.share().expect("share db"),
            locks,
        );
        EmbeddedServer::start(state, false).expect("server")
    }

    fn spawn_backend(&self) -> BackendHandle {
        spawn_backend(self.db.share().expect("share db"), TEST_MAX_PASTE_SIZE)
    }

    fn spawn_backend_with_locks(&self, locks: Arc<PasteLockManager>) -> BackendHandle {
        spawn_backend_with_locks(
            self.db.share().expect("share db"),
            TEST_MAX_PASTE_SIZE,
            locks,
        )
    }
}

#[test]
fn api_updates_are_visible_to_backend_list() {
    let env = TestEnv::new();
    let locks = Arc::new(PasteLockManager::default());
    let server = env.start_server(locks);

    let client = reqwest::blocking::Client::new();
    let url = format!("http://{}/api/paste", server.addr());
    let created: Paste = client
        .post(&url)
        .json(&serde_json::json!({ "content": "hello from api" }))
        .send()
        .expect("create request")
        .json()
        .expect("parse response");

    let backend = env.spawn_backend();
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
fn backend_shutdown_drains_queued_update_and_persists_across_reopen() {
    let dir = TempDir::new().expect("temp dir");
    let db_path = dir.path().join("db");
    let db_path_str = db_path.to_string_lossy().to_string();
    let db = Database::new(&db_path_str).expect("db");

    let paste = Paste::new("before-close".to_string(), "shutdown-seed".to_string());
    let paste_id = paste.id.clone();
    db.pastes.create(&paste).expect("create seed paste");

    let mut backend = spawn_backend(db.share().expect("share db"), TEST_MAX_PASTE_SIZE);
    backend
        .cmd_tx
        .send(CoreCmd::UpdatePaste {
            id: paste_id.clone(),
            content: "after-close".to_string(),
        })
        .expect("send update before shutdown");

    backend
        .shutdown_and_join(true, Duration::from_secs(5))
        .expect("shutdown backend");
    drop(backend);
    drop(db);

    let reopened = Database::new(&db_path_str).expect("reopen db");
    let persisted = reopened
        .pastes
        .get(&paste_id)
        .expect("read persisted paste")
        .expect("paste should exist");
    assert_eq!(persisted.content, "after-close");
}

#[test]
fn locked_paste_blocks_api_mutations_until_lock_is_released() {
    enum MutationKind {
        Delete,
        Update,
    }

    let env = TestEnv::new();
    let locks = Arc::new(PasteLockManager::default());
    let server = env.start_server(locks.clone());
    let client = reqwest::blocking::Client::new();

    let cases = [
        (MutationKind::Delete, "delete-owner"),
        (MutationKind::Update, "update-owner"),
    ];

    for (mutation, owner_id) in cases {
        let paste = Paste::new("locked content".to_string(), "locked".to_string());
        let paste_id = paste.id.clone();
        env.db.pastes.create(&paste).expect("create paste");
        let owner = LockOwnerId::new(owner_id.to_string());
        locks
            .acquire(&paste_id, &owner)
            .expect("acquire lock for mutation test");

        let url = format!("http://{}/api/paste/{}", server.addr(), paste_id);
        let blocked = match mutation {
            MutationKind::Delete => client.delete(&url).send().expect("delete request"),
            MutationKind::Update => client
                .put(&url)
                .json(&json!({ "content": "updated" }))
                .send()
                .expect("update request"),
        };
        assert_eq!(blocked.status(), reqwest::StatusCode::LOCKED);

        locks
            .release(&paste_id, &owner)
            .expect("release lock for mutation test");

        let allowed = match mutation {
            MutationKind::Delete => client.delete(&url).send().expect("delete request"),
            MutationKind::Update => client
                .put(&url)
                .json(&json!({ "content": "updated" }))
                .send()
                .expect("update request"),
        };
        assert!(allowed.status().is_success());
    }
}

#[test]
fn backend_delete_rejects_foreign_lock_holder_and_preserves_paste() {
    let env = TestEnv::new();
    let locks = Arc::new(PasteLockManager::default());
    let backend = env.spawn_backend_with_locks(locks.clone());

    backend
        .cmd_tx
        .send(CoreCmd::CreatePaste {
            content: "locked body".to_string(),
        })
        .expect("create paste");
    let paste_id = match recv_event(&backend.evt_rx) {
        CoreEvent::PasteCreated { paste } => paste.id,
        other => panic!("unexpected event: {:?}", other),
    };

    let foreign_owner = LockOwnerId::new("foreign-owner".to_string());
    locks
        .acquire(&paste_id, &foreign_owner)
        .expect("acquire foreign lock holder");

    backend
        .cmd_tx
        .send(CoreCmd::DeletePaste {
            id: paste_id.clone(),
        })
        .expect("send delete");
    match recv_event(&backend.evt_rx) {
        CoreEvent::Error { message, .. } => {
            assert!(
                message.contains("open for editing"),
                "expected lock rejection, got: {}",
                message
            );
        }
        other => panic!("unexpected event: {:?}", other),
    }

    backend
        .cmd_tx
        .send(CoreCmd::GetPaste {
            id: paste_id.clone(),
        })
        .expect("send get after rejected delete");
    match recv_event(&backend.evt_rx) {
        CoreEvent::PasteLoaded { paste } => assert_eq!(paste.id, paste_id),
        other => panic!("unexpected event: {:?}", other),
    }

    locks
        .release(&paste_id, &foreign_owner)
        .expect("release foreign lock holder");
}

#[test]
fn backend_update_paths_reject_foreign_lock_holder_and_preserve_paste() {
    let env = TestEnv::new();
    let locks = Arc::new(PasteLockManager::default());
    let backend = env.spawn_backend_with_locks(locks.clone());

    backend
        .cmd_tx
        .send(CoreCmd::CreatePaste {
            content: "locked body".to_string(),
        })
        .expect("create paste");
    let baseline = match recv_event(&backend.evt_rx) {
        CoreEvent::PasteCreated { paste } => paste,
        other => panic!("unexpected event: {:?}", other),
    };
    let paste_id = baseline.id.clone();

    let foreign_owner = LockOwnerId::new("foreign-owner".to_string());
    locks
        .acquire(&paste_id, &foreign_owner)
        .expect("acquire foreign lock holder");

    backend
        .cmd_tx
        .send(CoreCmd::UpdatePaste {
            id: paste_id.clone(),
            content: "mutated-body".to_string(),
        })
        .expect("send content update");
    match recv_event(&backend.evt_rx) {
        CoreEvent::Error { message, .. } => {
            assert!(
                message.contains("open for editing"),
                "expected lock rejection, got: {}",
                message
            );
        }
        other => panic!("unexpected event: {:?}", other),
    }

    backend
        .cmd_tx
        .send(CoreCmd::UpdatePasteMeta {
            id: paste_id.clone(),
            name: Some("mutated-name".to_string()),
            language: None,
            language_is_manual: None,
            folder_id: None,
            tags: None,
        })
        .expect("send metadata update");
    match recv_event(&backend.evt_rx) {
        CoreEvent::Error { message, .. } => {
            assert!(
                message.contains("open for editing"),
                "expected lock rejection, got: {}",
                message
            );
        }
        other => panic!("unexpected event: {:?}", other),
    }

    backend
        .cmd_tx
        .send(CoreCmd::GetPaste {
            id: paste_id.clone(),
        })
        .expect("send get after rejected updates");
    match recv_event(&backend.evt_rx) {
        CoreEvent::PasteLoaded { paste } => {
            assert_eq!(paste.id, paste_id);
            assert_eq!(paste.content, baseline.content);
            assert_eq!(paste.name, baseline.name);
        }
        other => panic!("unexpected event: {:?}", other),
    }

    locks
        .release(&paste_id, &foreign_owner)
        .expect("release foreign lock holder");
}

#[test]
fn locked_descendant_blocks_backend_folder_delete() {
    let env = TestEnv::new();
    let locks = Arc::new(PasteLockManager::default());
    let backend = env.spawn_backend_with_locks(locks.clone());

    backend
        .cmd_tx
        .send(CoreCmd::CreateFolder {
            name: "locked-root".to_string(),
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
            content: "locked body".to_string(),
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
            name: None,
            language: None,
            language_is_manual: None,
            folder_id: Some(folder_id.clone()),
            tags: None,
        })
        .expect("assign folder");
    match recv_event(&backend.evt_rx) {
        CoreEvent::PasteMetaSaved { paste } => {
            assert_eq!(paste.folder_id.as_deref(), Some(folder_id.as_str()));
        }
        other => panic!("unexpected event: {:?}", other),
    }

    let owner = LockOwnerId::new("folder-owner".to_string());
    locks
        .acquire(&paste_id, &owner)
        .expect("acquire lock for folder delete test");
    backend
        .cmd_tx
        .send(CoreCmd::DeleteFolder {
            id: folder_id.clone(),
        })
        .expect("delete folder");
    match recv_event(&backend.evt_rx) {
        CoreEvent::Error { message, .. } => {
            assert!(
                message.contains("locked paste"),
                "expected lock rejection, got: {}",
                message
            );
        }
        other => panic!("unexpected event: {:?}", other),
    }

    backend
        .cmd_tx
        .send(CoreCmd::GetPaste {
            id: paste_id.clone(),
        })
        .expect("get locked paste");
    match recv_event(&backend.evt_rx) {
        CoreEvent::PasteLoaded { paste } => {
            assert_eq!(paste.id, paste_id);
            assert_eq!(paste.folder_id.as_deref(), Some(folder_id.as_str()));
        }
        other => panic!("unexpected event: {:?}", other),
    }
}

#[test]
fn metadata_update_persists_and_manual_auto_language_transitions_work() {
    let env = TestEnv::new();
    let locks = Arc::new(PasteLockManager::default());
    let _server = env.start_server(locks);
    let backend = env.spawn_backend();

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
fn backend_virtual_update_and_api_delete_race_keeps_consistent_visibility() {
    let env = TestEnv::new();
    let locks = Arc::new(PasteLockManager::default());
    let server = env.start_server(locks.clone());
    let backend = env.spawn_backend_with_locks(locks);

    backend
        .cmd_tx
        .send(CoreCmd::CreatePaste {
            content: "race-seed".to_string(),
        })
        .expect("create seed");
    let paste_id = match recv_event(&backend.evt_rx) {
        CoreEvent::PasteCreated { paste } => paste.id,
        other => panic!("unexpected event: {:?}", other),
    };

    let delete_barrier = Arc::new(Barrier::new(2));
    let delete_barrier_thread = delete_barrier.clone();
    let delete_url = format!("http://{}/api/paste/{}", server.addr(), paste_id);
    let delete_thread = thread::spawn(move || {
        let client = reqwest::blocking::Client::new();
        delete_barrier_thread.wait();
        client
            .delete(delete_url.as_str())
            .send()
            .expect("delete request")
            .status()
    });

    delete_barrier.wait();
    backend
        .cmd_tx
        .send(CoreCmd::UpdatePasteVirtual {
            id: paste_id.clone(),
            content: Rope::from_str("race-virtual-update"),
        })
        .expect("send virtual update");

    match recv_event(&backend.evt_rx) {
        CoreEvent::PasteSaved { .. } | CoreEvent::PasteMissing { .. } => {}
        other => panic!("unexpected backend race result: {:?}", other),
    }

    let delete_status = delete_thread.join().expect("delete join");
    assert!(
        delete_status.is_success() || delete_status == reqwest::StatusCode::LOCKED,
        "delete request should either complete or be lock-rejected, got {}",
        delete_status
    );

    backend
        .cmd_tx
        .send(CoreCmd::GetPaste {
            id: paste_id.clone(),
        })
        .expect("get after race");
    if delete_status.is_success() {
        match recv_event(&backend.evt_rx) {
            CoreEvent::PasteMissing { id } => assert_eq!(id, paste_id),
            other => panic!("unexpected post-race get result: {:?}", other),
        }
    } else {
        match recv_event(&backend.evt_rx) {
            CoreEvent::PasteLoaded { paste } => assert_eq!(paste.id, paste_id),
            other => panic!("unexpected post-race get result: {:?}", other),
        }
    }

    backend
        .cmd_tx
        .send(CoreCmd::ListPastes {
            limit: 20,
            folder_id: None,
        })
        .expect("list after race");
    match recv_event(&backend.evt_rx) {
        CoreEvent::PasteList { items } => {
            if delete_status.is_success() {
                assert!(
                    items.iter().all(|item| item.id != paste_id),
                    "deleted paste must not appear in metadata list"
                );
            } else {
                assert!(
                    items.iter().any(|item| item.id == paste_id),
                    "locked delete must preserve paste visibility in metadata list"
                );
            }
        }
        other => panic!("unexpected post-race list result: {:?}", other),
    }
}

#[test]
fn backend_folder_move_and_api_folder_delete_race_preserves_folder_counts() {
    let env = TestEnv::new();
    let locks = Arc::new(PasteLockManager::default());
    let server = env.start_server(locks.clone());
    let backend = env.spawn_backend_with_locks(locks);

    let root = Folder::new("race-root".to_string());
    let root_id = root.id.clone();
    env.db.folders.create(&root).expect("create root");

    let target = Folder::new("race-target".to_string());
    let target_id = target.id.clone();
    env.db.folders.create(&target).expect("create target");

    let mut paste = Paste::new("race-content".to_string(), "race-paste".to_string());
    paste.folder_id = Some(root_id.clone());
    let paste_id = paste.id.clone();
    TransactionOps::create_paste_with_folder(&env.db, &paste, &root_id)
        .expect("seed paste with folder");

    let delete_barrier = Arc::new(Barrier::new(2));
    let delete_barrier_thread = delete_barrier.clone();
    let delete_url = format!("http://{}/api/folder/{}", server.addr(), root_id);
    let delete_thread = thread::spawn(move || {
        let client = reqwest::blocking::Client::new();
        delete_barrier_thread.wait();
        client
            .delete(delete_url.as_str())
            .send()
            .expect("delete folder request")
            .status()
    });

    delete_barrier.wait();
    backend
        .cmd_tx
        .send(CoreCmd::UpdatePasteMeta {
            id: paste_id.clone(),
            name: None,
            language: None,
            language_is_manual: None,
            folder_id: Some(target_id.clone()),
            tags: None,
        })
        .expect("send move metadata");

    match recv_event(&backend.evt_rx) {
        CoreEvent::PasteMetaSaved { .. } | CoreEvent::Error { .. } => {}
        other => panic!("unexpected metadata race event: {:?}", other),
    }

    let delete_status = delete_thread.join().expect("delete join");
    assert!(
        delete_status.is_success(),
        "folder delete should complete successfully, got {}",
        delete_status
    );

    let root_after = env.db.folders.get(&root_id).expect("root lookup");
    assert!(
        root_after.is_none(),
        "source folder should be deleted after race"
    );

    let paste_after = env
        .db
        .pastes
        .get(&paste_id)
        .expect("paste lookup")
        .expect("paste should remain visible");
    assert_ne!(
        paste_after.folder_id.as_deref(),
        Some(root_id.as_str()),
        "paste must not remain in deleted folder"
    );

    let target_after = env
        .db
        .folders
        .get(&target_id)
        .expect("target lookup")
        .expect("target folder exists");
    let target_list_len = env
        .db
        .pastes
        .list(10, Some(target_id.clone()))
        .expect("target list")
        .len();
    assert_eq!(
        target_after.paste_count, target_list_len,
        "folder count must match canonical ownership after race"
    );
}

#[test]
fn api_folder_changes_are_visible_to_backend_state() {
    let env = TestEnv::new();
    let locks = Arc::new(PasteLockManager::default());
    let server = env.start_server(locks);
    let backend = env.spawn_backend();
    let client = reqwest::blocking::Client::new();

    let folder_url = format!("http://{}/api/folder", server.addr());
    let created_folder: serde_json::Value = client
        .post(&folder_url)
        .json(&json!({ "name": "API Folder" }))
        .send()
        .expect("create folder request")
        .json()
        .expect("parse folder response");
    let folder_id = created_folder["id"]
        .as_str()
        .expect("folder id")
        .to_string();

    let paste_url = format!("http://{}/api/paste", server.addr());
    let created_paste: Paste = client
        .post(&paste_url)
        .json(&json!({
            "content": "api-managed",
            "name": "api-paste",
            "folder_id": folder_id.clone()
        }))
        .send()
        .expect("create paste request")
        .json()
        .expect("parse paste response");

    backend
        .cmd_tx
        .send(CoreCmd::ListPastes {
            limit: 10,
            folder_id: Some(folder_id.clone()),
        })
        .expect("list folder");
    match recv_event(&backend.evt_rx) {
        CoreEvent::PasteList { items } => {
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].id, created_paste.id);
        }
        other => panic!("unexpected event: {:?}", other),
    }

    let delete_url = format!("http://{}/api/folder/{}", server.addr(), folder_id);
    let delete_resp = client
        .delete(&delete_url)
        .send()
        .expect("delete folder request");
    assert!(delete_resp.status().is_success());

    backend
        .cmd_tx
        .send(CoreCmd::GetPaste {
            id: created_paste.id.clone(),
        })
        .expect("get migrated paste");
    match recv_event(&backend.evt_rx) {
        CoreEvent::PasteLoaded { paste } => assert!(paste.folder_id.is_none()),
        other => panic!("unexpected event: {:?}", other),
    }

    backend
        .cmd_tx
        .send(CoreCmd::ListFolders)
        .expect("list folders");
    match recv_event(&backend.evt_rx) {
        CoreEvent::FoldersLoaded { items } => {
            assert!(
                items.iter().all(|folder| folder.id != folder_id),
                "deleted folder should not appear in backend state"
            );
        }
        other => panic!("unexpected event: {:?}", other),
    }
}

#[test]
fn folder_delete_marker_rejects_new_assignments_server_and_gui() {
    let env = TestEnv::new();
    let locks = Arc::new(PasteLockManager::default());
    let server = env.start_server(locks.clone());
    let backend = env.spawn_backend_with_locks(locks);

    let folder = Folder::new("delete-marked".to_string());
    let folder_id = folder.id.clone();
    env.db.folders.create(&folder).expect("create folder");
    env.db
        .folders
        .mark_deleting(std::slice::from_ref(&folder_id))
        .expect("mark deleting");

    let client = reqwest::blocking::Client::new();
    let create_url = format!("http://{}/api/paste", server.addr());
    let api_create = client
        .post(&create_url)
        .json(&json!({
            "content": "api-folder-create",
            "name": "api-folder-create",
            "folder_id": folder_id
        }))
        .send()
        .expect("api create request");
    assert_eq!(api_create.status(), reqwest::StatusCode::BAD_REQUEST);

    backend
        .cmd_tx
        .send(CoreCmd::CreatePaste {
            content: "gui-seed".to_string(),
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
            name: None,
            language: None,
            language_is_manual: None,
            folder_id: Some(folder_id.clone()),
            tags: None,
        })
        .expect("send metadata update");
    match recv_event(&backend.evt_rx) {
        CoreEvent::Error { message, .. } => {
            assert!(
                message.contains("being deleted"),
                "expected delete marker rejection, got: {}",
                message
            );
        }
        other => panic!("unexpected event: {:?}", other),
    }

    backend
        .cmd_tx
        .send(CoreCmd::GetPaste { id: paste_id })
        .expect("get paste");
    match recv_event(&backend.evt_rx) {
        CoreEvent::PasteLoaded { paste } => assert!(paste.folder_id.is_none()),
        other => panic!("unexpected event: {:?}", other),
    }
}

#[test]
fn list_and_search_latency_stay_within_reasonable_headless_budget() {
    let env = TestEnv::new();

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
        env.db.pastes.create(&paste).expect("seed paste");
    }

    let backend = env.spawn_backend();

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
