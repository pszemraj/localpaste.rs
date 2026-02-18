//! UI-facing feedback helpers for status, toasts, and export completion.

use super::{LocalPasteApp, StatusMessage, ToastMessage, STATUS_TTL, TOAST_LIMIT, TOAST_TTL};
use std::time::Instant;

impl LocalPasteApp {
    /// Sets the status banner message and mirrors it into the toast queue.
    pub(super) fn set_status(&mut self, text: impl Into<String>) {
        let text = text.into();
        self.status = Some(StatusMessage {
            text: text.clone(),
            expires_at: Instant::now() + STATUS_TTL,
        });
        self.push_toast(text);
    }

    fn push_toast(&mut self, text: String) {
        let now = Instant::now();
        if let Some(last) = self.toasts.back_mut() {
            if last.text == text {
                last.expires_at = now + TOAST_TTL;
                return;
            }
        }
        self.toasts.push_back(ToastMessage {
            text,
            expires_at: now + TOAST_TTL,
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
