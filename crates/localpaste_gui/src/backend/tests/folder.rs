//! Folder command backend tests.

use super::*;

#[test]
fn backend_rejects_assignment_into_delete_marked_folder() {
    let TestDb { _dir: _guard, db } = setup_db();
    let folder = Folder::new("delete-marked".to_string());
    let folder_id = folder.id.clone();
    db.folders.create(&folder).expect("create folder");
    db.folders
        .mark_deleting(std::slice::from_ref(&folder_id))
        .expect("mark folder deleting");

    let backend = spawn_backend(db, 10 * 1024 * 1024);
    backend
        .cmd_tx
        .send(CoreCmd::CreatePaste {
            content: "seed".to_string(),
        })
        .expect("send create paste");
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
            folder_id: Some(folder_id),
            tags: None,
        })
        .expect("send metadata update");
    match recv_event(&backend.evt_rx) {
        CoreEvent::Error { source, message } => {
            assert_eq!(source, CoreErrorSource::SaveMetadata);
            assert!(
                message.contains("being deleted"),
                "expected delete-marker rejection message, got: {}",
                message
            );
        }
        other => panic!("unexpected event: {:?}", other),
    }

    backend
        .cmd_tx
        .send(CoreCmd::GetPaste { id: paste_id })
        .expect("send get paste");
    match recv_event(&backend.evt_rx) {
        CoreEvent::PasteLoaded { paste } => {
            assert!(paste.folder_id.is_none(), "paste should remain unfiled");
        }
        other => panic!("unexpected event: {:?}", other),
    }
}

#[test]
fn backend_folder_commands_enforce_parenting_rules_and_migrate_on_delete() {
    let TestDb { _dir: _guard, db } = setup_db();
    let backend = spawn_backend(db, 10 * 1024 * 1024);

    backend
        .cmd_tx
        .send(CoreCmd::CreateFolder {
            name: "root".to_string(),
            parent_id: None,
        })
        .expect("send create root");
    let root = match recv_event(&backend.evt_rx) {
        CoreEvent::FolderSaved { folder } => folder,
        other => panic!("unexpected event: {:?}", other),
    };

    backend
        .cmd_tx
        .send(CoreCmd::CreateFolder {
            name: "child".to_string(),
            parent_id: Some(root.id.clone()),
        })
        .expect("send create child");
    let child = match recv_event(&backend.evt_rx) {
        CoreEvent::FolderSaved { folder } => folder,
        other => panic!("unexpected event: {:?}", other),
    };

    backend
        .cmd_tx
        .send(CoreCmd::UpdateFolder {
            id: root.id.clone(),
            name: "root".to_string(),
            parent_id: Some(child.id.clone()),
        })
        .expect("send cycle update");
    expect_error_contains(&backend.evt_rx, "would create cycle");

    backend
        .cmd_tx
        .send(CoreCmd::CreateFolder {
            name: "orphan".to_string(),
            parent_id: Some("missing-parent".to_string()),
        })
        .expect("send missing-parent create");
    expect_error_contains(&backend.evt_rx, "does not exist");

    backend
        .cmd_tx
        .send(CoreCmd::CreatePaste {
            content: "folder-owned".to_string(),
        })
        .expect("send create paste");
    let paste_id = match recv_event(&backend.evt_rx) {
        CoreEvent::PasteCreated { paste } => paste.id,
        other => panic!("unexpected event: {:?}", other),
    };

    backend
        .cmd_tx
        .send(CoreCmd::UpdatePasteMeta {
            id: paste_id.clone(),
            name: Some("folder-owned".to_string()),
            language: None,
            language_is_manual: Some(false),
            folder_id: Some(child.id.clone()),
            tags: Some(Vec::new()),
        })
        .expect("send move paste to child");
    match recv_event(&backend.evt_rx) {
        CoreEvent::PasteMetaSaved { paste } => {
            assert_eq!(paste.folder_id.as_deref(), Some(child.id.as_str()));
        }
        other => panic!("unexpected event: {:?}", other),
    }

    backend
        .cmd_tx
        .send(CoreCmd::DeleteFolder {
            id: root.id.clone(),
        })
        .expect("send delete root");
    match recv_event(&backend.evt_rx) {
        CoreEvent::FolderDeleted { id } => assert_eq!(id, root.id),
        other => panic!("unexpected event: {:?}", other),
    }

    backend
        .cmd_tx
        .send(CoreCmd::GetPaste {
            id: paste_id.clone(),
        })
        .expect("send get moved paste");
    match recv_event(&backend.evt_rx) {
        CoreEvent::PasteLoaded { paste } => {
            assert_eq!(paste.id, paste_id);
            assert!(paste.folder_id.is_none());
        }
        other => panic!("unexpected event: {:?}", other),
    }

    backend
        .cmd_tx
        .send(CoreCmd::ListFolders)
        .expect("send folders list");
    match recv_event(&backend.evt_rx) {
        CoreEvent::FoldersLoaded { items } => assert!(items.is_empty()),
        other => panic!("unexpected event: {:?}", other),
    }
}

