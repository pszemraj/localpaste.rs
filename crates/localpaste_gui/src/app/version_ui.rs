//! Version-history and diff modal state/helpers for the editor panel.

use super::{LocalPasteApp, COLOR_TEXT_MUTED, COLOR_TEXT_SECONDARY};
use crate::backend::{CoreCmd, CoreEvent, PasteSummary};
use eframe::egui::{self, RichText};
use localpaste_core::diff::unified_diff_lines;
use localpaste_core::models::paste::{Paste, VersionMeta, VersionSnapshot};

const VERSION_UI_LIST_LIMIT: usize = 200;
const MAX_DIFF_CANDIDATES: usize = 40;
const MAX_INLINE_DIFF_BYTES: usize = 1024 * 1024;
const DIFF_MODAL_HEIGHT: f32 = 360.0;

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
    fn clear_history_selection(&mut self) {
        self.history_versions.clear();
        self.history_selected_index = 0;
        self.history_snapshot = None;
        self.history_loading_snapshot_id = None;
        self.history_reset_confirm_open = false;
        self.history_reset_in_flight = false;
    }

    fn clear_diff_selection(&mut self) {
        self.diff_query.clear();
        self.diff_target_id = None;
        self.diff_target_paste = None;
        self.diff_loading_target = false;
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

    fn selected_history_meta(&self) -> Option<&VersionMeta> {
        let index = self.version_ui.history_selected_index;
        if index == 0 {
            return None;
        }
        self.version_ui
            .history_versions
            .get(index.saturating_sub(1))
    }

    fn set_history_selected_index(&mut self, index: usize) {
        let max_index = self.version_ui.history_versions.len();
        let next_index = index.min(max_index);
        self.version_ui.history_selected_index = next_index;
        self.version_ui.history_snapshot = None;
        self.version_ui.history_loading_snapshot_id = None;
        if let Some(meta) = self.selected_history_meta() {
            self.request_version_snapshot(meta.version_id_ms);
        }
    }

    fn open_history_modal(&mut self) {
        let Some(_) = self.selected_id else {
            self.set_status("Nothing selected.");
            return;
        };
        self.version_ui.history_modal_open = true;
        self.version_ui.clear_history_selection();
        self.request_versions_for_selected();
    }

    fn open_diff_modal(&mut self) {
        let Some(_) = self.selected_id else {
            self.set_status("Nothing selected.");
            return;
        };
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
            self.version_ui.history_loading_snapshot_id = None;
            self.set_status("Load version failed: backend unavailable.");
        }
    }

    fn duplicate_selected_history_version(&mut self) {
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

    fn reset_selected_history_version(&mut self) {
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

    fn request_diff_target(&mut self, id: String) {
        self.version_ui.diff_target_id = Some(id.clone());
        self.version_ui.diff_target_paste = None;
        self.version_ui.diff_loading_target = true;
        if self.backend.cmd_tx.send(CoreCmd::GetPaste { id }).is_err() {
            self.version_ui.diff_target_id = None;
            self.version_ui.diff_loading_target = false;
            self.set_status("Diff load failed: backend unavailable.");
        }
    }

    fn maybe_capture_diff_target_from_loaded_paste(&mut self, paste: &Paste) {
        if self.version_ui.diff_target_id.as_deref() == Some(paste.id.as_str()) {
            self.version_ui.diff_target_paste = Some(paste.clone());
            self.version_ui.diff_loading_target = false;
        }
    }

    fn diff_candidates(&self) -> Vec<PasteSummary> {
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
                if let Some(version_id_ms) = selected_version_id {
                    if let Some(index) = self
                        .version_ui
                        .history_versions
                        .iter()
                        .position(|meta| meta.version_id_ms == version_id_ms)
                    {
                        self.version_ui.history_selected_index = index.saturating_add(1);
                    } else {
                        self.version_ui.history_selected_index = 0;
                        self.version_ui.history_snapshot = None;
                        self.version_ui.history_loading_snapshot_id = None;
                    }
                } else {
                    self.version_ui.history_selected_index = 0;
                    self.version_ui.history_snapshot = None;
                    self.version_ui.history_loading_snapshot_id = None;
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
            CoreEvent::PasteSaved { paste } => {
                if self.version_ui.history_reset_in_flight
                    && self.selected_id.as_deref() == Some(paste.id.as_str())
                {
                    // Reset is authoritative: replace any local unsaved/editor state
                    // with the canonical backend row that reset produced.
                    self.select_loaded_paste(paste.clone());
                    self.version_ui.history_reset_in_flight = false;
                    self.version_ui.history_modal_open = false;
                    self.version_ui.history_selected_index = 0;
                    self.version_ui.history_snapshot = None;
                    self.version_ui.history_loading_snapshot_id = None;
                    self.set_status("Reset current paste to selected historical snapshot.");
                }
            }
            CoreEvent::PasteMissing { id } => {
                if self.version_ui.diff_target_id.as_deref() == Some(id.as_str()) {
                    self.version_ui.diff_target_paste = None;
                    self.version_ui.diff_target_id = None;
                    self.version_ui.diff_loading_target = false;
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

    fn render_history_modal(&mut self, ctx: &egui::Context) {
        if !self.version_ui.history_modal_open {
            return;
        }
        let Some(selected_id) = self.selected_id.clone() else {
            self.version_ui.clear_history_selection();
            self.version_ui.history_modal_open = false;
            return;
        };

        let mut keep_open = true;
        let mut pending_selected_index: Option<usize> = None;
        let mut pending_refresh = false;
        let mut pending_duplicate = false;
        let mut pending_open_reset_confirm = false;

        egui::Window::new("History")
            .open(&mut keep_open)
            .default_width(1080.0)
            .default_height(760.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.small_button("Refresh").clicked() {
                        pending_refresh = true;
                    }
                    let total = self.version_ui.history_versions.len();
                    ui.label(
                        RichText::new(format!("{total} stored snapshots"))
                            .small()
                            .color(COLOR_TEXT_MUTED),
                    );
                });

                ui.add_space(8.0);
                ui.columns(2, |columns| {
                    let (left_columns, right_columns) = columns.split_at_mut(1);
                    let left = &mut left_columns[0];
                    let right = &mut right_columns[0];

                    left.label(RichText::new("Versions").small().color(COLOR_TEXT_MUTED));
                    left.add_space(4.0);
                    egui::ScrollArea::vertical()
                        .max_height(620.0)
                        .show(left, |ui| {
                            let current_selected = self.version_ui.history_selected_index == 0;
                            if ui
                                .selectable_label(current_selected, "Current working copy")
                                .clicked()
                            {
                                pending_selected_index = Some(0);
                            }
                            ui.separator();
                            for (idx, meta) in self.version_ui.history_versions.iter().enumerate() {
                                let row_index = idx.saturating_add(1);
                                let selected_row =
                                    self.version_ui.history_selected_index == row_index;
                                let label = format!(
                                    "{}  ({} bytes)",
                                    meta.created_at.to_rfc3339(),
                                    meta.len
                                );
                                if ui.selectable_label(selected_row, label).clicked() {
                                    pending_selected_index = Some(row_index);
                                }
                            }
                        });

                    right.label(RichText::new("Snapshot").small().color(COLOR_TEXT_MUTED));
                    right.add_space(4.0);
                    let viewing_historical = self.version_ui.history_selected_index > 0;
                    if viewing_historical {
                        if let Some(meta) = self.selected_history_meta() {
                            right.label(
                                RichText::new(format!(
                                    "Version {} at {}",
                                    meta.version_id_ms,
                                    meta.created_at.to_rfc3339()
                                ))
                                .small()
                                .color(COLOR_TEXT_SECONDARY),
                            );
                        }
                    } else {
                        right.label(
                            RichText::new("Current unsaved editor view")
                                .small()
                                .color(COLOR_TEXT_SECONDARY),
                        );
                    }
                    right.add_space(6.0);

                    let mut body = if self.version_ui.history_selected_index == 0 {
                        self.active_snapshot()
                    } else if let Some(snapshot) = self.version_ui.history_snapshot.as_ref() {
                        snapshot.content.clone()
                    } else {
                        String::new()
                    };

                    if self.version_ui.history_selected_index > 0
                        && self.version_ui.history_snapshot.is_none()
                    {
                        if self.version_ui.history_loading_snapshot_id.is_some() {
                            right.label(
                                RichText::new("Loading snapshot...")
                                    .small()
                                    .color(COLOR_TEXT_MUTED),
                            );
                        } else {
                            right.label(
                                RichText::new("Select a stored snapshot.")
                                    .small()
                                    .color(COLOR_TEXT_MUTED),
                            );
                        }
                    }

                    right.add(
                        egui::TextEdit::multiline(&mut body)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY)
                            .desired_rows(30)
                            .interactive(false),
                    );

                    right.add_space(8.0);
                    right.horizontal_wrapped(|ui| {
                        let can_act_on_snapshot = self.version_ui.history_selected_index > 0
                            && self.version_ui.history_snapshot.is_some();
                        if ui
                            .add_enabled(
                                can_act_on_snapshot,
                                egui::Button::new("Duplicate as New Paste"),
                            )
                            .clicked()
                        {
                            pending_duplicate = true;
                        }
                        if ui
                            .add_enabled(
                                can_act_on_snapshot && !self.version_ui.history_reset_in_flight,
                                egui::Button::new("Reset Current Paste to This Version"),
                            )
                            .clicked()
                        {
                            pending_open_reset_confirm = true;
                        }
                    });
                });
            });

        if let Some(index) = pending_selected_index {
            self.set_history_selected_index(index);
        }
        if pending_refresh {
            self.request_versions_for_selected();
        }
        if pending_duplicate {
            self.duplicate_selected_history_version();
        }
        if pending_open_reset_confirm {
            self.version_ui.history_reset_confirm_open = true;
        }
        if !keep_open {
            self.version_ui.history_modal_open = false;
            self.version_ui.history_reset_confirm_open = false;
        }

        if self.version_ui.history_reset_confirm_open {
            let mut confirm_open = self.version_ui.history_reset_confirm_open;
            egui::Window::new("Confirm reset")
                .collapsible(false)
                .resizable(false)
                .open(&mut confirm_open)
                .show(ctx, |ui| {
                    ui.label(
                        "Reset current paste to this snapshot? This discards newer history and any unsaved local edits.",
                    );
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            self.version_ui.history_reset_confirm_open = false;
                        }
                        if ui.button("Reset --hard").clicked() {
                            self.reset_selected_history_version();
                        }
                    });
                });
            self.version_ui.history_reset_confirm_open =
                confirm_open && self.version_ui.history_reset_confirm_open;
        }

        if !self.version_ui.history_modal_open {
            self.version_ui.history_reset_confirm_open = false;
            self.version_ui.history_loading_snapshot_id = None;
            self.version_ui.history_snapshot = None;
            self.version_ui.history_selected_index = 0;
        }

        if self.selected_id.as_deref() != Some(selected_id.as_str()) {
            self.version_ui.history_modal_open = false;
            self.version_ui.history_reset_confirm_open = false;
        }
    }

    fn render_diff_modal(&mut self, ctx: &egui::Context) {
        if !self.version_ui.diff_modal_open {
            return;
        }
        let Some(selected_name) = self.selected_paste.as_ref().map(|paste| paste.name.clone())
        else {
            self.version_ui.diff_modal_open = false;
            self.version_ui.clear_diff_selection();
            return;
        };

        let mut keep_open = true;
        let mut pending_target: Option<String> = None;

        egui::Window::new("Diff")
            .open(&mut keep_open)
            .default_width(1180.0)
            .default_height(760.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Compare current paste against:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.version_ui.diff_query)
                            .desired_width(280.0)
                            .hint_text("name, id, tag, language"),
                    );
                });
                ui.add_space(8.0);

                ui.columns(2, |columns| {
                    let (left_columns, right_columns) = columns.split_at_mut(1);
                    let left = &mut left_columns[0];
                    let right = &mut right_columns[0];

                    left.label(RichText::new("Candidates").small().color(COLOR_TEXT_MUTED));
                    left.add_space(4.0);

                    egui::ScrollArea::vertical()
                        .max_height(640.0)
                        .show(left, |ui| {
                            for item in self.diff_candidates() {
                                let selected_row = self.version_ui.diff_target_id.as_deref()
                                    == Some(item.id.as_str());
                                let label = format!(
                                    "{}  [{}]",
                                    item.name,
                                    item.language.as_deref().unwrap_or("text")
                                );
                                if ui.selectable_label(selected_row, label).clicked() {
                                    pending_target = Some(item.id.clone());
                                }
                            }
                        });

                    right.label(RichText::new("Line diff").small().color(COLOR_TEXT_MUTED));
                    right.add_space(4.0);

                    let lhs = self.active_snapshot();
                    match self.version_ui.diff_target_paste.as_ref() {
                        None => {
                            if self.version_ui.diff_loading_target {
                                right.label(
                                    RichText::new("Loading comparison target...")
                                        .color(COLOR_TEXT_MUTED),
                                );
                            } else {
                                right.label(
                                    RichText::new("Pick a paste from the left.")
                                        .color(COLOR_TEXT_MUTED),
                                );
                            }
                        }
                        Some(rhs) => {
                            right.horizontal_wrapped(|ui| {
                                ui.label(
                                    RichText::new(format!("Left: {}", selected_name))
                                        .color(COLOR_TEXT_SECONDARY),
                                );
                                ui.separator();
                                ui.label(
                                    RichText::new(format!("Right: {}", rhs.name))
                                        .color(COLOR_TEXT_SECONDARY),
                                );
                            });

                            if lhs.len().saturating_add(rhs.content.len()) > MAX_INLINE_DIFF_BYTES {
                                right.add_space(8.0);
                                right.label(
                                    RichText::new(
                                        "Diff preview capped for now; payloads are too large.",
                                    )
                                    .color(egui::Color32::YELLOW),
                                );
                                right.label(format!(
                                    "left={} bytes, right={} bytes",
                                    lhs.len(),
                                    rhs.content.len()
                                ));
                                return;
                            }

                            let diff_lines = unified_diff_lines(lhs.as_str(), rhs.content.as_str());
                            if diff_lines.is_empty() {
                                right.label(RichText::new("No changes.").color(COLOR_TEXT_MUTED));
                                return;
                            }

                            egui::ScrollArea::vertical()
                                .max_height(DIFF_MODAL_HEIGHT)
                                .show(right, |ui| {
                                    for line in &diff_lines {
                                        let color = match line.chars().next() {
                                            Some('-') => egui::Color32::LIGHT_RED,
                                            Some('+') => egui::Color32::LIGHT_GREEN,
                                            _ => COLOR_TEXT_SECONDARY,
                                        };
                                        ui.label(RichText::new(line).monospace().color(color));
                                    }
                                });
                        }
                    }
                });
            });

        if let Some(target_id) = pending_target {
            self.request_diff_target(target_id);
        }
        if !keep_open {
            self.version_ui.diff_modal_open = false;
            self.version_ui.clear_diff_selection();
        }
    }
}
