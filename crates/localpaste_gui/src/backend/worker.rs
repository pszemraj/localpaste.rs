//! Background worker thread for database access.

mod folder;
mod paste;
mod query;

use crate::backend::{CoreCmd, CoreErrorSource, CoreEvent};
use crossbeam_channel::{unbounded, Receiver, RecvTimeoutError, Sender};
use localpaste_core::{config::env_flag_enabled, Database};
use localpaste_server::{LockOwnerId, PasteLockManager};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// Handle for sending commands to, and receiving events from, the backend worker.
pub struct BackendHandle {
    pub cmd_tx: Sender<CoreCmd>,
    pub evt_rx: Receiver<CoreEvent>,
    worker_join: Option<thread::JoinHandle<()>>,
}

impl BackendHandle {
    /// Ask the backend worker to stop after draining queued commands.
    ///
    /// # Arguments
    /// - `flush`: When `true`, request an explicit database flush before exit.
    ///
    /// # Returns
    /// `Ok(())` after the shutdown command is queued.
    ///
    /// # Errors
    /// Returns an error if the shutdown command cannot be sent.
    pub fn request_shutdown(&self, flush: bool) -> Result<(), String> {
        self.cmd_tx.send(CoreCmd::Shutdown { flush }).map_err(|_| {
            "backend shutdown request failed: worker command channel closed".to_string()
        })
    }

    /// Wait for shutdown acknowledgement and join the backend worker thread.
    ///
    /// # Arguments
    /// - `flush`: When `true`, request an explicit database flush before exit.
    /// - `timeout`: Maximum time to wait for `ShutdownComplete` acknowledgement.
    ///
    /// # Returns
    /// `Ok(())` when shutdown is acknowledged and the worker thread is joined.
    ///
    /// # Errors
    /// Returns an error when the worker fails to acknowledge shutdown in time,
    /// reports a flush failure, or panics during join.
    pub fn shutdown_and_join(&mut self, flush: bool, timeout: Duration) -> Result<(), String> {
        if self.worker_join.is_none() {
            return Ok(());
        }
        self.request_shutdown(flush)?;
        let deadline = Instant::now() + timeout;
        let mut saw_ack = false;

        while Instant::now() < deadline {
            let wait_for = deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(25));
            if wait_for.is_zero() {
                break;
            }
            match self.evt_rx.recv_timeout(wait_for) {
                Ok(CoreEvent::ShutdownComplete { flush_result }) => {
                    saw_ack = true;
                    if let Err(message) = flush_result {
                        return Err(format!("backend shutdown flush failed: {}", message));
                    }
                    break;
                }
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    return self.join_worker();
                }
            }
        }

        if !saw_ack {
            return Err(format!(
                "backend shutdown timed out after {} ms",
                timeout.as_millis()
            ));
        }
        self.join_worker()
    }

    /// Join the backend worker thread when this handle owns one.
    ///
    /// # Returns
    /// `Ok(())` once the worker has been joined or when no join handle exists.
    ///
    /// # Errors
    /// Returns an error if the worker thread panicked.
    pub fn join_worker(&mut self) -> Result<(), String> {
        let Some(join) = self.worker_join.take() else {
            return Ok(());
        };
        join.join()
            .map_err(|_| "backend worker thread panicked".to_string())
    }

    #[cfg(test)]
    pub(crate) fn from_test_channels(cmd_tx: Sender<CoreCmd>, evt_rx: Receiver<CoreEvent>) -> Self {
        Self {
            cmd_tx,
            evt_rx,
            worker_join: None,
        }
    }
}

struct WorkerState {
    db: Database,
    evt_tx: Sender<CoreEvent>,
    max_paste_size: usize,
    locks: Arc<PasteLockManager>,
    lock_owner_id: LockOwnerId,
    perf_log_enabled: bool,
    query_cache: query::QueryCache,
}

fn send_error(evt_tx: &Sender<CoreEvent>, source: CoreErrorSource, message: String) {
    let _ = evt_tx.send(CoreEvent::Error { source, message });
}

