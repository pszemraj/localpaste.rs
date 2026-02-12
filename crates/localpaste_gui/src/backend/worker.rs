//! Background worker thread for database access.

use crate::backend::{CoreCmd, CoreEvent, PasteSummary};
use crossbeam_channel::{unbounded, Receiver, Sender};
use localpaste_core::{
    db::TransactionOps,
    folder_ops::{delete_folder_tree_and_migrate, introduces_cycle},
    models::{folder::Folder, paste::UpdatePasteRequest},
    naming, Database,
};
use std::thread;
use tracing::error;

/// Handle for sending commands to, and receiving events from, the backend worker.
pub struct BackendHandle {
    pub cmd_tx: Sender<CoreCmd>,
    pub evt_rx: Receiver<CoreEvent>,
}

/// Spawn the backend worker thread that performs blocking database access.
///
/// All I/O stays off the UI thread; the worker replies with [`CoreEvent`] values
/// that are polled each frame.
///
/// # Returns
/// A [`BackendHandle`] containing the command sender and event receiver.
///
/// # Panics
/// Panics if the worker thread cannot be spawned.
pub fn spawn_backend(db: Database) -> BackendHandle {
    let (cmd_tx, cmd_rx) = unbounded();
    let (evt_tx, evt_rx) = unbounded();

    thread::Builder::new()
        .name("localpaste-gui-backend".to_string())
        .spawn(move || {
            for cmd in cmd_rx.iter() {
                match cmd {
                    CoreCmd::ListPastes { limit, folder_id } => {
                        match db.pastes.list(limit, folder_id) {
                            Ok(pastes) => {
                                let items = pastes.iter().map(PasteSummary::from_paste).collect();
                                let _ = evt_tx.send(CoreEvent::PasteList { items });
                            }
                            Err(err) => {
                                error!("backend list failed: {}", err);
                                let _ = evt_tx.send(CoreEvent::Error {
                                    message: format!("List failed: {}", err),
                                });
                            }
                        }
                    }
                    CoreCmd::SearchPastes {
                        query,
                        limit,
                        folder_id,
                        language,
                    } => match db.pastes.search(&query, limit, folder_id, language) {
                        Ok(pastes) => {
                            let items = pastes.iter().map(PasteSummary::from_paste).collect();
                            let _ = evt_tx.send(CoreEvent::SearchResults { query, items });
                        }
                        Err(err) => {
                            error!("backend search failed: {}", err);
                            let _ = evt_tx.send(CoreEvent::Error {
                                message: format!("Search failed: {}", err),
                            });
                        }
                    },
                    CoreCmd::GetPaste { id } => match db.pastes.get(&id) {
                        Ok(Some(paste)) => {
                            let _ = evt_tx.send(CoreEvent::PasteLoaded { paste });
                        }
                        Ok(None) => {
                            let _ = evt_tx.send(CoreEvent::PasteMissing { id });
                        }
                        Err(err) => {
                            error!("backend get failed: {}", err);
                            let _ = evt_tx.send(CoreEvent::Error {
                                message: format!("Get failed: {}", err),
                            });
                        }
                    },
                    CoreCmd::CreatePaste { content } => {
                        let inferred = localpaste_core::models::paste::detect_language(&content);
                        let name = naming::generate_name_for_content(&content, inferred.as_deref());
                        let paste = localpaste_core::models::paste::Paste::new(content, name);
                        match db.pastes.create(&paste) {
                            Ok(()) => {
                                let _ = evt_tx.send(CoreEvent::PasteCreated { paste });
                            }
                            Err(err) => {
                                error!("backend create failed: {}", err);
                                let _ = evt_tx.send(CoreEvent::Error {
                                    message: format!("Create failed: {}", err),
                                });
                            }
                        }
                    }
                    CoreCmd::UpdatePaste { id, content } => {
                        let update = UpdatePasteRequest {
                            content: Some(content),
                            name: None,
                            language: None,
                            language_is_manual: None,
                            folder_id: None,
                            tags: None,
                        };
                        match db.pastes.update(&id, update) {
                            Ok(Some(paste)) => {
                                let _ = evt_tx.send(CoreEvent::PasteSaved { paste });
                            }
                            Ok(None) => {
                                let _ = evt_tx.send(CoreEvent::PasteMissing { id });
                            }
                            Err(err) => {
                                error!("backend update failed: {}", err);
                                let _ = evt_tx.send(CoreEvent::Error {
                                    message: format!("Update failed: {}", err),
                                });
                            }
                        }
                    }
                    CoreCmd::UpdatePasteMeta {
                        id,
                        name,
                        language,
                        language_is_manual,
                        folder_id,
                        tags,
                    } => {
                        let existing = match db.pastes.get(&id) {
                            Ok(Some(paste)) => paste,
                            Ok(None) => {
                                let _ = evt_tx.send(CoreEvent::PasteMissing { id });
                                continue;
                            }
                            Err(err) => {
                                error!("backend metadata load failed: {}", err);
                                let _ = evt_tx.send(CoreEvent::Error {
                                    message: format!("Metadata update failed: {}", err),
                                });
                                continue;
                            }
                        };

                        let normalized_folder_id = folder_id.map(|fid| {
                            let trimmed = fid.trim().to_string();
                            if trimmed.is_empty() {
                                String::new()
                            } else {
                                trimmed
                            }
                        });

                        if let Some(folder_id) =
                            normalized_folder_id.as_ref().filter(|fid| !fid.is_empty())
                        {
                            match db.folders.get(folder_id) {
                                Ok(Some(_)) => {}
                                Ok(None) => {
                                    let _ = evt_tx.send(CoreEvent::Error {
                                        message: format!(
                                            "Metadata update failed: folder '{}' does not exist",
                                            folder_id
                                        ),
                                    });
                                    continue;
                                }
                                Err(err) => {
                                    error!("backend folder lookup failed: {}", err);
                                    let _ = evt_tx.send(CoreEvent::Error {
                                        message: format!("Metadata update failed: {}", err),
                                    });
                                    continue;
                                }
                            }
                        }

                        let update = UpdatePasteRequest {
                            content: None,
                            name,
                            language,
                            language_is_manual,
                            folder_id: normalized_folder_id.clone(),
                            tags,
                        };

                        let folder_changing = normalized_folder_id.is_some() && {
                            let new_folder = normalized_folder_id.as_ref().and_then(|f| {
                                if f.is_empty() {
                                    None
                                } else {
                                    Some(f.as_str())
                                }
                            });
                            new_folder != existing.folder_id.as_deref()
                        };

                        let result = if folder_changing {
                            let new_folder_id = normalized_folder_id.clone().and_then(|f| {
                                if f.is_empty() {
                                    None
                                } else {
                                    Some(f)
                                }
                            });
                            TransactionOps::move_paste_between_folders(
                                &db,
                                &id,
                                existing.folder_id.as_deref(),
                                new_folder_id.as_deref(),
                                update,
                            )
                        } else {
                            db.pastes.update(&id, update)
                        };

                        match result {
                            Ok(Some(paste)) => {
                                let _ = evt_tx.send(CoreEvent::PasteMetaSaved { paste });
                            }
                            Ok(None) => {
                                let _ = evt_tx.send(CoreEvent::PasteMissing { id });
                            }
                            Err(err) => {
                                error!("backend metadata update failed: {}", err);
                                let _ = evt_tx.send(CoreEvent::Error {
                                    message: format!("Metadata update failed: {}", err),
                                });
                            }
                        }
                    }
                    CoreCmd::DeletePaste { id } => {
                        let _existing = match db.pastes.get(&id) {
                            Ok(Some(paste)) => paste,
                            Ok(None) => {
                                let _ = evt_tx.send(CoreEvent::PasteMissing { id });
                                continue;
                            }
                            Err(err) => {
                                error!("backend delete failed during lookup: {}", err);
                                let _ = evt_tx.send(CoreEvent::Error {
                                    message: format!("Delete failed: {}", err),
                                });
                                continue;
                            }
                        };

                        let deleted = TransactionOps::delete_paste_with_folder(&db, &id);

                        match deleted {
                            Ok(true) => {
                                let _ = evt_tx.send(CoreEvent::PasteDeleted { id });
                            }
                            Ok(false) => {
                                let _ = evt_tx.send(CoreEvent::PasteMissing { id });
                            }
                            Err(err) => {
                                error!("backend delete failed: {}", err);
                                let _ = evt_tx.send(CoreEvent::Error {
                                    message: format!("Delete failed: {}", err),
                                });
                            }
                        }
                    }
                    CoreCmd::ListFolders => match db.folders.list() {
                        Ok(items) => {
                            let _ = evt_tx.send(CoreEvent::FoldersLoaded { items });
                        }
                        Err(err) => {
                            error!("backend list folders failed: {}", err);
                            let _ = evt_tx.send(CoreEvent::Error {
                                message: format!("List folders failed: {}", err),
                            });
                        }
                    },
                    CoreCmd::CreateFolder { name, parent_id } => {
                        let normalized_parent = parent_id.filter(|pid| !pid.trim().is_empty());
                        if let Some(parent_id) = normalized_parent.as_deref() {
                            match db.folders.get(parent_id) {
                                Ok(Some(_)) => {}
                                Ok(None) => {
                                    let _ = evt_tx.send(CoreEvent::Error {
                                        message: format!(
                                            "Create folder failed: parent '{}' does not exist",
                                            parent_id
                                        ),
                                    });
                                    continue;
                                }
                                Err(err) => {
                                    let _ = evt_tx.send(CoreEvent::Error {
                                        message: format!("Create folder failed: {}", err),
                                    });
                                    continue;
                                }
                            }
                        }

                        let folder = Folder::with_parent(name, normalized_parent);
                        match db.folders.create(&folder) {
                            Ok(()) => {
                                let _ = evt_tx.send(CoreEvent::FolderSaved { folder });
                            }
                            Err(err) => {
                                error!("backend create folder failed: {}", err);
                                let _ = evt_tx.send(CoreEvent::Error {
                                    message: format!("Create folder failed: {}", err),
                                });
                            }
                        }
                    }
                    CoreCmd::UpdateFolder {
                        id,
                        name,
                        parent_id,
                    } => {
                        // Preserve API semantics:
                        // - `None` => leave parent unchanged
                        // - `Some("")` => clear parent (top-level)
                        // - `Some("id")` => set explicit parent
                        let parent_update = parent_id.map(|pid| pid.trim().to_string());
                        let normalized_parent =
                            parent_update.as_ref().and_then(|pid| match pid.trim() {
                                "" => None,
                                trimmed => Some(trimmed),
                            });
                        if normalized_parent == Some(id.as_str()) {
                            let _ = evt_tx.send(CoreEvent::Error {
                                message: "Update folder failed: folder cannot be its own parent"
                                    .to_string(),
                            });
                            continue;
                        }

                        if let Some(parent_id) = normalized_parent {
                            let folders = match db.folders.list() {
                                Ok(folders) => folders,
                                Err(err) => {
                                    let _ = evt_tx.send(CoreEvent::Error {
                                        message: format!("Update folder failed: {}", err),
                                    });
                                    continue;
                                }
                            };

                            if folders.iter().all(|f| f.id != parent_id) {
                                let _ = evt_tx.send(CoreEvent::Error {
                                    message: format!(
                                        "Update folder failed: parent '{}' does not exist",
                                        parent_id
                                    ),
                                });
                                continue;
                            }

                            if introduces_cycle(&folders, &id, parent_id) {
                                let _ = evt_tx.send(CoreEvent::Error {
                                    message: "Update folder failed: would create cycle".to_string(),
                                });
                                continue;
                            }
                        }

                        match db.folders.update(&id, name, parent_update) {
                            Ok(Some(folder)) => {
                                let _ = evt_tx.send(CoreEvent::FolderSaved { folder });
                            }
                            Ok(None) => {
                                let _ = evt_tx.send(CoreEvent::Error {
                                    message: "Update folder failed: folder not found".to_string(),
                                });
                            }
                            Err(err) => {
                                error!("backend update folder failed: {}", err);
                                let _ = evt_tx.send(CoreEvent::Error {
                                    message: format!("Update folder failed: {}", err),
                                });
                            }
                        }
                    }
                    CoreCmd::DeleteFolder { id } => {
                        match delete_folder_tree_and_migrate(&db, &id) {
                            Ok(_) => {
                                let _ = evt_tx.send(CoreEvent::FolderDeleted { id });
                            }
                            Err(err) => {
                                error!("backend delete folder failed: {}", err);
                                let _ = evt_tx.send(CoreEvent::Error {
                                    message: format!("Delete folder failed: {}", err),
                                });
                            }
                        }
                    }
                }
            }
        })
        .expect("spawn backend thread");

    BackendHandle { cmd_tx, evt_rx }
}
