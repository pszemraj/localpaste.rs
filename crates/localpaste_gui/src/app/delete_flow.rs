//! Delete workflows that must coordinate with local editor save state.

use super::deferred_saves::rollback_deferred_save_dispatches;
use super::{LocalPasteApp, SaveStatus};
use crate::backend::CoreCmd;

impl LocalPasteApp {
    /// Sends a delete command for `id` and reports whether dispatch succeeded.
    /// # Returns
    /// `true` when the backend command was queued or accepted for deferred dispatch, otherwise `false`.
    pub(super) fn send_delete_paste(&mut self, id: String) -> bool {
        if self.mutation_shortcut_block_reason().is_some() {
            self.set_mutation_shortcut_blocked_status();
            return false;
        }
        if self.history_reset_pending_for(id.as_str()) {
            self.set_reset_transition_blocked_status();
            return false;
        }
        if self.delete_requires_selected_flush(id.as_str()) {
            return self.queue_delete_after_selected_flush(id);
        }
        self.send_backend_cmd_or_status(
            CoreCmd::DeletePaste { id },
            "Delete failed: backend unavailable.",
        )
    }

    /// Deletes the currently selected paste, if any.
    pub(super) fn delete_selected(&mut self) {
        if let Some(id) = self.selected_id.clone() {
            let _sent = self.send_delete_paste(id);
        }
    }

    fn delete_requires_selected_flush(&self, id: &str) -> bool {
        self.selected_id.as_deref() == Some(id)
            && (self.save_status != SaveStatus::Saved
                || self.save_in_flight
                || self.metadata_dirty
                || self.metadata_save_in_flight)
    }

    fn queue_delete_after_selected_flush(&mut self, id: String) -> bool {
        self.pending_delete_id = Some(id);
        let content_save_needed = self.save_status == SaveStatus::Dirty;
        let metadata_save_needed = self.metadata_dirty;
        if content_save_needed {
            self.save_now();
        }
        if metadata_save_needed {
            self.save_metadata_now();
        }

        let content_save_dispatched = !content_save_needed || self.save_in_flight;
        let metadata_save_dispatched = !metadata_save_needed || self.metadata_save_in_flight;
        if !content_save_dispatched || !metadata_save_dispatched {
            rollback_deferred_save_dispatches(self, content_save_needed, metadata_save_needed);
            self.pending_delete_id = None;
            self.set_status("Delete cancelled because current paste could not be saved.");
            return false;
        }

        self.set_status("Saving current paste before deleting...");
        self.maybe_continue_pending_delete();
        true
    }

    /// Clears any selected-paste delete waiting on content or metadata saves.
    pub(super) fn cancel_pending_delete(&mut self) {
        self.pending_delete_id = None;
    }

    /// Continues a queued selected-paste delete after save state settles.
    pub(super) fn maybe_continue_pending_delete(&mut self) {
        let Some(paste_id) = self.pending_delete_id.clone() else {
            return;
        };
        // Destructive selected-paste delete only crosses into the backend once
        // every GUI-owned selected-paste buffer has been persisted.
        if self.selected_id.as_deref() != Some(paste_id.as_str()) {
            self.pending_delete_id = None;
            self.set_status("Delete cancelled because the selected paste changed.");
            return;
        }
        if self.delete_requires_selected_flush(paste_id.as_str()) {
            if (self.save_status == SaveStatus::Dirty && !self.save_in_flight)
                || (self.metadata_dirty && !self.metadata_save_in_flight)
            {
                let _ = self.queue_delete_after_selected_flush(paste_id);
            }
            return;
        }
        self.pending_delete_id = None;
        let _sent = self.send_delete_paste(paste_id);
    }
}
