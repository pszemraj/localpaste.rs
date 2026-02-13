//! Background worker thread for database access.

mod folder;
mod paste;
mod query;

use crate::backend::{CoreCmd, CoreErrorSource, CoreEvent};
use crossbeam_channel::{unbounded, Receiver, Sender};
use localpaste_core::{config::env_flag_enabled, Database};
use localpaste_server::PasteLockManager;
use std::sync::Arc;
use std::thread;

/// Handle for sending commands to, and receiving events from, the backend worker.
pub struct BackendHandle {
    pub cmd_tx: Sender<CoreCmd>,
    pub evt_rx: Receiver<CoreEvent>,
}

struct WorkerState {
    db: Database,
    evt_tx: Sender<CoreEvent>,
    max_paste_size: usize,
    locks: Arc<PasteLockManager>,
    perf_log_enabled: bool,
    query_cache: query::QueryCache,
}

fn send_error(evt_tx: &Sender<CoreEvent>, source: CoreErrorSource, message: String) {
    let _ = evt_tx.send(CoreEvent::Error { source, message });
}

fn validate_paste_size(content: &str, max_paste_size: usize) -> Result<(), String> {
    if content.len() > max_paste_size {
        Err(format!(
            "Paste size exceeds maximum of {} bytes",
            max_paste_size
        ))
    } else {
        Ok(())
    }
}

fn dispatch_command(state: &mut WorkerState, cmd: CoreCmd) {
    match cmd {
        CoreCmd::ListPastes { limit, folder_id } => {
            query::handle_list_pastes(state, limit, folder_id);
        }
        CoreCmd::SearchPastes {
            query,
            limit,
            folder_id,
            language,
        } => {
            query::handle_search_pastes(state, query, limit, folder_id, language);
        }
        CoreCmd::SearchPalette { query, limit } => {
            query::handle_palette_search(state, query, limit);
        }
        CoreCmd::GetPaste { id } => {
            paste::handle_get_paste(state, id);
        }
        CoreCmd::CreatePaste { content } => {
            paste::handle_create_paste(state, content);
        }
        CoreCmd::UpdatePaste { id, content } => {
            paste::handle_update_paste(state, id, content);
        }
        CoreCmd::UpdatePasteVirtual { id, content } => {
            paste::handle_update_paste_virtual(state, id, content);
        }
        CoreCmd::UpdatePasteMeta {
            id,
            name,
            language,
            language_is_manual,
            folder_id,
            tags,
        } => {
            paste::handle_update_paste_meta(
                state,
                id,
                name,
                language,
                language_is_manual,
                folder_id,
                tags,
            );
        }
        CoreCmd::DeletePaste { id } => {
            paste::handle_delete_paste(state, id);
        }
        CoreCmd::ListFolders => {
            folder::handle_list_folders(state);
        }
        CoreCmd::CreateFolder { name, parent_id } => {
            folder::handle_create_folder(state, name, parent_id);
        }
        CoreCmd::UpdateFolder {
            id,
            name,
            parent_id,
        } => {
            folder::handle_update_folder(state, id, name, parent_id);
        }
        CoreCmd::DeleteFolder { id } => {
            folder::handle_delete_folder(state, id);
        }
    }
}

/// Spawn the backend worker thread that performs blocking database access.
///
/// All I/O stays off the UI thread; the worker replies with [`CoreEvent`] values
/// that are polled each frame.
///
/// # Arguments
/// - `db`: Open database handle shared by backend command handlers.
/// - `max_paste_size`: Maximum allowed paste content size in bytes.
///
/// # Returns
/// A [`BackendHandle`] containing the command sender and event receiver.
///
/// # Panics
/// Panics if the worker thread cannot be spawned.
pub fn spawn_backend(db: Database, max_paste_size: usize) -> BackendHandle {
    spawn_backend_with_locks(db, max_paste_size, Arc::new(PasteLockManager::default()))
}

/// Spawn the backend worker thread with a shared lock manager.
///
/// # Arguments
/// - `db`: Open database handle shared by backend command handlers.
/// - `max_paste_size`: Maximum allowed paste content size in bytes.
/// - `locks`: Shared paste lock manager used for lock-aware bulk operations.
///
/// # Returns
/// A [`BackendHandle`] containing the command sender and event receiver.
///
/// # Panics
/// Panics if the worker thread cannot be spawned.
pub fn spawn_backend_with_locks(
    db: Database,
    max_paste_size: usize,
    locks: Arc<PasteLockManager>,
) -> BackendHandle {
    let (cmd_tx, cmd_rx) = unbounded();
    let (evt_tx, evt_rx) = unbounded();

    thread::Builder::new()
        .name("localpaste-gui-backend".to_string())
        .spawn(move || {
            let mut state = WorkerState {
                db,
                evt_tx,
                max_paste_size,
                locks,
                perf_log_enabled: env_flag_enabled("LOCALPASTE_BACKEND_PERF_LOG"),
                query_cache: query::QueryCache::default(),
            };
            for cmd in cmd_rx.iter() {
                dispatch_command(&mut state, cmd);
            }
        })
        .expect("spawn backend thread");

    BackendHandle { cmd_tx, evt_rx }
}
