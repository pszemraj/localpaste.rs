//! UI-facing feedback helpers for status, toasts, and export completion.

use super::{
    LocalPasteApp, StatusMessage, ToastAction, ToastMessage, DELETE_UNDO_LIMIT, STATUS_TTL,
    TOAST_LIMIT, TOAST_TTL, UNDO_DELETE_TOAST_TTL,
};
use crate::backend::CoreCmd;
use std::time::Instant;

impl LocalPasteApp {
    /// Sets the status banner message and mirrors it into the toast queue.
    pub(super) fn set_status(&mut self, text: impl Into<String>) {
        self.set_status_inner(text.into(), None);
    }

    /// Sets status text and mirrors it into a toast with an attached action.
    ///
    /// # Arguments
    /// - `text`: User-facing status/toast message.
    /// - `action`: Toast action rendered alongside the message.
    pub(super) fn set_status_with_action(&mut self, text: impl Into<String>, action: ToastAction) {
        self.set_status_inner(text.into(), Some(action));
    }

    fn set_status_inner(&mut self, text: String, action: Option<ToastAction>) {
        self.status = Some(StatusMessage {
            text: text.clone(),
            expires_at: Instant::now() + STATUS_TTL,
        });
        self.push_toast(text, action);
    }

    fn push_toast(&mut self, text: String, action: Option<ToastAction>) {
        let now = Instant::now();
        let ttl = if action.is_some() {
            UNDO_DELETE_TOAST_TTL
        } else {
            TOAST_TTL
        };
        if let Some(last) = self.toasts.back_mut() {
            if last.text == text && last.action == action {
                last.expires_at = now + ttl;
                return;
            }
        }
        self.toasts.push_back(ToastMessage {
            text,
            expires_at: now + ttl,
            action,
        });
        self.prune_toast_overflow();
    }

    /// Removes every toast whose expiration has passed.
    pub(super) fn prune_expired_toasts(&mut self, now: Instant) {
        self.toasts.retain(|toast| now < toast.expires_at);
    }

    /// Removes any pending undo-delete toast for a consumed or expired token.
    pub(super) fn remove_undo_toast(&mut self, undo_token: &str) {
        self.toasts.retain(|toast| {
            !matches!(
                &toast.action,
                Some(ToastAction::UndoDelete { undo_token: token }) if token == undo_token
            )
        });
    }

    /// Returns the next toast expiration time, regardless of queue order.
    ///
    /// # Returns
    /// Earliest toast expiration, or `None` when no toasts are queued.
    pub(super) fn next_toast_expiration(&self) -> Option<Instant> {
        self.toasts.iter().map(|toast| toast.expires_at).min()
    }

    fn prune_toast_overflow(&mut self) {
        // Undo toasts mirror the backend's bounded live-token queue, so they are
        // allowed to exceed the ordinary status-toast cap.
        while self.undo_delete_toast_count() > DELETE_UNDO_LIMIT {
            let Some(index) = self.oldest_undo_delete_toast_index() else {
                break;
            };
            self.toasts.remove(index);
        }

        while self.toasts.len() > TOAST_LIMIT {
            let Some(index) = self.toasts.iter().position(|toast| toast.action.is_none()) else {
                break;
            };
            self.toasts.remove(index);
        }
    }

    fn undo_delete_toast_count(&self) -> usize {
        self.toasts
            .iter()
            .filter(|toast| matches!(&toast.action, Some(ToastAction::UndoDelete { .. })))
            .count()
    }

    fn oldest_undo_delete_toast_index(&self) -> Option<usize> {
        self.toasts
            .iter()
            .position(|toast| matches!(&toast.action, Some(ToastAction::UndoDelete { .. })))
    }

    /// Requests restoration of a recently deleted paste from an undo token.
    pub(super) fn restore_deleted_paste(&mut self, undo_token: String) {
        let toast_is_live = self.toasts.iter().any(|toast| {
            matches!(
                &toast.action,
                Some(ToastAction::UndoDelete { undo_token: token }) if token == &undo_token
            )
        });
        if !toast_is_live {
            return;
        }
        if self.pending_undo_restore_tokens.contains(&undo_token) {
            return;
        }
        let sent = self.send_backend_cmd_or_status(
            CoreCmd::RestoreDeletedPaste {
                undo_token: undo_token.clone(),
            },
            "Undo delete failed: backend unavailable.",
        );
        if sent {
            self.pending_undo_restore_tokens.insert(undo_token);
            self.set_status("Restoring deleted paste...");
        }
    }

    /// Polls asynchronous export completion and reports success/failure to status.
    pub(super) fn poll_export_result(&mut self) {
        let completion = {
            let Some(rx) = self.export_result_rx.as_ref() else {
                return;
            };
            match rx.try_recv() {
                Ok(completion) => Some(completion),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.export_result_rx = None;
                    self.set_status("Export failed: worker disconnected.");
                    return;
                }
            }
        };
        let Some(completion) = completion else {
            return;
        };
        self.export_result_rx = None;
        match completion.result {
            Ok(()) => {
                self.set_status(format!(
                    "Exported {} to {}",
                    completion.paste_id, completion.path
                ));
            }
            Err(err) => {
                self.set_status(format!("Export failed: {}", err));
            }
        }
    }
}
