//! Backend worker wiring for the native rewrite.
//!
//! This module exposes the command/event protocol plus the worker spawn helper
//! used by the egui UI thread.

mod protocol;
mod worker;

pub use protocol::{CoreCmd, CoreErrorSource, CoreEvent, PasteSummary};
pub use worker::{spawn_backend, BackendHandle};

#[cfg(test)]
mod tests {
    use super::*;
    use localpaste_core::models::folder::Folder;
    use localpaste_core::models::paste::Paste;
    use localpaste_core::Database;
    use std::time::Duration;
    use tempfile::TempDir;

    struct TestDb {
        _dir: TempDir,
        db: Database,
    }

    fn setup_db() -> TestDb {
        let dir = TempDir::new().expect("temp dir");
        let db_path = dir.path().join("db");
        let db = Database::new(db_path.to_str().expect("db path")).expect("db");
        TestDb { _dir: dir, db }
    }

    fn recv_event(rx: &crossbeam_channel::Receiver<CoreEvent>) -> CoreEvent {
        rx.recv_timeout(Duration::from_secs(2))
            .expect("expected backend event")
    }

    fn expect_error_contains(rx: &crossbeam_channel::Receiver<CoreEvent>, expected_fragment: &str) {
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

    #[test]
    fn backend_lists_pastes() {
        let TestDb { _dir: _guard, db } = setup_db();
        let paste1 = Paste::new("alpha".to_string(), "first".to_string());
        let paste2 = Paste::new("beta".to_string(), "second".to_string());
        db.pastes.create(&paste1).expect("create paste1");
        db.pastes.create(&paste2).expect("create paste2");

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
                let ids: Vec<&str> = items.iter().map(|p| p.id.as_str()).collect();
                assert!(ids.contains(&paste1.id.as_str()));
                assert!(ids.contains(&paste2.id.as_str()));
            }
            other => panic!("unexpected event: {:?}", other),
        }

