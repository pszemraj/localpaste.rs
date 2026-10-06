//! Sidebar projection, navigation, and selection retention.

use super::*;

impl LocalPasteApp {
    /// Returns the visible-list index of the selected paste, if visible.
    /// # Returns
    /// `Some(index)` when the selected paste is visible, otherwise `None`.
    pub(in crate::app) fn selected_index(&self) -> Option<usize> {
        let id = self.selected_id.as_ref()?;
        self.pastes.iter().position(|paste| paste.id == *id)
    }

    /// Returns the sidebar paste id targeted by an arrow-key move, if any.
    ///
    /// # Arguments
    /// - `direction`: Signed navigation delta relative to the current selection.
    ///
    /// # Returns
    /// `Some(id)` when the arrow move should select another visible paste, otherwise `None`.
    pub(in crate::app) fn sidebar_arrow_target_id(&self, direction: i32) -> Option<String> {
        if direction == 0 {
            return None;
        }
        let current = self.selected_index().unwrap_or(0) as i32;
        let max_index = self.pastes.len().checked_sub(1)? as i32;
        let next = (current + direction).clamp(0, max_index) as usize;
        if self.selected_index() == Some(next) {
            return None;
        }
        self.pastes.get(next).map(|paste| paste.id.clone())
    }

    /// Builds the active metadata filters sent with a sidebar search.
    ///
    /// # Returns
    /// Unrestricted folder and the canonical active language, if selected.
    pub(super) fn search_backend_filters(&self) -> (Option<String>, Option<String>) {
        (None, self.active_language_filter.clone())
    }

    /// Filters sidebar summaries through the active collection/language state.
    /// # Returns
    /// Visible sidebar rows preserving the input ordering of `items`.
    pub(in crate::app) fn filter_by_collection(&self, items: &[PasteSummary]) -> Vec<PasteSummary> {
        let (today_local, week_cutoff_day, recent_cutoff) =
            crate::backend::collections::current_filter_cutoffs();
        let active_language_filter = self.active_language_filter.as_deref();
        items
            .iter()
            .filter(|item| {
                matches_active_filters(
                    item,
                    &self.active_collection,
                    active_language_filter,
                    today_local,
                    week_cutoff_day,
                    recent_cutoff,
                )
            })
            .cloned()
            .collect()
    }

    /// Drops cached result rows that no longer match collection or language filters.
    pub(super) fn retain_search_results_for_active_filters(&mut self) {
        let (today_local, week_cutoff_day, recent_cutoff) =
            crate::backend::collections::current_filter_cutoffs();
        let active_collection = self.active_collection.clone();
        let active_language_filter = self.active_language_filter.clone();
        self.pastes.retain(|item| {
            matches_active_filters(
                item,
                &active_collection,
                active_language_filter.as_deref(),
                today_local,
                week_cutoff_day,
                recent_cutoff,
            )
        });
    }

    /// Ensures the current selection still exists in the visible sidebar list.
    ///
    /// When the active item no longer matches the current filters, this selects
    /// the first remaining visible paste or clears a fully saved selection if
    /// none remain. Drafts and outstanding saves retain their editor and lock.
    pub(in crate::app) fn ensure_selection_after_list_update(&mut self) {
        if self.selection_transition_block_reason().is_some() {
            return;
        }
        if let Some(pending) = self.pending_selection_id.clone() {
            if self.pastes.iter().any(|paste| paste.id == pending) {
                self.try_apply_pending_selection();
                if self.selected_id.as_deref() == Some(pending.as_str())
                    || self.pending_selection_id.is_none()
                {
                    return;
                }
            } else if self.selected_id.is_none() {
                self.pending_selection_id = None;
                let _ = self.apply_selection_now(pending);
                return;
            }
        }
        let selection_valid = self
            .selected_id
            .as_ref()
            .map(|id| self.pastes.iter().any(|p| p.id == *id))
            .unwrap_or(false);
        if selection_valid {
            return;
        }
        let picker_selection_remains_valid =
            self.picker_selection_pin.as_deref().is_some_and(|id| {
                self.selected_id.as_deref() == Some(id)
                    || self.pending_selection_id.as_deref() == Some(id)
            });
        if picker_selection_remains_valid {
            return;
        }
        self.picker_selection_pin = None;
        if let Some(first) = self.pastes.first() {
            self.select_paste(first.id.clone());
        } else {
            if self.save_status != SaveStatus::Saved
                || self.metadata_dirty
                || self.save_in_flight
                || self.metadata_save_in_flight
            {
                return;
            }
            self.clear_selection();
        }
    }
}