fn validate_paste_size_bytes(content_len: usize, max_paste_size: usize) -> Result<(), String> {
    if content_len > max_paste_size {
        Err(format!(
            "Paste size exceeds maximum of {} bytes",
            max_paste_size
        ))
    } else {
        Ok(())
    }
}

fn validate_paste_size(content: &str, max_paste_size: usize) -> Result<(), String> {
    validate_paste_size_bytes(content.len(), max_paste_size)
}

fn dispatch_command(state: &mut WorkerState, cmd: CoreCmd) -> bool {
    match cmd {
        CoreCmd::ListPastes { limit, folder_id } => {
            query::handle_list_pastes(state, limit, folder_id);
            true
        }
        CoreCmd::SearchPastes {
            query,
            limit,
            folder_id,
            language,
        } => {
            query::handle_search(
                state,
                query::SearchRoute::Standard {
                    folder_id,
                    language,
                },
                query,
                limit,
            );
            true
        }
        CoreCmd::SearchPalette { query, limit } => {
            query::handle_search(state, query::SearchRoute::Palette, query, limit);
            true
        }
        CoreCmd::GetPaste { id } => {
            paste::handle_get_paste(state, id);
            true
        }
        CoreCmd::CreatePaste { content } => {
            paste::handle_create_paste(state, content);
            true
        }
        CoreCmd::UpdatePaste { id, content } => {
            paste::handle_update_paste(state, id, content);
            true
        }
        CoreCmd::UpdatePasteVirtual { id, content } => {
            paste::handle_update_paste_virtual(state, id, content);
            true
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
            true
        }
        CoreCmd::DeletePaste { id } => {
            paste::handle_delete_paste(state, id);
            true
        }
        CoreCmd::ListFolders => {
            folder::handle_list_folders(state);
            true
        }
        CoreCmd::CreateFolder { name, parent_id } => {
            folder::handle_create_folder(state, name, parent_id);
            true
        }
        CoreCmd::UpdateFolder {
            id,
            name,
            parent_id,
        } => {
            folder::handle_update_folder(state, id, name, parent_id);
            true
        }
        CoreCmd::DeleteFolder { id } => {
            folder::handle_delete_folder(state, id);
            true
        }
        CoreCmd::Shutdown { flush } => {
            let flush_result = if flush {
                state.db.flush().map_err(|err| err.to_string())
            } else {
                Ok(())
            };
            let _ = state
                .evt_tx
                .send(CoreEvent::ShutdownComplete { flush_result });
            false
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
    spawn_backend_with_locks_and_owner(
        db,
        max_paste_size,
        locks,
        LockOwnerId::new("gui-backend-worker"),
    )
}

/// Spawn the backend worker thread with a shared lock manager and owner id.
///
/// # Arguments
/// - `db`: Open database handle shared by backend command handlers.
/// - `max_paste_size`: Maximum allowed paste content size in bytes.
/// - `locks`: Shared paste lock manager used for lock-aware operations.
/// - `lock_owner_id`: Owner id representing this backend's in-process GUI owner.
///
/// # Returns
/// A [`BackendHandle`] containing the command sender and event receiver.
///
/// # Panics
/// Panics if the worker thread cannot be spawned.
pub fn spawn_backend_with_locks_and_owner(
    db: Database,
    max_paste_size: usize,
    locks: Arc<PasteLockManager>,
    lock_owner_id: LockOwnerId,
) -> BackendHandle {
    let (cmd_tx, cmd_rx) = unbounded();
    let (evt_tx, evt_rx) = unbounded();

    let worker_join = thread::Builder::new()
        .name("localpaste-gui-backend".to_string())
        .spawn(move || {
            localpaste_core::detection::prewarm();
            let mut state = WorkerState {
                db,
                evt_tx,
                max_paste_size,
                locks,
                lock_owner_id,
                perf_log_enabled: env_flag_enabled("LOCALPASTE_BACKEND_PERF_LOG"),
                query_cache: query::QueryCache::default(),
            };
            for cmd in cmd_rx.iter() {
                if !dispatch_command(&mut state, cmd) {
                    break;
                }
            }
        })
        .expect("spawn backend thread");

    BackendHandle {
        cmd_tx,
        evt_rx,
        worker_join: Some(worker_join),
    }
}
