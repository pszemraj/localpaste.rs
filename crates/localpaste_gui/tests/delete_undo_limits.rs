//! Focused GUI backend tests for durable delete undo staging.

use crossbeam_channel::Receiver;
use localpaste_core::{
    db::tables::PASTE_VERSIONS_CONTENT,
    models::paste::{Paste, UpdatePasteRequest},
    Database,
};
use localpaste_gui::backend::{spawn_backend, CoreCmd, CoreEvent};
use std::time::Duration;
use tempfile::TempDir;

const TEST_MAX_PASTE_SIZE: usize = 10 * 1024 * 1024;

fn recv_event(rx: &Receiver<CoreEvent>) -> CoreEvent {
    rx.recv_timeout(Duration::from_secs(2))
        .expect("expected backend event")
}

#[test]
fn backend_delete_returns_undo_when_version_history_payload_exceeds_old_cap() {
    let dir = TempDir::new().expect("temp dir");
    let db_path = dir.path().join("db");
    let db = Database::new(db_path.to_str().expect("db path")).expect("db");
    let seed = Paste::new("x".repeat(17 * 1024 * 1024), "undo-too-large".to_string());
    let paste_id = seed.id.clone();
    db.pastes.create(&seed).expect("create seed paste");
    db.pastes
        .update(
            &paste_id,
            UpdatePasteRequest {
                content: Some("small-current-head".to_string()),
                name: None,
                language: None,
                language_is_manual: None,
                folder_id: None,
                tags: None,
            },
        )
        .expect("update seed paste")
        .expect("paste should exist for update");

    let backend = spawn_backend(db.share().expect("share db"), TEST_MAX_PASTE_SIZE);
    backend
        .cmd_tx
        .send(CoreCmd::DeletePaste {
            id: paste_id.clone(),
        })
        .expect("send delete");
    match recv_event(&backend.evt_rx) {
        CoreEvent::PasteDeleted { id, undo_token } => {
            assert_eq!(id, paste_id);
            let undo_token = undo_token.expect("large history delete should remain undoable");
            assert!(db
                .pastes
                .get(&paste_id)
                .expect("get after delete")
                .is_none());
            backend
                .cmd_tx
                .send(CoreCmd::RestoreDeletedPaste { undo_token })
                .expect("send restore");
        }
        other => panic!("expected PasteDeleted event, got {:?}", other),
    }
    match recv_event(&backend.evt_rx) {
        CoreEvent::PasteRestored { paste, .. } => {
            assert_eq!(paste.id, paste_id);
            assert_eq!(paste.content, "small-current-head");
        }
        other => panic!("expected PasteRestored event, got {:?}", other),
    }
    let versions = db
        .pastes
        .list_versions(&paste_id, Some(10))
        .expect("list restored versions")
        .expect("paste exists");
    assert_eq!(versions.len(), 1);
    let restored_version = db
        .pastes
        .get_version(&paste_id, versions[0].version_id_ms)
        .expect("get restored version")
        .expect("version exists");
    assert_eq!(restored_version.content.len(), 17 * 1024 * 1024);
}

#[test]
fn backend_delete_fails_without_removing_paste_when_version_history_content_is_missing() {
    let dir = TempDir::new().expect("temp dir");
    let db_path = dir.path().join("db");
    let db = Database::new(db_path.to_str().expect("db path")).expect("db");
    let seed = Paste::new("historical content".to_string(), "undo-missing".to_string());
    let paste_id = seed.id.clone();
    db.pastes.create(&seed).expect("create seed paste");
    db.pastes
        .update(
            &paste_id,
            UpdatePasteRequest {
                content: Some("current head".to_string()),
                name: None,
                language: None,
                language_is_manual: None,
                folder_id: None,
                tags: None,
            },
        )
        .expect("update seed paste")
        .expect("paste should exist for update");

    let version_id = db
        .pastes
        .list_versions(&paste_id, Some(1))
        .expect("list versions")
        .expect("paste exists")[0]
        .version_id_ms;
    let write_txn = db.db.begin_write().expect("begin write");
    {
        let mut versions_content = write_txn
            .open_table(PASTE_VERSIONS_CONTENT)
            .expect("open versions content");
        let removed = versions_content
            .remove((paste_id.as_str(), version_id))
            .expect("remove version content");
        assert!(removed.is_some());
    }
    write_txn.commit().expect("commit missing content row");

    let backend = spawn_backend(db.share().expect("share db"), TEST_MAX_PASTE_SIZE);
    backend
        .cmd_tx
        .send(CoreCmd::DeletePaste {
            id: paste_id.clone(),
        })
        .expect("send delete");
    match recv_event(&backend.evt_rx) {
        CoreEvent::Error { source, message } => {
            assert_eq!(source, localpaste_gui::backend::CoreErrorSource::Other);
            assert!(
                message.contains("Missing version content"),
                "unexpected delete error: {}",
                message
            );
        }
        other => panic!("expected delete error event, got {:?}", other),
    }
    assert!(db
        .pastes
        .get(&paste_id)
        .expect("get after delete")
        .is_some());
}