        drop(backend);
    }

    #[test]
    fn backend_gets_paste_and_reports_missing() {
        let TestDb { _dir: _guard, db } = setup_db();
        let paste = Paste::new("gamma".to_string(), "third".to_string());
        let paste_id = paste.id.clone();
        db.pastes.create(&paste).expect("create paste");

        let backend = spawn_backend(db);
        backend
            .cmd_tx
            .send(CoreCmd::GetPaste {
                id: paste_id.clone(),
            })
            .expect("send get");

        match recv_event(&backend.evt_rx) {
            CoreEvent::PasteLoaded { paste } => {
                assert_eq!(paste.id, paste_id);
                assert_eq!(paste.content, "gamma");
            }
            other => panic!("unexpected event: {:?}", other),
        }

        let missing_id = "missing-id".to_string();
        backend
            .cmd_tx
            .send(CoreCmd::GetPaste {
                id: missing_id.clone(),
            })
            .expect("send missing");

        match recv_event(&backend.evt_rx) {
            CoreEvent::PasteMissing { id } => assert_eq!(id, missing_id),
            other => panic!("unexpected event: {:?}", other),
        }

        drop(backend);
    }

    #[test]
    fn backend_creates_updates_and_deletes_paste() {
        let TestDb { _dir: _guard, db } = setup_db();
        let backend = spawn_backend(db);

        backend
            .cmd_tx
            .send(CoreCmd::CreatePaste {
                content: "hello".to_string(),
            })
            .expect("send create");

        let created_id = match recv_event(&backend.evt_rx) {
            CoreEvent::PasteCreated { paste } => {
                assert_eq!(paste.content, "hello");
                paste.id
            }
            other => panic!("unexpected event: {:?}", other),
        };

        backend
            .cmd_tx
            .send(CoreCmd::UpdatePaste {
                id: created_id.clone(),
                content: "updated".to_string(),
            })
            .expect("send update");

        match recv_event(&backend.evt_rx) {
            CoreEvent::PasteSaved { paste } => {
                assert_eq!(paste.id, created_id);
                assert_eq!(paste.content, "updated");
            }
            other => panic!("unexpected event: {:?}", other),
        }

        backend
            .cmd_tx
            .send(CoreCmd::DeletePaste {
                id: created_id.clone(),
            })
            .expect("send delete");

        match recv_event(&backend.evt_rx) {
            CoreEvent::PasteDeleted { id } => assert_eq!(id, created_id),
            other => panic!("unexpected event: {:?}", other),
        }
    }

    #[test]
    fn backend_delete_paste_updates_folder_count() {
        let TestDb { _dir: _guard, db } = setup_db();
        let backend = spawn_backend(db);

        backend
            .cmd_tx
            .send(CoreCmd::CreateFolder {
                name: "Scripts".to_string(),
                parent_id: None,
            })
            .expect("send create folder");
        let folder_id = match recv_event(&backend.evt_rx) {
            CoreEvent::FolderSaved { folder } => folder.id,
            other => panic!("unexpected event: {:?}", other),
        };

        backend
            .cmd_tx
            .send(CoreCmd::CreatePaste {
                content: "print('hello')".to_string(),
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
                folder_id: Some(folder_id.clone()),
                tags: None,
            })
            .expect("send assign folder");
        match recv_event(&backend.evt_rx) {
            CoreEvent::PasteMetaSaved { paste } => {
                assert_eq!(paste.folder_id.as_deref(), Some(folder_id.as_str()));
            }
            other => panic!("unexpected event: {:?}", other),
        }

        backend
            .cmd_tx
            .send(CoreCmd::ListFolders)
            .expect("send list folders before delete");
        match recv_event(&backend.evt_rx) {
            CoreEvent::FoldersLoaded { items } => {
                let folder = items
                    .iter()
                    .find(|folder| folder.id == folder_id)
                    .expect("folder should exist");
                assert_eq!(folder.paste_count, 1);
            }
            other => panic!("unexpected event: {:?}", other),
        }

        backend
            .cmd_tx
            .send(CoreCmd::DeletePaste {
                id: paste_id.clone(),
            })
            .expect("send delete paste");
        match recv_event(&backend.evt_rx) {
            CoreEvent::PasteDeleted { id } => assert_eq!(id, paste_id),
            other => panic!("unexpected event: {:?}", other),
        }

        backend
            .cmd_tx
            .send(CoreCmd::ListFolders)
            .expect("send list folders after delete");
        match recv_event(&backend.evt_rx) {
            CoreEvent::FoldersLoaded { items } => {
                let folder = items
                    .iter()
                    .find(|folder| folder.id == folder_id)
                    .expect("folder should exist");
                assert_eq!(folder.paste_count, 0);
            }
            other => panic!("unexpected event: {:?}", other),
        }
    }

    #[test]
    fn backend_searches_metadata_and_lists_folders() {
        let TestDb { _dir: _guard, db } = setup_db();
        let root = localpaste_core::models::folder::Folder::new("Root".to_string());
        db.folders.create(&root).expect("create folder");

        // GUI backend search is metadata-only (name/tags/language), not full content.
        let paste = Paste::new("alpha beta".to_string(), "rust-alpha".to_string());
        db.pastes.create(&paste).expect("create paste");

        let backend = spawn_backend(db);
        backend
            .cmd_tx
            .send(CoreCmd::SearchPastes {
                query: "rust".to_string(),
                limit: 10,
                folder_id: None,
                language: None,
            })
            .expect("send search");

        match recv_event(&backend.evt_rx) {
            CoreEvent::SearchResults { query, items } => {
                assert_eq!(query, "rust");
                assert_eq!(items.len(), 1);
            }
            other => panic!("unexpected event: {:?}", other),
        }

        backend
            .cmd_tx
            .send(CoreCmd::ListFolders)
            .expect("send list folders");

        match recv_event(&backend.evt_rx) {
            CoreEvent::FoldersLoaded { items } => {
                assert_eq!(items.len(), 1);
                assert_eq!(items[0].name, "Root");
            }
            other => panic!("unexpected event: {:?}", other),
        }
    }

    #[test]
    fn backend_updates_paste_metadata() {
        let TestDb { _dir: _guard, db } = setup_db();
        let backend = spawn_backend(db);

        backend
            .cmd_tx
            .send(CoreCmd::CreateFolder {
                name: "Scripts".to_string(),
                parent_id: None,
            })
            .expect("send create folder");

        let folder_id = match recv_event(&backend.evt_rx) {
            CoreEvent::FolderSaved { folder } => folder.id,
            other => panic!("unexpected event: {:?}", other),
        };

        backend
            .cmd_tx
            .send(CoreCmd::CreatePaste {
                content: "print('hi')".to_string(),
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
                name: Some("Script One".to_string()),
                language: Some("python".to_string()),
                language_is_manual: Some(true),
                folder_id: Some(folder_id.clone()),
                tags: Some(vec!["tooling".to_string(), "python".to_string()]),
            })
            .expect("send metadata update");

        match recv_event(&backend.evt_rx) {
            CoreEvent::PasteMetaSaved { paste } => {
                assert_eq!(paste.id, paste_id);
                assert_eq!(paste.name, "Script One");
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
            .send(CoreCmd::ListFolders)
            .expect("send folder list");
        match recv_event(&backend.evt_rx) {
            CoreEvent::FoldersLoaded { items } => {
                let folder = items
                    .iter()
                    .find(|folder| folder.id == folder_id)
                    .expect("folder should exist");
                assert_eq!(folder.paste_count, 1);
            }
            other => panic!("unexpected event: {:?}", other),
        }

        backend
            .cmd_tx
            .send(CoreCmd::GetPaste {
                id: paste_id.clone(),
            })
            .expect("send get paste");
        match recv_event(&backend.evt_rx) {
            CoreEvent::PasteLoaded { paste } => {
                assert_eq!(paste.id, paste_id);
                assert_eq!(paste.language.as_deref(), Some("python"));
                assert!(paste.language_is_manual);
                assert_eq!(paste.folder_id.as_deref(), Some(folder_id.as_str()));
            }
            other => panic!("unexpected event: {:?}", other),
        }

        backend
            .cmd_tx
            .send(CoreCmd::UpdatePasteMeta {
                id: paste_id.clone(),
                name: None,
                language: None,
                language_is_manual: Some(false),
                folder_id: Some(String::new()),
                tags: None,
            })
            .expect("send metadata clear-folder update");

        match recv_event(&backend.evt_rx) {
            CoreEvent::PasteMetaSaved { paste } => {
                assert_eq!(paste.id, paste_id);
                assert!(paste.folder_id.is_none());
                assert!(!paste.language_is_manual);
                assert!(paste.language.is_none());
            }
            other => panic!("unexpected event: {:?}", other),
        }

        backend
            .cmd_tx
            .send(CoreCmd::ListFolders)
            .expect("send folder list after unfile");
        match recv_event(&backend.evt_rx) {
            CoreEvent::FoldersLoaded { items } => {
                let folder = items
                    .iter()
                    .find(|folder| folder.id == folder_id)
                    .expect("folder should exist");
                assert_eq!(folder.paste_count, 0);
            }
            other => panic!("unexpected event: {:?}", other),
        }

        backend
            .cmd_tx
            .send(CoreCmd::UpdatePasteMeta {
                id: paste_id,
                name: None,
                language: None,
                language_is_manual: None,
                folder_id: Some("missing-folder".to_string()),
                tags: None,
            })
            .expect("send metadata missing-folder update");
        expect_error_contains(&backend.evt_rx, "does not exist");
    }

    #[test]
    fn backend_folder_commands_enforce_parenting_rules_and_migrate_on_delete() {
        let TestDb { _dir: _guard, db } = setup_db();
        let backend = spawn_backend(db);

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
    fn backend_rejects_missing_folder_parent_on_update() {
        let TestDb { _dir: _guard, db } = setup_db();
        let root = Folder::new("root".to_string());
        let root_id = root.id.clone();
        db.folders.create(&root).expect("create folder");
        let backend = spawn_backend(db);

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
        let backend = spawn_backend(db);

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
}
