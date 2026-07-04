//! Folder and race headless integration tests for GUI/backend workflows.

mod headless_support;

use headless_support::{recv_event, TestEnv};
use localpaste_core::{
    db::TransactionOps,
    models::{folder::Folder, paste::Paste},
};
use localpaste_gui::backend::{CoreCmd, CoreErrorSource, CoreEvent};
use localpaste_server::PasteLockManager;
use ropey::Rope;
use serde_json::json;
use std::sync::{Arc, Barrier};
use std::thread;
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
            protected_version_id_ms: None,
        })
        .expect("send virtual update");

    match recv_event(&backend.evt_rx) {
        CoreEvent::PasteSaved { .. } | CoreEvent::PasteMissing { .. } => {}
        CoreEvent::Error { source, message } => {
            assert_eq!(
                source,
                CoreErrorSource::SaveContent,
                "race lock rejection should report SaveContent source"
            );
            assert!(
                message.contains("open for editing"),
                "race lock rejection should explain lock conflict, got: {}",
                message
            );
        }
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
