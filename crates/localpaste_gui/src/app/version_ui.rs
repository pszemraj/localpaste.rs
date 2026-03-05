//! Version-history and diff modal state/helpers for the editor panel.

use super::LocalPasteApp;
use crate::backend::{CoreCmd, CoreErrorSource, CoreEvent, PasteSummary};
use eframe::egui;
use localpaste_core::models::paste::{Paste, VersionMeta, VersionSnapshot};
use std::time::Instant;

const VERSION_UI_LIST_LIMIT: usize = 200;
const MAX_DIFF_CANDIDATES: usize = 40;

/// UI state for detached diff/history modals.
#[derive(Debug, Clone, Default)]
pub(crate) struct VersionUiState {
    pub(super) history_modal_open: bool,
    pub(super) history_versions: Vec<VersionMeta>,
    pub(super) history_selected_index: usize, // 0 = current working copy
    pub(super) history_snapshot: Option<VersionSnapshot>,
    pub(super) history_loading_snapshot_id: Option<u64>,
    pub(super) history_reset_confirm_open: bool,
    pub(super) history_reset_in_flight: bool,
    pub(super) diff_modal_open: bool,
    pub(super) diff_query: String,
    pub(super) diff_target_id: Option<String>,
    pub(super) diff_target_paste: Option<Paste>,
    pub(super) diff_loading_target: bool,
}

impl VersionUiState {
    fn clear_history_snapshot_state(&mut self) {
        self.history_snapshot = None;
        self.history_loading_snapshot_id = None;
    }

    /// Clears history-list and snapshot selection state.
    pub(super) fn clear_history_selection(&mut self) {
        self.history_versions.clear();
        self.history_selected_index = 0;
        self.clear_history_snapshot_state();
        self.history_reset_confirm_open = false;
        self.history_reset_in_flight = false;
    }

    fn clear_diff_target_state(&mut self) {
        self.diff_target_id = None;
        self.diff_target_paste = None;
        self.diff_loading_target = false;
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
    }
}

impl LocalPasteApp {
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
        if self.selected_id.is_none() {
            self.set_status("Nothing selected.");
            return;
        }
        self.version_ui.history_modal_open = true;
        self.version_ui.clear_history_selection();
        self.request_versions_for_selected();
    }

    /// Opens the diff modal for the currently selected paste.
    pub(super) fn open_diff_modal(&mut self) {
        if self.selected_id.is_none() {
            self.set_status("Nothing selected.");
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
                limit: VERSION_UI_LIST_LIMIT,
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

    /// Requests backend reset to the selected historical snapshot.
    pub(super) fn reset_selected_history_version(&mut self) {
        let Some(id) = self.selected_id.clone() else {
            return;
        };
        let Some(meta) = self.selected_history_meta() else {
            return;
        };
        if self
            .backend
            .cmd_tx
            .send(CoreCmd::ResetPasteHardToVersion {
                id,
                version_id_ms: meta.version_id_ms,
            })
            .is_err()
        {
            self.set_status("Reset hard failed: backend unavailable.");
            return;
        }
        self.version_ui.history_reset_in_flight = true;
        self.version_ui.history_reset_confirm_open = false;
        self.set_status("Resetting paste to selected version...");
    }

    /// Side-loads a paste for diff comparison without changing active selection.
    pub(super) fn request_diff_target(&mut self, id: String) {
        self.version_ui.diff_target_id = Some(id.clone());
        self.version_ui.diff_target_paste = None;
        self.version_ui.diff_loading_target = true;
        if self.backend.cmd_tx.send(CoreCmd::GetPaste { id }).is_err() {
            self.version_ui.clear_diff_target_state();
            self.set_status("Diff load failed: backend unavailable.");
        }
    }

    fn maybe_capture_diff_target_from_loaded_paste(&mut self, paste: &Paste) {
        if self.version_ui.diff_target_id.as_deref() == Some(paste.id.as_str()) {
            self.version_ui.diff_target_paste = Some(paste.clone());
            self.version_ui.diff_loading_target = false;
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
            CoreEvent::PasteLoaded { paste } => {
                self.maybe_capture_diff_target_from_loaded_paste(paste);
            }
            CoreEvent::PasteVersionsLoaded { id, items } => {
                if self.selected_id.as_deref() != Some(id.as_str()) {
                    return;
                }
                let selected_version_id =
                    self.selected_history_meta().map(|meta| meta.version_id_ms);
                self.version_ui.history_versions = items.clone();
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
            CoreEvent::PasteResetToVersion { paste } => {
                let paste_id = paste.id.clone();
                self.version_ui.history_reset_in_flight = false;
                if let Some(item) = self.all_pastes.iter_mut().find(|item| item.id == paste_id) {
                    *item = PasteSummary::from_paste(paste);
                }
                if let Some(item) = self.pastes.iter_mut().find(|item| item.id == paste_id) {
                    *item = PasteSummary::from_paste(paste);
                }
                if !self.search_query.trim().is_empty() {
                    // Reset can change search inclusion/ranking (content/language/updated_at),
                    // so force a fresh backend search even when query text is unchanged.
                    self.search_last_sent.clear();
                    self.search_last_input_at = Some(Instant::now() - super::SEARCH_DEBOUNCE);
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
            CoreEvent::PasteLoadFailed { id, .. } => {
                if self.version_ui.diff_target_id.as_deref() == Some(id.as_str()) {
                    self.version_ui.clear_diff_target_state();
                }
            }
            CoreEvent::PasteMissing { id } => {
                if self.version_ui.diff_target_id.as_deref() == Some(id.as_str()) {
                    self.version_ui.clear_diff_target_state();
                }
            }
            CoreEvent::Error { source, .. } => {
                // Detached history/diff modals must never leave spinners or disabled
                // actions stuck after backend failures.
                if self.version_ui.history_loading_snapshot_id.is_some() {
                    self.version_ui.clear_history_snapshot_state();
                }
                if matches!(source, CoreErrorSource::SaveContent)
                    && self.version_ui.history_reset_in_flight
                {
                    self.version_ui.history_reset_in_flight = false;
                }
            }
            _ => {}
        }
    }

    /// Renders history/diff entry points in the editor toolbar.
    pub(super) fn render_version_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        if ui.small_button("Diff").clicked() {
            self.open_diff_modal();
        }
        if ui.small_button("History").clicked() {
            self.open_history_modal();
        }
    }

    /// Renders detached history and diff modal dialogs.
    pub(super) fn render_version_dialogs(&mut self, ctx: &egui::Context) {
        self.render_history_modal(ctx);
        self.render_diff_modal(ctx);
    }
}
