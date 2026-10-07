//! Delete workflows that must coordinate with local editor save state.

use super::deferred_saves::rollback_deferred_save_dispatches;
use super::{LocalPasteApp, PickerDeleteTransition, SaveStatus, ToastAction, SEARCH_DEBOUNCE};
use crate::backend::CoreCmd;
use std::time::Instant;

/// Status explaining why discovery retains input during a selected-paste delete.
pub(super) const PICKER_DELETE_TRANSITION_BLOCKED_STATUS: &str =
    "Finishing deletion before returning to the editor...";

impl LocalPasteApp {
    /// Applies a delete acknowledgement and retains picker ownership through replacement loading.
    ///
    /// # Arguments
    /// - `id`: Paste whose deletion has completed.
    /// - `undo_token`: Recovery action available for this acknowledged deletion.
    pub(super) fn apply_paste_deleted(&mut self, id: String, undo_token: Option<String>) {
        let deleted_index = self.pastes.iter().position(|paste| paste.id == id);
        let was_selected = self.selected_id.as_deref() == Some(id.as_str());
        let picker_delete_transition = if self.picker_delete_matches_deleted(id.as_str()) {
            self.picker_delete_transition.take()
        } else {
            None
        };
        let requested_replacement_id = self
            .pending_selection_id
            .clone()
            .filter(|pending_id| pending_id != &id);
        self.all_pastes.retain(|paste| paste.id != id);
        self.pastes.retain(|paste| paste.id != id);
        self.palette_search_results.retain(|paste| paste.id != id);
        if self.paste_picker_open {
            self.palette_search_last_sent.clear();
            self.palette_search_last_input_at = Some(Instant::now() - SEARCH_DEBOUNCE);
        }
        self.clear_pending_palette_copy_for(id.as_str());
        self.clear_picker_selection_context_for(id.as_str());
        if was_selected {
            let replacement_id = requested_replacement_id.or_else(|| {
                deleted_index.and_then(|index| {
                    self.pastes
                        .get(index)
                        .or_else(|| index.checked_sub(1).and_then(|prev| self.pastes.get(prev)))
                        .map(|paste| paste.id.clone())
                })
            });
            self.clear_selection();
            if let Some(replacement_id) = replacement_id {
                if let Some(transition) = picker_delete_transition {
                    self.select_picker_delete_replacement(transition, replacement_id);
                } else {
                    let _ = self.select_paste(replacement_id);
                }
            }
            if let Some(undo_token) = undo_token {
                self.set_status_with_action(
                    "Paste deleted.",
                    ToastAction::UndoDelete { undo_token },
                );
            } else {
                self.set_status("Paste deleted. Undo unavailable.");
            }
        } else if let Some(undo_token) = undo_token {
            self.set_status_with_action(
                "Paste deleted; list refreshed.",
                ToastAction::UndoDelete { undo_token },
            );
        } else {
            self.set_status("Paste deleted; list refreshed. Undo unavailable.");
        }
        self.request_refresh();
    }

    /// Whether a selected-picker delete still owns keyboard and selection state.
    ///
    /// # Returns
    /// `true` until the accepted delete and replacement load have settled.
    pub(super) fn picker_delete_transition_active(&self) -> bool {
        self.picker_delete_transition.is_some()
    }

    /// Starts the ownership fence for a selected paste deleted from the picker.
    pub(super) fn begin_picker_delete_transition(&mut self, deleted_id: String) {
        self.picker_delete_transition = Some(PickerDeleteTransition {
            deleted_id,
            replacement_id: None,
        });
    }

    /// Reports the shared status while the selected-picker delete is settling.
    pub(super) fn set_picker_delete_transition_blocked_status(&mut self) {
        self.set_status(PICKER_DELETE_TRANSITION_BLOCKED_STATUS);
    }

    /// Clears a transition when `id` is its successfully loaded or failed replacement paste.
    pub(super) fn clear_picker_delete_transition_for_replacement(&mut self, id: &str) {
        let matches = self
            .picker_delete_transition
            .as_ref()
            .is_some_and(|transition| transition.replacement_id.as_deref() == Some(id));
        if matches {
            self.picker_delete_transition = None;
        }
    }

    /// Clears a transition when `id` is the delete request that reached a terminal outcome.
    pub(super) fn clear_picker_delete_transition_for_deleted(&mut self, id: &str) {
        let matches = self
            .picker_delete_transition
            .as_ref()
            .is_some_and(|transition| transition.deleted_id == id);
        if matches {
            self.picker_delete_transition = None;
        }
    }

    /// Clears any selected-picker delete that can no longer receive backend replies.
    pub(super) fn clear_picker_delete_transition(&mut self) {
        self.picker_delete_transition = None;
    }

    /// Whether `id` is the paste whose selected-picker delete is awaiting acknowledgement.
    ///
    /// # Returns
    /// `true` when the active transition belongs to this deleted paste.
    pub(super) fn picker_delete_matches_deleted(&self, id: &str) -> bool {
        self.picker_delete_transition
            .as_ref()
            .is_some_and(|transition| transition.deleted_id == id)
    }

    /// Selects the adjacent paste without releasing picker ownership until its load settles.
    ///
    /// # Arguments
    /// - `transition`: Matching delete transition removed while selection is dispatched.
    /// - `replacement_id`: Adjacent paste to load before releasing ownership.
    fn select_picker_delete_replacement(
        &mut self,
        mut transition: PickerDeleteTransition,
        replacement_id: String,
    ) {
        transition.replacement_id = Some(replacement_id.clone());
        if self.select_paste(replacement_id) {
            self.picker_delete_transition = Some(transition);
        }
    }

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
        self.dispatch_delete_paste(id)
    }

    /// Dispatches a delete whose user intent and prerequisite saves were already accepted.
    fn dispatch_delete_paste(&mut self, id: String) -> bool {
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
        self.pending_delete_id = Some(id.clone());
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
            self.clear_picker_delete_transition_for_deleted(id.as_str());
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
            self.clear_picker_delete_transition_for_deleted(paste_id.as_str());
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
        if !self.dispatch_delete_paste(paste_id.clone()) {
            self.clear_picker_delete_transition_for_deleted(paste_id.as_str());
        }
    }
}
