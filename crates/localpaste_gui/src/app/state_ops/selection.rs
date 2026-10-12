//! Selection switching, deferral behind pending saves, and edit-lock handoff.

use super::super::deferred_saves::rollback_deferred_save_dispatches;
use super::*;

impl LocalPasteApp {
    /// Selects a paste by id, deferring selection when unsaved edits must be flushed first.
    /// # Returns
    /// `true` when selection was applied or successfully deferred, otherwise `false`.
    pub(in crate::app) fn select_paste(&mut self, id: String) -> bool {
        if self.picker_delete_transition_active() {
            self.set_picker_delete_transition_blocked_status();
            return false;
        }
        // Detached version workflows own the current subject paste; switching away would
        // invalidate the open modal context and, during reset, release the held lock too early.
        if self.selection_transition_block_reason().is_some() {
            self.set_selection_transition_blocked_status();
            return false;
        }
        if self.selected_id.as_deref() == Some(id.as_str()) {
            self.picker_selection_pin = None;
            self.clear_pending_selection_request();
            return true;
        }
        if self.save_status == SaveStatus::Dirty || self.metadata_dirty {
            let previous_pending_selection = self.pending_selection_id.clone();
            let previous_picker_open = self.pending_picker_open.clone();
            let previous_picker_pin = self.pending_picker_selection_pin.clone();
            self.queue_pending_selection(id);
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
                self.pending_selection_id = previous_pending_selection;
                self.pending_picker_open = previous_picker_open;
                self.pending_picker_selection_pin = previous_picker_pin;
                return false;
            }
            self.cancel_pending_delete();
            self.set_status("Saving current paste before switching...");
            return true;
        }
        let save_in_progress = self.save_in_flight
            || self.metadata_save_in_flight
            || self.save_status == SaveStatus::Saving;
        if save_in_progress {
            self.queue_pending_selection(id);
            self.cancel_pending_delete();
            self.set_status("Saving current paste before switching...");
            return true;
        }
        self.apply_selection_now(id)
    }

    /// Queues `id` as the selection to apply once saves and workflow fences clear,
    /// dropping picker context that belongs to a different target.
    pub(super) fn queue_pending_selection(&mut self, id: String) {
        if self.pending_picker_selection_pin.as_deref() != Some(id.as_str()) {
            self.pending_picker_selection_pin = None;
        }
        if self
            .pending_picker_open
            .as_ref()
            .is_some_and(|opening| opening.id != id)
        {
            self.pending_picker_open = None;
        }
        if self.pending_selection_id.as_deref() == Some(id.as_str()) {
            return;
        }
        self.pending_selection_id = Some(id);
    }

    /// Pins a queued switch to `id`, a paste the sidebar does not list.
    ///
    /// The loaded paste stays pinned until the switch applies and transfers the
    /// pin to `id`, so list refreshes and search replies replace neither.
    pub(super) fn pin_queued_selection(&mut self, id: String) {
        self.pending_picker_selection_pin = Some(id);
        self.picker_selection_pin = self.selected_id.clone();
    }

    /// Cancels any queued selection switch that has not been applied yet.
    pub(in crate::app) fn clear_pending_selection_request(&mut self) {
        self.pending_selection_id = None;
        self.pending_picker_open = None;
        self.pending_picker_selection_pin = None;
    }

    /// Applies a fully loaded paste into editor state and resets transient edit caches.
    pub(in crate::app) fn select_loaded_paste(&mut self, paste: Paste) {
        let id = paste.id.clone();
        if self.selected_id.as_deref() != Some(id.as_str()) {
            if !self.acquire_paste_lock(id.as_str()) {
                self.pending_picker_open = None;
                return;
            }
            if let Some(prev) = self.selected_id.replace(id.clone()) {
                self.release_paste_lock(prev.as_str());
            }
        }
        self.sync_editor_metadata(&paste);
        self.bump_active_buffer_epoch();
        self.reset_virtual_editor(paste.content.as_str());
        self.clear_highlight_state();
        self.selected_paste = Some(paste);
        if !self.prime_editor_find_from_picker_open() {
            self.prime_editor_find_from_sidebar_query();
        }
        self.save_status = SaveStatus::Saved;
        self.last_edit_at = None;
        self.save_in_flight = false;
        self.save_request_revision = None;
        self.metadata_save_in_flight = false;
        self.metadata_save_request = None;
        self.clear_version_view_state();
    }

    fn reset_selection_editor_state(&mut self) {
        self.selected_paste = None;
        self.edit_name.clear();
        self.edit_language = None;
        self.edit_language_is_manual = false;
        self.edit_tags.clear();
        self.metadata_dirty = false;
        self.metadata_save_in_flight = false;
        self.metadata_save_request = None;
        self.bump_active_buffer_epoch();
        self.reset_virtual_editor("");
        self.clear_highlight_state();
        self.save_status = SaveStatus::Saved;
        self.last_edit_at = None;
        self.save_in_flight = false;
        self.save_request_revision = None;
        self.clear_version_view_state();
    }

    /// Switches the selection to `id` and requests its body, transferring a matching pending pin.
    ///
    /// # Returns
    /// `true` when the target lock was acquired and its load was dispatched.
    pub(super) fn apply_selection_now(&mut self, id: String) -> bool {
        // Acquire target lock before releasing current selection lock so failed
        // switches never drop the currently editable paste unexpectedly.
        if !self.acquire_paste_lock(id.as_str()) {
            if self.pending_picker_selection_pin.as_deref() == Some(id.as_str()) {
                self.pending_picker_selection_pin = None;
            }
            if self
                .pending_picker_open
                .as_ref()
                .is_some_and(|opening| opening.id == id)
            {
                // A failed target must not keep capturing editor input.
                self.pending_picker_open = None;
            }
            return false;
        }
        self.cancel_pending_delete();
        self.pending_selection_id = None;
        if self.pending_picker_selection_pin.take().as_deref() == Some(id.as_str()) {
            self.picker_selection_pin = Some(id.clone());
        } else {
            self.pending_picker_open = None;
            self.picker_selection_pin = None;
        }
        if let Some(prev) = self.selected_id.replace(id.clone()) {
            self.release_paste_lock(prev.as_str());
        }
        self.reset_selection_editor_state();
        if !self.dispatch_backend_cmd(CoreCmd::GetPaste {
            id,
            selection_epoch: self.active_buffer_epoch,
        }) {
            self.clear_selection();
            self.notify("Get paste failed: backend unavailable.");
            return false;
        }
        true
    }

    /// Applies a queued selection switch once save and workflow fences have cleared.
    pub(in crate::app) fn try_apply_pending_selection(&mut self) {
        if self.selection_transition_block_reason().is_some() {
            return;
        }
        if self.save_in_flight || self.metadata_save_in_flight {
            return;
        }
        if self.save_status == SaveStatus::Saving {
            return;
        }
        if self.save_status == SaveStatus::Dirty || self.metadata_dirty {
            return;
        }
        let Some(pending) = self.pending_selection_id.take() else {
            return;
        };
        let _ = self.apply_selection_now(pending);
    }

    /// Clears active/pending selection and releases any held paste lock.
    pub(in crate::app) fn clear_selection(&mut self) {
        self.picker_selection_pin = None;
        self.clear_pending_selection_request();
        self.cancel_pending_delete();
        if let Some(prev) = self.selected_id.take() {
            self.release_paste_lock(prev.as_str());
        }
        self.reset_selection_editor_state();
    }

    /// Discards picker-open and selection pins owned by a removed paste.
    pub(in crate::app) fn clear_picker_selection_context_for(&mut self, id: &str) {
        if self.pending_picker_selection_pin.as_deref() == Some(id) {
            self.pending_picker_selection_pin = None;
        }
        if self.picker_selection_pin.as_deref() == Some(id) {
            self.picker_selection_pin = None;
        }
        if self
            .pending_picker_open
            .as_ref()
            .is_some_and(|opening| opening.id == id)
        {
            self.pending_picker_open = None;
        }
    }
}
