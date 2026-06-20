//! Background worker thread for database access.

mod folder;
mod paste;
mod query;

use crate::backend::{CoreCmd, CoreErrorSource, CoreEvent, DELETE_UNDO_LIMIT};
use chrono::Utc;
use crossbeam_channel::{unbounded, Receiver, RecvTimeoutError, Sender};
use localpaste_core::{config::env_flag_enabled, db::TransactionOps, Database};
use localpaste_server::{LockOwnerId, PasteLockManager};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use tracing::warn;

const DELETE_UNDO_TTL: Duration = Duration::from_secs(10);

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
                Ok(_) | Err(RecvTimeoutError::Timeout) => {}
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
    /// Builds a backend handle from pre-wired test channels.
    ///
    /// # Arguments
    /// - `cmd_tx`: Sender used by tests to inject backend commands.
    /// - `evt_rx`: Receiver used by tests to observe backend events.
    ///
    /// # Returns
    /// A handle with no worker thread join handle attached.
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
    search_case_sensitive: bool,
    query_cache: query::QueryCache,
    delete_undo_seq: u64,
    deleted_paste_order: VecDeque<String>,
    deleted_paste_undo: HashMap<String, PendingDeletedPaste>,
}

struct PendingDeletedPaste {
    expires_at: Instant,
}

impl WorkerState {
    fn prune_deleted_paste_undo(&mut self) {
        match TransactionOps::prune_expired_deleted_paste_undo(
            &self.db,
            Utc::now().timestamp_millis(),
        ) {
            Ok(tokens) => {
                for token in tokens {
                    self.deleted_paste_undo.remove(token.as_str());
                    self.deleted_paste_order.retain(|item| item != &token);
                }
            }
            Err(err) => warn!("failed to prune expired delete undo tombstones: {}", err),
        }
        let now = Instant::now();
        while let Some(token) = self.deleted_paste_order.front() {
            let expired = self
                .deleted_paste_undo
                .get(token)
                .map(|pending| pending.expires_at <= now)
                .unwrap_or(true);
            if !expired {
                break;
            }
            if let Some(token) = self.deleted_paste_order.pop_front() {
                self.deleted_paste_undo.remove(token.as_str());
                if let Err(err) = TransactionOps::discard_deleted_paste_undo(&self.db, &token) {
                    warn!("failed to discard expired delete undo token: {}", err);
                }
            }
        }
        while self.deleted_paste_order.len() > DELETE_UNDO_LIMIT {
            if let Some(token) = self.deleted_paste_order.pop_front() {
                self.deleted_paste_undo.remove(token.as_str());
                if let Err(err) = TransactionOps::discard_deleted_paste_undo(&self.db, &token) {
                    warn!("failed to discard overflow delete undo token: {}", err);
                }
            }
        }
    }

    fn next_deleted_paste_undo_token(&mut self) -> String {
        self.delete_undo_seq = self.delete_undo_seq.wrapping_add(1);
        format!(
            "delete-{}-{}",
            Utc::now().timestamp_millis(),
            self.delete_undo_seq
        )
    }

    fn register_deleted_paste_undo(&mut self, token: String, expires_at: Instant) {
        self.prune_deleted_paste_undo();
        self.deleted_paste_order.push_back(token.clone());
        self.deleted_paste_undo
            .insert(token, PendingDeletedPaste { expires_at });
        self.prune_deleted_paste_undo();
    }

    fn pending_deleted_paste_token(&mut self, token: &str) -> bool {
        self.prune_deleted_paste_undo();
        self.deleted_paste_undo.contains_key(token)
    }

    fn discard_deleted_paste_undo(&mut self, token: &str) {
        self.deleted_paste_undo.remove(token);
        self.deleted_paste_order.retain(|item| item != token);
        if let Err(err) = TransactionOps::discard_deleted_paste_undo(&self.db, token) {
            warn!("failed to discard delete undo token: {}", err);
        }
    }

    fn next_deleted_paste_undo_timeout(&mut self) -> Option<Duration> {
        self.prune_deleted_paste_undo();
        let token = self.deleted_paste_order.front()?;
        self.deleted_paste_undo
            .get(token)
            .map(|pending| pending.expires_at.saturating_duration_since(Instant::now()))
    }
}

