//! Version-history and diff UI state/helpers for the editor panel.

use super::{LocalPasteApp, SaveStatus};
use crate::backend::{CoreCmd, CoreEvent};
use eframe::egui::{self, RichText};
use localpaste_core::diff::DiffResponse;
use localpaste_core::models::paste::{VersionMeta, VersionSnapshot};

const VERSION_UI_LIST_LIMIT: usize = 200;
const DIFF_MODAL_HEIGHT: f32 = 320.0;

/// UI state for version-history navigation and diff interactions.
#[derive(Debug, Clone, Default)]
pub(crate) struct VersionUiState {
    pub(super) versions: Vec<VersionMeta>,
    pub(super) active_version_id_ms: Option<u64>,
    pub(super) historical_snapshot: Option<VersionSnapshot>,
    pub(super) reset_confirm_open: bool,
    pub(super) diff_modal_open: bool,
    pub(super) diff_target_paste_id: String,
    pub(super) diff_target_version_id: String,
    pub(super) diff_result: Option<DiffResponse>,
}

impl VersionUiState {
    fn active_version_index(&self) -> Option<usize> {
        let active = self.active_version_id_ms?;
        self.versions
            .iter()
            .position(|item| item.version_id_ms == active)
    }
}

impl LocalPasteApp {
    /// Returns whether the editor currently displays a historical snapshot.
    ///
    /// # Returns
    /// `true` when a historical version snapshot is active.
    pub(super) fn is_viewing_historical_version(&self) -> bool {
        self.version_ui.historical_snapshot.is_some()
    }

    /// Emits a read-only status and returns `true` when history view is active.
    ///
    /// # Returns
    /// `true` when the caller should stop mutating editor state.
    pub(super) fn block_if_historical_read_only(&mut self) -> bool {
        if self.is_viewing_historical_version() {
            self.set_status("Historical version is read-only.");
            true
        } else {
            false
        }
    }

    /// Clears active historical version state and related transient flags.
    pub(super) fn clear_version_view_state(&mut self) {
        self.version_ui.active_version_id_ms = None;
        self.version_ui.historical_snapshot = None;
        self.version_ui.reset_confirm_open = false;
    }

