//! Version-history and diff modal state/helpers for the editor panel.

use super::ui::diff_modal::{
    inline_diff_preview_from_response, InlineDiffPreview, MAX_INLINE_DIFF_BYTES,
};
use super::{
    non_focusable_click_sense, EditorLineIndex, LocalPasteApp, SaveStatus, SEARCH_DEBOUNCE,
};
use crate::backend::{
    CoreCmd, CoreErrorSource, CoreEvent, PasteSummary, VERSION_WORKFLOW_LIST_LIMIT,
};
use chrono::{DateTime, Utc};
use eframe::egui;
use localpaste_core::models::paste::{Paste, VersionMeta, VersionSnapshot};
use std::time::Instant;

const MAX_DIFF_CANDIDATES: usize = 40;
const RESET_TRANSITION_BLOCKED_STATUS: &str = "Reset in progress; editor is temporarily read-only.";
const VERSION_OVERLAY_MUTATION_BLOCKED_STATUS: &str =
    "Close the open version window before mutating the selected paste.";
const VERSION_OVERLAY_SELECTION_BLOCKED_STATUS: &str =
    "Close the open version window before changing selection.";

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveSnapshotCacheKey {
    paste_id: String,
    buffer_epoch: u64,
    revision: u64,
    text_len: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HistoryPreviewCacheKey {
    paste_id: String,
    version_id_ms: u64,
    len: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DiffPreviewCacheKey {
    lhs: ActiveSnapshotCacheKey,
    rhs_paste_id: String,
    rhs_updated_at: DateTime<Utc>,
    rhs_content_len: usize,
}

/// UI state for detached diff/history modals.
#[derive(Debug, Clone, Default)]
pub(crate) struct VersionUiState {
    pub(super) history_modal_open: bool,
    pub(super) history_versions: Vec<VersionMeta>,
    pub(super) history_selected_index: usize, // 0 = current working copy
    pub(super) history_snapshot: Option<VersionSnapshot>,
    pub(super) history_loading_snapshot_id: Option<u64>,
    pub(super) history_reset_confirm_open: bool,
    pub(super) history_reset_confirm_target: Option<u64>,
    pub(super) history_reset_in_flight_paste_id: Option<String>,
    active_snapshot_cache_key: Option<ActiveSnapshotCacheKey>,
    pub(super) active_snapshot_cache_text: String,
    pub(super) active_snapshot_preview_lines: EditorLineIndex,
    history_preview_cache_key: Option<HistoryPreviewCacheKey>,
    pub(super) history_preview_text: String,
    pub(super) history_preview_lines: EditorLineIndex,
    pub(super) diff_modal_open: bool,
    pub(super) diff_query: String,
    pub(super) diff_target_id: Option<String>,
    pub(super) diff_target_paste: Option<Paste>,
    pub(super) diff_loading_target: bool,
    diff_preview_cache_key: Option<DiffPreviewCacheKey>,
    diff_preview_pending_request_id: Option<u64>,
    diff_preview_request_seq: u64,
    pub(super) diff_preview: Option<InlineDiffPreview>,
}

impl VersionUiState {
    /// Clears any open reset-confirm dialog state and captured target version id.
    pub(super) fn clear_history_reset_confirm(&mut self) {
        self.history_reset_confirm_open = false;
        self.history_reset_confirm_target = None;
    }

    fn clear_active_snapshot_cache(&mut self) {
        self.active_snapshot_cache_key = None;
        self.active_snapshot_cache_text.clear();
        self.active_snapshot_preview_lines.reset();
    }

    fn clear_history_preview_cache(&mut self) {
        self.history_preview_cache_key = None;
        self.history_preview_text.clear();
        self.history_preview_lines.reset();
    }

    fn clear_history_snapshot_state(&mut self) {
        self.history_snapshot = None;
        self.history_loading_snapshot_id = None;
        self.clear_history_preview_cache();
    }

    fn begin_history_reset_for(&mut self, paste_id: String) {
        self.history_reset_in_flight_paste_id = Some(paste_id);
    }

    fn clear_history_reset_transition(&mut self) {
        self.history_reset_in_flight_paste_id = None;
    }

    fn history_reset_in_flight(&self) -> bool {
        self.history_reset_in_flight_paste_id.is_some()
    }

    fn history_reset_in_flight_for(&self, paste_id: Option<&str>) -> bool {
        paste_id.is_some() && self.history_reset_in_flight_paste_id.as_deref() == paste_id
    }

    /// Clears history-list and snapshot selection state.
    pub(super) fn clear_history_selection(&mut self) {
        self.history_versions.clear();
        self.history_selected_index = 0;
        self.clear_history_snapshot_state();
        self.clear_history_reset_confirm();
        // Preserve the reset fence here. Ordinary modal teardown and selection changes
        // must not drop a pending reset before its own backend ack/error arrives.
    }

    /// Clears only the loaded diff target and cached preview state.
    pub(super) fn clear_diff_target_state(&mut self) {
        self.diff_target_id = None;
        self.diff_target_paste = None;
        self.diff_loading_target = false;
        self.diff_preview_cache_key = None;
        self.diff_preview_pending_request_id = None;
        self.diff_preview = None;
    }

    /// Clears diff-target selection state.
    pub(super) fn clear_diff_selection(&mut self) {
        self.diff_query.clear();
        self.clear_diff_target_state();
    }

    fn clear_all(&mut self) {
        self.history_modal_open = false;
        self.diff_modal_open = false;
        self.clear_history_selection();
        self.clear_diff_selection();
        self.clear_active_snapshot_cache();
    }

    fn next_diff_preview_request_id(&mut self) -> u64 {
        self.diff_preview_request_seq = self.diff_preview_request_seq.wrapping_add(1);
        self.diff_preview_request_seq
    }
}

impl LocalPasteApp {
    /// Advances the active-buffer identity token after a full buffer replacement.
    ///
    /// # Notes
    /// Editor revisions cover incremental edits, but selection changes and load
    /// acks can replace the entire buffer while resetting revision counters.
    pub(super) fn bump_active_buffer_epoch(&mut self) {
        self.active_buffer_epoch = self.active_buffer_epoch.wrapping_add(1);
        self.version_ui.clear_active_snapshot_cache();
    }

    fn active_snapshot_cache_key(&self) -> Option<ActiveSnapshotCacheKey> {
        Some(ActiveSnapshotCacheKey {
            paste_id: self.selected_id.clone()?,
            buffer_epoch: self.active_buffer_epoch,
            revision: self.active_revision(),
            text_len: self.active_text_len_bytes(),
        })
    }

    fn ensure_selected_paste_for_version_modal(&mut self) -> bool {
        if self.selected_id.is_some() {
            return true;
        }
        self.set_status("Nothing selected.");
        false
    }

    fn begin_version_overlay(&mut self) -> bool {
        if !self.ensure_selected_paste_for_version_modal() {
            return false;
        }
        self.clear_pending_selection_request();
        true
    }

    /// Clears all version/diff modal state.
    pub(super) fn clear_version_view_state(&mut self) {
        self.version_ui.clear_all();
    }

    /// Returns metadata for the currently selected historical entry.
    ///
    /// # Returns
    /// `Some(meta)` when a stored snapshot row is selected, otherwise `None`.
    pub(super) fn selected_history_meta(&self) -> Option<&VersionMeta> {
        let index = self.version_ui.history_selected_index;
        if index == 0 {
            return None;
        }
        self.version_ui
            .history_versions
            .get(index.saturating_sub(1))
    }

    /// Updates the selected history row and triggers snapshot load when needed.
    pub(super) fn set_history_selected_index(&mut self, index: usize) {
        let max_index = self.version_ui.history_versions.len();
        let next_index = index.min(max_index);
        self.version_ui.history_selected_index = next_index;
        self.version_ui.clear_history_snapshot_state();
        if let Some(meta) = self.selected_history_meta() {
            self.request_version_snapshot(meta.version_id_ms);
        }
    }

    /// Opens the history modal for the currently selected paste.
    pub(super) fn open_history_modal(&mut self) {
        if !self.begin_version_overlay() {
            return;
        }
        self.version_ui.history_modal_open = true;
        self.version_ui.clear_history_selection();
        self.request_versions_for_selected();
    }

    /// Opens the diff modal for the currently selected paste.
    pub(super) fn open_diff_modal(&mut self) {
        if !self.begin_version_overlay() {
            return;
        }
        self.version_ui.diff_modal_open = true;
        self.version_ui.clear_diff_selection();
    }

    /// Requests version metadata rows for the currently selected paste.
    pub(super) fn request_versions_for_selected(&mut self) {
        let Some(id) = self.selected_id.clone() else {
            self.version_ui.clear_history_selection();
            return;
        };
        if self
            .backend
            .cmd_tx
            .send(CoreCmd::ListPasteVersions {
                id,
                limit: VERSION_WORKFLOW_LIST_LIMIT,
            })
            .is_err()
        {
            self.set_status("List versions failed: backend unavailable.");
        }
    }

    fn request_version_snapshot(&mut self, version_id_ms: u64) {
        let Some(id) = self.selected_id.clone() else {
            return;
        };
        self.version_ui.history_loading_snapshot_id = Some(version_id_ms);
        if self
            .backend
            .cmd_tx
            .send(CoreCmd::GetPasteVersion { id, version_id_ms })
            .is_err()
        {
            self.version_ui.clear_history_snapshot_state();
            self.set_status("Load version failed: backend unavailable.");
        }
    }

    /// Refreshes the cached current editor snapshot only when the active buffer identity changes.
    ///
    /// # Returns
    /// `true` when a fresh owned snapshot had to be cloned from the editor buffer.
    pub(super) fn sync_active_snapshot_cache(&mut self) -> bool {
        let Some(cache_key) = self.active_snapshot_cache_key() else {
            self.version_ui.clear_active_snapshot_cache();
            return false;
        };
        if self.version_ui.active_snapshot_cache_key.as_ref() == Some(&cache_key) {
            return false;
        }
        self.version_ui.active_snapshot_cache_text = self.active_snapshot();
        self.version_ui.active_snapshot_preview_lines.rebuild(
            cache_key.revision,
            self.version_ui.active_snapshot_cache_text.as_str(),
        );
        self.version_ui.active_snapshot_cache_key = Some(cache_key);
        true
    }

    /// Refreshes the cached read-only history preview body for the selected stored snapshot.
    ///
    /// # Returns
    /// `true` when the snapshot body had to be cloned into preview storage.
    pub(super) fn sync_history_preview_cache(&mut self) -> bool {
        let Some(snapshot) = self.version_ui.history_snapshot.as_ref() else {
            self.version_ui.clear_history_preview_cache();
            return false;
        };
        let cache_key = HistoryPreviewCacheKey {
            paste_id: snapshot.paste_id.clone(),
            version_id_ms: snapshot.version_id_ms,
            len: snapshot.len,
        };
        if self.version_ui.history_preview_cache_key.as_ref() == Some(&cache_key) {
            return false;
        }
        self.version_ui.history_preview_text = snapshot.content.clone();
        self.version_ui.history_preview_lines.rebuild(
            snapshot.version_id_ms,
            self.version_ui.history_preview_text.as_str(),
        );
        self.version_ui.history_preview_cache_key = Some(cache_key);
        true
    }

    /// Refreshes detached diff preview state when the left or right side changes.
    ///
    /// At most one worker diff request stays in flight. While a request is pending,
    /// newer editor revisions wait for that result to drain and then enqueue the
    /// freshest preview on the next repaint.
    ///
    /// # Returns
    /// `true` when preview state changed or a fresh worker request was queued.
    pub(super) fn sync_diff_preview_cache(&mut self) -> bool {
        let Some(lhs_cache_key) = self.active_snapshot_cache_key() else {
            self.version_ui.clear_active_snapshot_cache();
            self.version_ui.diff_preview_cache_key = None;
            self.version_ui.diff_preview_pending_request_id = None;
            self.version_ui.diff_preview = None;
            return false;
        };
        let Some(cache_key) =
            self.version_ui
                .diff_target_paste
                .as_ref()
                .map(|rhs| DiffPreviewCacheKey {
                    lhs: lhs_cache_key,
                    rhs_paste_id: rhs.id.clone(),
                    rhs_updated_at: rhs.updated_at,
                    rhs_content_len: rhs.content.len(),
                })
        else {
            self.version_ui.diff_preview_cache_key = None;
            self.version_ui.diff_preview_pending_request_id = None;
            self.version_ui.diff_preview = None;
            return false;
        };
        if self.version_ui.diff_preview_cache_key.as_ref() == Some(&cache_key) {
            return false;
        }

        let lhs_bytes = self.active_text_len_bytes();
        let rhs_bytes = cache_key.rhs_content_len;
        if lhs_bytes.saturating_add(rhs_bytes) > MAX_INLINE_DIFF_BYTES {
            self.version_ui.diff_preview = Some(InlineDiffPreview::TooLarge {
                lhs_bytes,
                rhs_bytes,
            });
            self.version_ui.diff_preview_cache_key = Some(cache_key);
            self.version_ui.diff_preview_pending_request_id = None;
            return true;
        }

        if self.version_ui.diff_preview_pending_request_id.is_some() {
            return false;
        }

        let _recomputed_snapshot = self.sync_active_snapshot_cache();
        let Some(right_text) = self
            .version_ui
            .diff_target_paste
            .as_ref()
            .map(|rhs| rhs.content.clone())
        else {
            self.version_ui.diff_preview_cache_key = None;
            self.version_ui.diff_preview_pending_request_id = None;
            self.version_ui.diff_preview = None;
            return false;
        };
        let request_id = self.version_ui.next_diff_preview_request_id();
        if self
            .backend
            .cmd_tx
            .send(CoreCmd::ComputeDiffPreview {
                request_id,
                left_text: self.version_ui.active_snapshot_cache_text.clone(),
                right_text,
            })
            .is_err()
        {
            self.version_ui.diff_preview_cache_key = None;
            self.version_ui.diff_preview_pending_request_id = None;
            self.version_ui.diff_preview = None;
            self.set_status("Diff preview failed: backend unavailable.");
            return false;
        }
        self.version_ui.diff_preview_cache_key = Some(cache_key);
        self.version_ui.diff_preview_pending_request_id = Some(request_id);
        self.version_ui.diff_preview = None;
        true
    }

    /// Requests backend duplication for the selected historical snapshot.
    pub(super) fn duplicate_selected_history_version(&mut self) {
        let Some(id) = self.selected_id.clone() else {
            return;
        };
        let Some(meta) = self.selected_history_meta() else {
            return;
        };
        if self
            .backend
            .cmd_tx
            .send(CoreCmd::DuplicatePasteVersion {
                id,
                version_id_ms: meta.version_id_ms,
                name: None,
            })
            .is_err()
        {
            self.set_status("Duplicate version failed: backend unavailable.");
            return;
        }
        self.set_status("Duplicating historical version...");
    }

    /// Reports whether hard reset can run without racing unsaved or in-flight local saves.
    /// # Returns
    /// `true` when content and metadata state are both clean and no reset is in flight.
    pub(super) fn can_queue_history_reset(&self) -> bool {
        self.history_reset_queue_block_reason().is_none()
    }

    /// Returns whether a confirmed history reset is still awaiting its authoritative backend ack.
    ///
    /// # Returns
    /// `true` while the currently selected paste is still awaiting its own reset ack.
    pub(super) fn reset_transition_active(&self) -> bool {
        self.version_ui
            .history_reset_in_flight_for(self.selected_id.as_deref())
    }

    /// Returns whether `paste_id` is still awaiting a reset ack/error.
    ///
    /// # Returns
    /// `true` when reset is pending for that specific paste id.
    pub(super) fn history_reset_pending_for(&self, paste_id: &str) -> bool {
        self.version_ui.history_reset_in_flight_for(Some(paste_id))
    }

    /// Reports why persisting the current dirty draft should be blocked right now.
    ///
    /// Detached version windows stay read-only, but they must not strand existing
    /// dirty state. Only an authoritative reset transition fences save dispatch.
    ///
    /// # Returns
    /// `Some(reason)` only while a queued reset is still authoritative for the
    /// currently selected paste.
    pub(super) fn save_block_reason(&self) -> Option<&'static str> {
        if self.reset_transition_active() {
            Some(RESET_TRANSITION_BLOCKED_STATUS)
        } else {
            None
        }
    }

    /// Reports why selection transitions should be blocked right now.
    ///
    /// Detached version workflows own the current paste while they are open.
    /// Selection must not move underneath them, and queued reset transitions are
    /// stricter still because they also preserve the current paste lock.
    ///
    /// # Returns
    /// `Some(reason)` when selection must stay pinned to the current paste for
    /// an open version workflow or in-flight hard reset.
    pub(super) fn selection_transition_block_reason(&self) -> Option<&'static str> {
        if self.reset_transition_active() {
            Some(RESET_TRANSITION_BLOCKED_STATUS)
        } else if self.version_overlay_open() {
            Some(VERSION_OVERLAY_SELECTION_BLOCKED_STATUS)
        } else {
            None
        }
    }

    /// Reports why a background mutating action should be blocked right now.
    ///
    /// Detached version windows block destructive workflow changes like create,
    /// delete, reset, and selection-changing shortcuts, but do not block save
    /// dispatch for already-dirty editor state.
    ///
    /// # Returns
    /// `Some(reason)` when destructive workflow changes should be fenced by a
    /// queued reset or an open detached version window.
    pub(super) fn mutation_shortcut_block_reason(&self) -> Option<&'static str> {
        if let Some(reason) = self.save_block_reason() {
            Some(reason)
        } else if self.version_overlay_open() {
            Some(VERSION_OVERLAY_MUTATION_BLOCKED_STATUS)
        } else {
            None
        }
    }

    /// Reports why a new hard reset cannot be queued right now.
    ///
    /// # Returns
    /// `Some(reason)` when another reset is pending or local save state is not clean.
    pub(super) fn history_reset_queue_block_reason(&self) -> Option<&'static str> {
        if self.version_ui.history_reset_in_flight() {
            return Some("Reset is unavailable while another history reset is still in progress.");
        }
        if self.save_status != SaveStatus::Saved
            || self.save_in_flight
            || self.metadata_dirty
            || self.metadata_save_in_flight
        {
            return Some("Reset is unavailable while local changes are unsaved or saving.");
        }
        None
    }

    /// Reports the shared read-only status used while reset temporarily fences mutations.
    pub(super) fn set_reset_transition_blocked_status(&mut self) {
        self.set_blocked_status(RESET_TRANSITION_BLOCKED_STATUS);
    }

    fn set_blocked_status(&mut self, reason: &'static str) {
        if self.status.as_ref().map(|status| status.text.as_str()) != Some(reason) {
            self.set_status(reason);
        }
    }

    /// Reports the shared status used when save dispatch is fenced.
    pub(super) fn set_save_blocked_status(&mut self) {
        if let Some(reason) = self.save_block_reason() {
            self.set_blocked_status(reason);
        }
    }

    /// Reports the shared status used when detached version windows fence background mutations.
    pub(super) fn set_mutation_shortcut_blocked_status(&mut self) {
        let Some(reason) = self.mutation_shortcut_block_reason() else {
            return;
        };
        self.set_blocked_status(reason);
    }

    /// Reports the shared status used when a version workflow pins selection.
    pub(super) fn set_selection_transition_blocked_status(&mut self) {
        let Some(reason) = self.selection_transition_block_reason() else {
            return;
        };
        self.set_blocked_status(reason);
    }

    /// Captures the currently selected history row as the immutable reset-confirm target.
    pub(super) fn open_history_reset_confirm(&mut self) {
        // Destructive confirmation must bind to immutable intent, not live selection.
        let target = self.selected_history_meta().map(|meta| meta.version_id_ms);
        self.version_ui.history_reset_confirm_target = target;
        self.version_ui.history_reset_confirm_open = target.is_some();
    }

    /// Requests backend reset to a specific confirmed historical snapshot.
    fn reset_history_version_by_id(&mut self, version_id_ms: u64) {
        if let Some(reason) = self.history_reset_queue_block_reason() {
            self.set_status(reason);
            return;
        }
        let Some(id) = self.selected_id.clone() else {
            return;
        };
        if self
            .backend
            .cmd_tx
            .send(CoreCmd::ResetPasteHardToVersion {
                id: id.clone(),
                version_id_ms,
            })
            .is_err()
        {
            self.set_status("Reset hard failed: backend unavailable.");
            return;
        }
        self.version_ui.begin_history_reset_for(id);
        self.version_ui.clear_history_reset_confirm();
        self.set_status("Resetting paste to selected version...");
    }

    /// Requests backend reset to the confirmed historical snapshot.
    pub(super) fn reset_selected_history_version(&mut self) {
        let Some(version_id_ms) = self.version_ui.history_reset_confirm_target else {
            return;
        };
        self.reset_history_version_by_id(version_id_ms);
    }

    /// Side-loads a paste for diff comparison without changing active selection.
    pub(super) fn request_diff_target(&mut self, id: String) {
        self.version_ui.diff_target_id = Some(id.clone());
        self.version_ui.diff_target_paste = None;
        self.version_ui.diff_loading_target = true;
        self.version_ui.diff_preview_cache_key = None;
        self.version_ui.diff_preview_pending_request_id = None;
        self.version_ui.diff_preview = None;
        if self
            .backend
            .cmd_tx
            .send(CoreCmd::GetDiffTargetPaste { id })
            .is_err()
        {
            self.version_ui.clear_diff_target_state();
            self.set_status("Diff load failed: backend unavailable.");
        }
    }

    fn is_active_history_snapshot_request(&self, paste_id: &str, version_id_ms: u64) -> bool {
        self.selected_id.as_deref() == Some(paste_id)
            && self.version_ui.history_loading_snapshot_id == Some(version_id_ms)
    }

    fn is_active_diff_target_request(&self, paste_id: &str) -> bool {
        self.version_ui.diff_target_id.as_deref() == Some(paste_id)
    }

    fn maybe_capture_diff_target_from_loaded_paste(&mut self, paste: &Paste) {
        if self.version_ui.diff_target_id.as_deref() == Some(paste.id.as_str()) {
            self.version_ui.diff_target_paste = Some(paste.clone());
            self.version_ui.diff_loading_target = false;
            self.version_ui.diff_preview_cache_key = None;
            self.version_ui.diff_preview_pending_request_id = None;
            self.version_ui.diff_preview = None;
        }
    }

    /// Returns filtered paste candidates for diff target picking.
    ///
    /// # Returns
    /// Up to 40 matching paste summaries excluding the currently selected paste.
    pub(super) fn diff_candidates(&self) -> Vec<PasteSummary> {
        let query = self.version_ui.diff_query.trim().to_ascii_lowercase();
        self.all_pastes
            .iter()
            .filter(|item| Some(item.id.as_str()) != self.selected_id.as_deref())
            .filter(|item| {
                if query.is_empty() {
                    return true;
                }
                item.name.to_ascii_lowercase().contains(query.as_str())
                    || item.id.to_ascii_lowercase().contains(query.as_str())
                    || item
                        .language
                        .as_deref()
                        .unwrap_or_default()
                        .to_ascii_lowercase()
                        .contains(query.as_str())
                    || item
                        .tags
                        .iter()
                        .any(|tag| tag.to_ascii_lowercase().contains(query.as_str()))
            })
            .take(MAX_DIFF_CANDIDATES)
            .cloned()
            .collect()
    }

    /// Applies version/diff backend events to detached modal state.
    pub(super) fn on_version_event(&mut self, event: &CoreEvent) {
        match event {
            CoreEvent::DiffTargetLoaded { paste } => {
                self.maybe_capture_diff_target_from_loaded_paste(paste);
            }
            CoreEvent::DiffPreviewComputed { request_id, diff } => {
                if self.version_ui.diff_preview_pending_request_id != Some(*request_id) {
                    return;
                }
                self.version_ui.diff_preview_pending_request_id = None;
                self.version_ui.diff_preview =
                    Some(inline_diff_preview_from_response(diff.clone()));
            }
            CoreEvent::PasteSaved { paste } => {
                if self.version_ui.history_modal_open
                    && self.selected_id.as_deref() == Some(paste.id.as_str())
                {
                    self.request_versions_for_selected();
                }
            }
            CoreEvent::PasteVersionsLoaded { id, items } => {
                if self.selected_id.as_deref() != Some(id.as_str()) {
                    return;
                }
                let selected_version_id =
                    self.selected_history_meta().map(|meta| meta.version_id_ms);
                self.version_ui.history_versions = items.clone();
                if let Some(target) = self.version_ui.history_reset_confirm_target {
                    let target_still_available = self
                        .version_ui
                        .history_versions
                        .iter()
                        .any(|meta| meta.version_id_ms == target);
                    if !target_still_available {
                        self.version_ui.clear_history_reset_confirm();
                    }
                }
                match selected_version_id {
                    Some(version_id_ms) => {
                        if let Some(index) = self
                            .version_ui
                            .history_versions
                            .iter()
                            .position(|meta| meta.version_id_ms == version_id_ms)
                        {
                            self.version_ui.history_selected_index = index.saturating_add(1);
                            // A refresh should restore the selected historical snapshot after a
                            // prior load failure, not leave the modal on a dead row.
                            if self.version_ui.history_snapshot.is_none()
                                && self.version_ui.history_loading_snapshot_id.is_none()
                            {
                                self.request_version_snapshot(version_id_ms);
                            }
                        } else {
                            self.version_ui.history_selected_index = 0;
                            self.version_ui.clear_history_snapshot_state();
                        }
                    }
                    None => {
                        self.version_ui.history_selected_index = 0;
                        self.version_ui.clear_history_snapshot_state();
                    }
                }
            }
            CoreEvent::PasteVersionLoaded { snapshot } => {
                if self.selected_id.as_deref() != Some(snapshot.paste_id.as_str()) {
                    return;
                }
                let Some(meta) = self.selected_history_meta() else {
                    return;
                };
                if snapshot.version_id_ms != meta.version_id_ms {
                    return;
                }
                self.version_ui.history_snapshot = Some(snapshot.clone());
                self.version_ui.history_loading_snapshot_id = None;
            }
            CoreEvent::PasteVersionLoadFailed {
                paste_id,
                version_id_ms,
                message,
            } => {
                if !self.is_active_history_snapshot_request(paste_id.as_str(), *version_id_ms) {
                    return;
                }
                self.version_ui.clear_history_snapshot_state();
                self.set_status(message.clone());
            }
            CoreEvent::PasteResetToVersion { paste } => {
                let paste_id = paste.id.clone();
                if self.history_reset_pending_for(paste_id.as_str()) {
                    self.version_ui.clear_history_reset_transition();
                }
                self.upsert_cached_paste_summary(paste);
                if !self.search_query.trim().is_empty() {
                    // Reset can change search inclusion/ranking (content/language/updated_at),
                    // so force a fresh backend search even when query text is unchanged.
                    self.search_last_sent.clear();
                    self.search_last_input_at = Some(Instant::now() - SEARCH_DEBOUNCE);
                } else {
                    // Reset mutates metadata that drives smart collections and language
                    // filters, so the visible sidebar projection must be recomputed
                    // immediately even when no text search is active.
                    self.recompute_visible_pastes();
                }
                if self.selected_id.as_deref() == Some(paste_id.as_str()) {
                    // Reset is authoritative: replace any local unsaved/editor state
                    // with the canonical backend row that reset produced.
                    self.select_loaded_paste(paste.clone());
                    if self.search_query.trim().is_empty() {
                        self.ensure_selection_after_list_update();
                    }
                    self.version_ui.history_modal_open = false;
                    self.version_ui.history_selected_index = 0;
                    self.version_ui.clear_history_snapshot_state();
                    self.set_status("Reset current paste to selected historical snapshot.");
                }
            }
            CoreEvent::DiffTargetLoadFailed { id, message } => {
                if !self.is_active_diff_target_request(id.as_str()) {
                    return;
                }
                self.version_ui.clear_diff_target_state();
                self.set_status(message.clone());
            }
            CoreEvent::PasteMissing { id } => {
                if self.history_reset_pending_for(id.as_str()) {
                    self.version_ui.clear_history_reset_transition();
                }
                if self.is_active_diff_target_request(id.as_str()) {
                    self.version_ui.clear_diff_target_state();
                }
            }
            CoreEvent::Error { source, .. } => {
                // Version-preview cleanup is driven by version-specific failure events.
                // Generic backend errors (save/search/list) must not tear down an
                // unrelated history preview that is still waiting on its own reply.
                if matches!(source, CoreErrorSource::SaveContent)
                    && self.version_ui.history_reset_in_flight()
                    && !self.save_in_flight
                {
                    self.version_ui.clear_history_reset_transition();
                }
            }
            _ => {}
        }
    }

    /// Renders history/diff entry points in the editor toolbar.
    ///
    /// # Arguments
    /// - `ui`: Toolbar UI row receiving the version-action buttons.
    /// - `editor_had_virtual_focus`: Whether the virtual editor owned keyboard
    ///   focus before the toolbar action was triggered this frame.
    ///
    /// # Returns
    /// Returns whether the virtual editor should keep keyboard focus after this
    /// frame because a mouse-first toolbar action fired.
    pub(super) fn render_version_toolbar(
        &mut self,
        ui: &mut egui::Ui,
        editor_had_virtual_focus: bool,
    ) -> bool {
        let mut preserve_virtual_editor_focus = false;
        ui.separator();
        if ui
            .add(
                egui::Button::new("Diff")
                    .small()
                    .sense(non_focusable_click_sense()),
            )
            .clicked()
        {
            self.open_diff_modal();
            preserve_virtual_editor_focus |= editor_had_virtual_focus;
        }
        if ui
            .add(
                egui::Button::new("History")
                    .small()
                    .sense(non_focusable_click_sense()),
            )
            .clicked()
        {
            self.open_history_modal();
            preserve_virtual_editor_focus |= editor_had_virtual_focus;
        }
        preserve_virtual_editor_focus
    }

    /// Renders detached history and diff modal dialogs.
    pub(super) fn render_version_dialogs(&mut self, ctx: &egui::Context) {
        self.render_history_modal(ctx);
        self.render_diff_modal(ctx);
    }
}