fn send_error(evt_tx: &Sender<CoreEvent>, source: CoreErrorSource, message: String) {
    let _ = evt_tx.send(CoreEvent::Error { source, message });
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
        CoreCmd::GetDiffTargetPaste { id } => {
            paste::handle_get_diff_target_paste(state, id);
            true
        }
        CoreCmd::CreatePaste { content } => {
            paste::handle_create_paste(state, content);
            true
        }
        CoreCmd::UpdatePasteVirtual {
            id,
            content,
            protected_version_id_ms,
        } => {
            paste::handle_update_paste_virtual(state, id, content, protected_version_id_ms);
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
        CoreCmd::RestoreDeletedPaste { undo_token } => {
            paste::handle_restore_deleted_paste(state, undo_token);
            true
        }
        CoreCmd::ListPasteVersions { id, limit } => {
            paste::handle_list_paste_versions(state, id, limit);
            true
        }
        CoreCmd::GetPasteVersion { id, version_id_ms } => {
            paste::handle_get_paste_version(state, id, version_id_ms);
            true
        }
        CoreCmd::ResetPasteHardToVersion {
            id,
            version_id_ms,
            preserve_current_head,
        } => {
            paste::handle_reset_paste_hard_to_version(
                state,
                id,
                version_id_ms,
                preserve_current_head,
            );
            true
        }
        CoreCmd::DuplicatePasteVersion {
            id,
            version_id_ms,
            name,
        } => {
            paste::handle_duplicate_paste_version(state, id, version_id_ms, name);
            true
        }
        CoreCmd::ComputeDiffPreview {
            request_id,
            left_text,
            right_text,
        } => {
            paste::handle_compute_diff_preview(state, request_id, left_text, right_text);
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

fn recv_command_or_prune_deleted_paste_undo(
    cmd_rx: &Receiver<CoreCmd>,
    state: &mut WorkerState,
) -> Result<Option<CoreCmd>, RecvTimeoutError> {
    match state.next_deleted_paste_undo_timeout() {
        Some(timeout) => match cmd_rx.recv_timeout(timeout) {
            Ok(cmd) => Ok(Some(cmd)),
            Err(RecvTimeoutError::Timeout) => {
                state.prune_deleted_paste_undo();
                Ok(None)
            }
            Err(RecvTimeoutError::Disconnected) => Err(RecvTimeoutError::Disconnected),
        },
        None => cmd_rx
            .recv()
            .map(Some)
            .map_err(|_| RecvTimeoutError::Disconnected),
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
        crate::lock_owner::next_lock_owner_id("gui-backend-worker"),
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
                search_case_sensitive: env_flag_enabled("LOCALPASTE_SEARCH_CASE_SENSITIVE"),
                query_cache: query::QueryCache::default(),
                delete_undo_seq: 0,
                deleted_paste_order: VecDeque::new(),
                deleted_paste_undo: HashMap::new(),
            };
            loop {
                match recv_command_or_prune_deleted_paste_undo(&cmd_rx, &mut state) {
                    Ok(Some(cmd)) => {
                        if !dispatch_command(&mut state, cmd) {
                            break;
                        }
                    }
                    Ok(None) => {}
                    Err(RecvTimeoutError::Disconnected) => break,
                    Err(RecvTimeoutError::Timeout) => {}
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

#[cfg(test)]
mod tests {
    use super::*;
    use localpaste_core::models::paste::Paste;
    use tempfile::TempDir;

    struct TestWorkerState {
        _dir: TempDir,
        state: WorkerState,
        evt_rx: Receiver<CoreEvent>,
    }

    fn make_state() -> TestWorkerState {
        let dir = TempDir::new().expect("temp dir");
        let db_path = dir.path().join("db");
        let db = Database::new(db_path.to_str().expect("db path")).expect("db");
        let (evt_tx, evt_rx) = unbounded();
        TestWorkerState {
            _dir: dir,
            state: WorkerState {
                db,
                evt_tx,
                max_paste_size: 10 * 1024 * 1024,
                locks: Arc::new(PasteLockManager::default()),
                lock_owner_id: LockOwnerId::new("test-worker".to_string()),
                perf_log_enabled: false,
                search_case_sensitive: false,
                query_cache: query::QueryCache::default(),
                delete_undo_seq: 0,
                deleted_paste_order: VecDeque::new(),
                deleted_paste_undo: HashMap::new(),
            },
            evt_rx,
        }
    }

    fn stage_deleted_paste(worker: &mut TestWorkerState, id: &str) -> String {
        let mut paste = Paste::new("content".to_string(), "deleted".to_string());
        paste.id = id.to_string();
        worker
            .state
            .db
            .pastes
            .create(&paste)
            .expect("seed deleted paste");
        let token = worker.state.next_deleted_paste_undo_token();
        let expires_at = Instant::now() + DELETE_UNDO_TTL;
        let expires_at_ms = Utc::now().timestamp_millis() + DELETE_UNDO_TTL.as_millis() as i64;
        {
            let guard = TransactionOps::acquire_folder_txn_guard(&worker.state.db).expect("guard");
            let deleted = TransactionOps::delete_paste_with_folder_staged_undo_locked(
                &worker.state.db,
                &guard,
                id,
                token.as_str(),
                expires_at_ms,
            )
            .expect("stage deleted paste");
            assert!(deleted, "paste should exist");
        }
        worker
            .state
            .register_deleted_paste_undo(token.clone(), expires_at);
        token
    }

    #[test]
    fn deleted_paste_undo_token_is_consumed_after_success() {
        let mut worker = make_state();
        let token = stage_deleted_paste(&mut worker, "alpha");

        assert!(worker.state.pending_deleted_paste_token(&token));
        worker.state.discard_deleted_paste_undo(&token);
        assert!(
            !worker.state.pending_deleted_paste_token(&token),
            "a consumed undo token must not restore the same tombstone twice"
        );
    }

    #[test]
    fn deleted_paste_undo_rejects_expired_token() {
        let mut worker = make_state();
        let token = stage_deleted_paste(&mut worker, "alpha");
        worker
            .state
            .deleted_paste_undo
            .get_mut(token.as_str())
            .expect("registered undo")
            .expires_at = Instant::now() - Duration::from_secs(1);

        assert!(
            !worker.state.pending_deleted_paste_token(&token),
            "expired undo token should be pruned before restore"
        );
        assert!(!worker.state.deleted_paste_undo.contains_key(&token));
        assert!(!worker.state.deleted_paste_order.contains(&token));
        assert!(
            TransactionOps::restore_deleted_paste_by_token(&worker.state.db, &token)
                .expect("restore lookup")
                .is_none(),
            "expired memory token should discard the staged tombstone"
        );
    }

    #[test]
    fn expired_restore_emits_nonretryable_restore_failure() {
        let mut worker = make_state();
        let token = stage_deleted_paste(&mut worker, "alpha");
        worker
            .state
            .deleted_paste_undo
            .get_mut(token.as_str())
            .expect("registered undo")
            .expires_at = Instant::now() - Duration::from_secs(1);

        paste::handle_restore_deleted_paste(&mut worker.state, token.clone());

        match worker
            .evt_rx
            .recv_timeout(Duration::from_millis(200))
            .expect("restore failure event")
        {
            CoreEvent::PasteRestoreFailed {
                undo_token,
                message,
                retryable,
            } => {
                assert_eq!(undo_token, token);
                assert_eq!(message, "Undo delete expired.");
                assert!(!retryable);
            }
            other => panic!("expected restore failure event, got {:?}", other),
        }
        assert!(!worker.state.deleted_paste_undo.contains_key(&token));
    }

    #[test]
    fn deleted_paste_undo_is_pruned_after_idle_timeout() {
        let mut worker = make_state();
        let token = stage_deleted_paste(&mut worker, "alpha");
        worker
            .state
            .deleted_paste_undo
            .get_mut(token.as_str())
            .expect("registered undo")
            .expires_at = Instant::now() + Duration::from_millis(10);

        let (_cmd_tx, cmd_rx) = unbounded();
        let result = recv_command_or_prune_deleted_paste_undo(&cmd_rx, &mut worker.state)
            .expect("idle timeout should prune instead of disconnecting");

        assert!(
            result.is_none(),
            "idle timeout should not synthesize a backend command"
        );
        assert!(!worker.state.deleted_paste_undo.contains_key(&token));
        assert!(!worker.state.deleted_paste_order.contains(&token));
        assert!(
            TransactionOps::restore_deleted_paste_by_token(&worker.state.db, &token)
                .expect("restore lookup")
                .is_none(),
            "idle pruning should discard the staged tombstone"
        );
    }

    #[test]
    fn failed_restore_keeps_deleted_paste_undo_retryable() {
        let mut worker = make_state();
        let token = stage_deleted_paste(&mut worker, "alpha");
        let mut existing = Paste::new("live content".to_string(), "live".to_string());
        existing.id = "alpha".to_string();
        worker
            .state
            .db
            .pastes
            .create(&existing)
            .expect("seed conflicting paste");

        paste::handle_restore_deleted_paste(&mut worker.state, token.clone());

        match worker
            .evt_rx
            .recv_timeout(Duration::from_millis(200))
            .expect("restore error event")
        {
            CoreEvent::PasteRestoreFailed {
                undo_token,
                message,
                retryable,
            } => {
                assert_eq!(undo_token, token);
                assert!(retryable);
                assert!(
                    message.contains("already exists"),
                    "expected collision error, got: {}",
                    message
                );
            }
            other => panic!("expected restore error event, got {:?}", other),
        }
        assert!(
            worker.state.pending_deleted_paste_token(&token),
            "failed restore must preserve the undo token for retry"
        );
    }
}