    /// Requests version metadata rows for the currently selected paste.
    pub(super) fn request_versions_for_selected(&mut self) {
        let Some(id) = self.selected_id.clone() else {
            self.version_ui.versions.clear();
            self.clear_version_view_state();
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
        if self
            .backend
            .cmd_tx
            .send(CoreCmd::GetPasteVersion { id, version_id_ms })
            .is_err()
        {
            self.set_status("Load version failed: backend unavailable.");
        }
    }

    fn restore_head_content_from_selected(&mut self) {
        let Some(content) = self
            .selected_paste
            .as_ref()
            .map(|paste| paste.content.clone())
        else {
            return;
        };
        self.selected_content.reset(content.clone());
        self.reset_virtual_editor(content.as_str());
        self.save_status = SaveStatus::Saved;
        self.save_in_flight = false;
        self.save_request_revision = None;
        self.last_edit_at = None;
    }

    /// Returns the editor to the current (head) paste content.
    pub(super) fn return_to_head_version(&mut self) {
        self.clear_version_view_state();
        self.restore_head_content_from_selected();
        self.set_status("Viewing current paste.");
    }

    fn navigate_to_older_version(&mut self) {
        if self.version_ui.versions.is_empty() {
            self.request_versions_for_selected();
            self.set_status("Loading versions...");
            return;
        }
        let target = match self.version_ui.active_version_index() {
            Some(index) => index.saturating_add(1),
            None => 0,
        };
        if let Some(meta) = self.version_ui.versions.get(target) {
            self.request_version_snapshot(meta.version_id_ms);
        }
    }

    fn navigate_to_newer_version(&mut self) {
        if self.version_ui.versions.is_empty() {
            self.request_versions_for_selected();
            self.set_status("Loading versions...");
            return;
        }
        let Some(index) = self.version_ui.active_version_index() else {
            return;
        };
        if index == 0 {
            self.return_to_head_version();
            return;
        }
        if let Some(meta) = self.version_ui.versions.get(index.saturating_sub(1)) {
            self.request_version_snapshot(meta.version_id_ms);
        }
    }

    fn duplicate_active_version(&mut self) {
        let Some(id) = self.selected_id.clone() else {
            return;
        };
        let Some(version_id_ms) = self.version_ui.active_version_id_ms else {
            return;
        };
        if self
            .backend
            .cmd_tx
            .send(CoreCmd::DuplicatePasteVersion {
                id,
                version_id_ms,
                name: None,
            })
            .is_err()
        {
            self.set_status("Duplicate version failed: backend unavailable.");
            return;
        }
        self.set_status("Duplicating historical version...");
    }

    fn reset_hard_to_active_version(&mut self) {
        let Some(id) = self.selected_id.clone() else {
            return;
        };
        let Some(version_id_ms) = self.version_ui.active_version_id_ms else {
            return;
        };
        if self
            .backend
            .cmd_tx
            .send(CoreCmd::ResetPasteHardToVersion { id, version_id_ms })
            .is_err()
        {
            self.set_status("Reset hard failed: backend unavailable.");
            return;
        }
        self.version_ui.reset_confirm_open = false;
        self.set_status("Resetting paste to selected version...");
    }

    fn dispatch_diff_request(&mut self) {
        let Some(left_id) = self.selected_id.clone() else {
            return;
        };
        let right_id = self.version_ui.diff_target_paste_id.trim().to_string();
        if right_id.is_empty() {
            self.set_status("Diff target paste id is required.");
            return;
        }
        let right_version_id_ms = if self.version_ui.diff_target_version_id.trim().is_empty() {
            None
        } else {
            match self.version_ui.diff_target_version_id.trim().parse::<u64>() {
                Ok(value) => Some(value),
                Err(_) => {
                    self.set_status("Diff target version must be an integer.");
                    return;
                }
            }
        };
        if self
            .backend
            .cmd_tx
            .send(CoreCmd::DiffPastes {
                left_id,
                right_id,
                left_version_id_ms: self.version_ui.active_version_id_ms,
                right_version_id_ms,
            })
            .is_err()
        {
            self.set_status("Diff failed: backend unavailable.");
            return;
        }
        self.set_status("Computing diff...");
    }

    /// Applies version/diff backend events to local UI state.
    pub(super) fn on_version_event(&mut self, event: &CoreEvent) {
        match event {
            CoreEvent::PasteVersionsLoaded { id, items } => {
                if self.selected_id.as_deref() != Some(id.as_str()) {
                    return;
                }
                self.version_ui.versions = items.clone();
                if let Some(active) = self.version_ui.active_version_id_ms {
                    let still_present = self
                        .version_ui
                        .versions
                        .iter()
                        .any(|version| version.version_id_ms == active);
                    if !still_present {
                        self.return_to_head_version();
                    }
                }
            }
            CoreEvent::PasteVersionLoaded { snapshot } => {
                if self.selected_id.as_deref() != Some(snapshot.paste_id.as_str()) {
                    return;
                }
                self.version_ui.active_version_id_ms = Some(snapshot.version_id_ms);
                self.version_ui.historical_snapshot = Some(snapshot.clone());
                self.selected_content.reset(snapshot.content.clone());
                self.reset_virtual_editor(snapshot.content.as_str());
                self.save_status = SaveStatus::Saved;
                self.save_in_flight = false;
                self.save_request_revision = None;
                self.last_edit_at = None;
                self.set_status(format!(
                    "Viewing historical version {} (read-only).",
                    snapshot.version_id_ms
                ));
            }
            CoreEvent::PasteDiffComputed {
                left_id,
                right_id,
                left_version_id_ms,
                right_version_id_ms,
                diff,
            } => {
                if self.selected_id.as_deref() != Some(left_id.as_str()) {
                    return;
                }
                self.version_ui.diff_modal_open = true;
                self.version_ui.diff_target_paste_id = right_id.clone();
                self.version_ui.diff_target_version_id = right_version_id_ms
                    .map(|value| value.to_string())
                    .unwrap_or_default();
                let _ = left_version_id_ms;
                self.version_ui.diff_result = Some(diff.clone());
                if diff.equal {
                    self.set_status("Diff complete: no changes.");
                } else {
                    self.set_status("Diff complete.");
                }
            }
            _ => {}
        }
    }

    /// Renders version navigation controls in the editor toolbar.
    pub(super) fn render_version_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        let active_label = self
            .version_ui
            .active_version_id_ms
            .map(|id| format!("v{}", id))
            .unwrap_or_else(|| "Head".to_string());
        ui.label(RichText::new(active_label).monospace());

        if ui.small_button("Older").clicked() {
            self.navigate_to_older_version();
        }
        if ui.small_button("Newer").clicked() {
            self.navigate_to_newer_version();
        }
        if ui.small_button("Head").clicked() {
            self.return_to_head_version();
        }
        if ui.small_button("Versions").clicked() {
            self.request_versions_for_selected();
        }
        if ui.small_button("Diff").clicked() {
            self.version_ui.diff_modal_open = true;
            self.version_ui.diff_result = None;
        }

        let historical_active = self.is_viewing_historical_version();
        if ui
            .add_enabled(historical_active, egui::Button::new("Duplicate v"))
            .clicked()
        {
            self.duplicate_active_version();
        }
        if ui
            .add_enabled(historical_active, egui::Button::new("Reset --hard"))
            .clicked()
        {
            self.version_ui.reset_confirm_open = true;
        }
        if historical_active {
            ui.label(RichText::new("read-only").small());
        }
    }

    /// Renders reset confirmation and diff modal dialogs.
    pub(super) fn render_version_dialogs(&mut self, ctx: &egui::Context) {
        if self.version_ui.reset_confirm_open {
            let mut open = self.version_ui.reset_confirm_open;
            egui::Window::new("Confirm Reset --hard")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label("Reset current paste to selected historical version?");
                    ui.label("All newer snapshots will be removed.");
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            self.version_ui.reset_confirm_open = false;
                        }
                        if ui.button("Reset").clicked() {
                            self.reset_hard_to_active_version();
                        }
                    });
                });
            self.version_ui.reset_confirm_open = open && self.version_ui.reset_confirm_open;
        }

        if self.version_ui.diff_modal_open {
            let mut open = self.version_ui.diff_modal_open;
            egui::Window::new("Diff")
                .open(&mut open)
                .resizable(true)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Target paste id:");
                        ui.text_edit_singleline(&mut self.version_ui.diff_target_paste_id);
                    });
                    ui.horizontal(|ui| {
                        ui.label("Target version id (optional):");
                        ui.text_edit_singleline(&mut self.version_ui.diff_target_version_id);
                    });
                    if ui.button("Compare").clicked() {
                        self.dispatch_diff_request();
                    }
                    ui.separator();
                    if let Some(diff) = self.version_ui.diff_result.as_ref() {
                        if diff.equal {
                            ui.label("No changes.");
                        } else {
                            egui::ScrollArea::vertical()
                                .max_height(DIFF_MODAL_HEIGHT)
                                .show(ui, |ui| {
                                    for line in &diff.unified {
                                        ui.monospace(line);
                                    }
                                });
                        }
                    } else {
                        ui.label("Run a comparison to view unified diff output.");
                    }
                });
            self.version_ui.diff_modal_open = open;
        }
    }
}
