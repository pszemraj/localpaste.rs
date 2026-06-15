//! UI-facing feedback helpers for status, toasts, and export completion.

use super::{
    LocalPasteApp, StatusMessage, ToastAction, ToastMessage, STATUS_TTL, TOAST_LIMIT, TOAST_TTL,
    UNDO_DELETE_TOAST_TTL,
};
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
        while self.toasts.len() > TOAST_LIMIT {
            self.toasts.pop_front();
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
