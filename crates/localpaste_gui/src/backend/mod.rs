//! Backend worker wiring for the native rewrite.
//!
//! This module exposes the command/event protocol plus the worker spawn helper
//! used by the egui UI thread.

mod protocol;
mod worker;

pub use protocol::{CoreCmd, CoreEvent, PasteSummary};
pub use worker::{spawn_backend, BackendHandle};

#[cfg(test)]
mod tests {
    use super::*;
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
    fn backend_searches_and_lists_folders() {
        let TestDb { _dir: _guard, db } = setup_db();
        let root = localpaste_core::models::folder::Folder::new("Root".to_string());
        db.folders.create(&root).expect("create folder");

        let paste = Paste::new("alpha rust beta".to_string(), "alpha".to_string());
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
}