#[test]
fn backend_create_folder_trims_parent_id() {
    let TestDb { _dir: _guard, db } = setup_db();
    let backend = spawn_backend(db, 10 * 1024 * 1024);

    backend
        .cmd_tx
        .send(CoreCmd::CreateFolder {
            name: "root".to_string(),
            parent_id: None,
        })
        .expect("send create root");
    let root = match recv_event(&backend.evt_rx) {
        CoreEvent::FolderSaved { folder } => folder,
        other => panic!("unexpected event: {:?}", other),
    };

    backend
        .cmd_tx
        .send(CoreCmd::CreateFolder {
            name: "child".to_string(),
            parent_id: Some(format!("  {}  ", root.id)),
        })
        .expect("send create child");
    match recv_event(&backend.evt_rx) {
        CoreEvent::FolderSaved { folder } => {
            assert_eq!(folder.name, "child");
            assert_eq!(folder.parent_id.as_deref(), Some(root.id.as_str()));
        }
        other => panic!("unexpected event: {:?}", other),
    }
}

#[test]
fn backend_rejects_missing_folder_parent_on_update() {
    let TestDb { _dir: _guard, db } = setup_db();
    let root = Folder::new("root".to_string());
    let root_id = root.id.clone();
    db.folders.create(&root).expect("create folder");
    let backend = spawn_backend(db, 10 * 1024 * 1024);

    backend
        .cmd_tx
        .send(CoreCmd::UpdateFolder {
            id: root_id,
            name: "root".to_string(),
            parent_id: Some("missing-parent".to_string()),
        })
        .expect("send update");
    expect_error_contains(&backend.evt_rx, "does not exist");
}

#[test]
fn backend_update_folder_preserves_parent_unless_clear_is_explicit() {
    let TestDb { _dir: _guard, db } = setup_db();
    let backend = spawn_backend(db, 10 * 1024 * 1024);

    backend
        .cmd_tx
        .send(CoreCmd::CreateFolder {
            name: "root".to_string(),
            parent_id: None,
        })
        .expect("send create root");
    let root = match recv_event(&backend.evt_rx) {
        CoreEvent::FolderSaved { folder } => folder,
        other => panic!("unexpected event: {:?}", other),
    };

    backend
        .cmd_tx
        .send(CoreCmd::CreateFolder {
            name: "child".to_string(),
            parent_id: Some(root.id.clone()),
        })
        .expect("send create child");
    let child = match recv_event(&backend.evt_rx) {
        CoreEvent::FolderSaved { folder } => folder,
        other => panic!("unexpected event: {:?}", other),
    };

    backend
        .cmd_tx
        .send(CoreCmd::UpdateFolder {
            id: child.id.clone(),
            name: "child-renamed".to_string(),
            parent_id: None,
        })
        .expect("send rename without re-parenting");

    match recv_event(&backend.evt_rx) {
        CoreEvent::FolderSaved { folder } => {
            assert_eq!(folder.id, child.id);
            assert_eq!(folder.name, "child-renamed");
            assert_eq!(folder.parent_id.as_deref(), Some(root.id.as_str()));
        }
        other => panic!("unexpected event: {:?}", other),
    }

    backend
        .cmd_tx
        .send(CoreCmd::UpdateFolder {
            id: child.id.clone(),
            name: "child-renamed".to_string(),
            parent_id: Some(String::new()),
        })
        .expect("send explicit clear parent");

    match recv_event(&backend.evt_rx) {
        CoreEvent::FolderSaved { folder } => {
            assert_eq!(folder.id, child.id);
            assert!(folder.parent_id.is_none());
        }
        other => panic!("unexpected event: {:?}", other),
    }
}
