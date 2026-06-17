//! Detached version-history modal rendering.

use super::super::*;
use crate::app::editor::EditorLineIndex;
use crate::app::text_coords::prefix_by_chars;
use chrono::{DateTime, Local, Utc};
use eframe::egui::{self, RichText};
use localpaste_core::models::paste::VersionMeta;

const MAX_INLINE_HISTORY_TEXTEDIT_BYTES: usize = 256 * 1024;
const HISTORY_PREVIEW_MAX_HEIGHT: f32 = 560.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HistoryPreviewRenderMode {
    InlineTextEdit,
    FastRows,
}

fn history_preview_render_mode(body_len: usize) -> HistoryPreviewRenderMode {
    if body_len > MAX_INLINE_HISTORY_TEXTEDIT_BYTES {
        HistoryPreviewRenderMode::FastRows
    } else {
        HistoryPreviewRenderMode::InlineTextEdit
    }
}

fn format_history_size(len: usize) -> String {
    const KIB: f32 = 1024.0;
    const MIB: f32 = 1024.0 * 1024.0;
    if len < 1024 {
        format!("{len} B")
    } else if len < 1024 * 1024 {
        format!("{:.1} KB", len as f32 / KIB)
    } else {
        format!("{:.1} MB", len as f32 / MIB)
    }
}

fn format_history_relative_time(now: DateTime<Utc>, created_at: DateTime<Utc>) -> String {
    let elapsed_secs = now.signed_duration_since(created_at).num_seconds().max(0);
    if elapsed_secs < 60 {
        "just now".to_string()
    } else if elapsed_secs < 60 * 60 {
        format!("{} min ago", elapsed_secs / 60)
    } else if elapsed_secs < 24 * 60 * 60 {
        format!("{} hr ago", elapsed_secs / (60 * 60))
    } else if elapsed_secs < 7 * 24 * 60 * 60 {
        format!("{} d ago", elapsed_secs / (24 * 60 * 60))
    } else {
        created_at
            .with_timezone(&Local)
            .format("%Y-%m-%d")
            .to_string()
    }
}

fn format_history_row_label(now: DateTime<Utc>, meta: &VersionMeta) -> String {
    let relative = format_history_relative_time(now, meta.created_at);
    let size = format_history_size(meta.len);
    let language = display_language_label(
        meta.language.as_deref(),
        meta.language_is_manual,
        meta.len > HIGHLIGHT_PLAIN_THRESHOLD,
    );
    format!("{relative} | {size} | {language}")
}

fn history_index_after_arrow_key(
    current_index: usize,
    version_count: usize,
    key: egui::Key,
) -> Option<usize> {
    match key {
        egui::Key::ArrowUp if current_index > 0 => Some(current_index.saturating_sub(1)),
        egui::Key::ArrowDown if current_index < version_count => {
            Some(current_index.saturating_add(1))
        }
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HistoryActionHelper {
    ResetInProgress,
    SaveBeforeReset,
    Blocked(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HistorySnapshotActionState {
    can_duplicate: bool,
    can_open_reset_confirm: bool,
    reset_label: &'static str,
    helper: Option<HistoryActionHelper>,
}

fn history_snapshot_action_state(
    can_act_on_snapshot: bool,
    reset_transition_active: bool,
    can_queue_history_reset: bool,
    flush_needed: bool,
    block_reason: Option<&'static str>,
) -> HistorySnapshotActionState {
    let helper = if can_act_on_snapshot && reset_transition_active {
        Some(HistoryActionHelper::ResetInProgress)
    } else if can_act_on_snapshot {
        block_reason
            .map(HistoryActionHelper::Blocked)
            .or_else(|| flush_needed.then_some(HistoryActionHelper::SaveBeforeReset))
    } else {
        None
    };

    HistorySnapshotActionState {
        can_duplicate: can_act_on_snapshot && !reset_transition_active,
        can_open_reset_confirm: can_act_on_snapshot && can_queue_history_reset,
        reset_label: if flush_needed {
            "Save and Reset Current Paste"
        } else {
            "Reset Current Paste to This Version"
        },
        helper,
    }
}

fn render_large_history_preview(ui: &mut egui::Ui, text: &str, lines: &EditorLineIndex) {
    let row_height = ui.text_style_height(&egui::TextStyle::Monospace).max(1.0);
    let line_count = lines.line_count();
    let line_digits = line_count.max(1).to_string().len();

    ui.label(
        RichText::new(format!(
            "Large snapshot view optimized for responsiveness; lines render up to {} chars.",
            MAX_RENDER_CHARS_PER_LINE
        ))
        .small()
        .color(COLOR_TEXT_MUTED),
    );
    ui.add_space(6.0);

    egui::ScrollArea::vertical()
        .max_height(HISTORY_PREVIEW_MAX_HEIGHT)
        .auto_shrink([false, false])
        .show_rows(ui, row_height, line_count, |ui, range| {
            ui.set_min_width(ui.available_width());
            for line_idx in range {
                let line = lines.line_without_newline(text, line_idx);
                let render_line = prefix_by_chars(line, MAX_RENDER_CHARS_PER_LINE);
                ui.horizontal(|ui| {
                    ui.add_sized(
                        [((line_digits + 2) as f32) * row_height * 0.7, row_height],
                        egui::Label::new(
                            RichText::new(format!(
                                "{:>width$}",
                                line_idx.saturating_add(1),
                                width = line_digits
                            ))
                            .monospace()
                            .color(COLOR_TEXT_MUTED),
                        )
                        .truncate(),
                    );
                    ui.add_sized(
                        [ui.available_width(), row_height],
                        egui::Label::new(RichText::new(render_line).monospace()).truncate(),
                    );
                });
            }
        });
}

impl LocalPasteApp {
    /// Renders the detached history modal for read-only snapshot navigation.
    ///
    /// # Panics
    /// Panics if egui text layout internals fail while shaping modal content.
    pub(crate) fn render_history_modal(&mut self, ctx: &egui::Context) {
        if !self.version_ui.history_modal_open {
            return;
        }
        let Some(selected_id) = self.selected_id.clone() else {
            self.close_history_modal();
            return;
        };

        let mut keep_open = true;
        let mut pending_selected_index: Option<usize> = None;
        let mut pending_refresh = false;
        let mut pending_duplicate = false;
        let mut pending_open_reset_confirm = false;
        if !self.version_ui.history_reset_confirm_open {
            let arrow_key = ctx.input_mut(|input| {
                if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                    Some(egui::Key::ArrowUp)
                } else if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
                    Some(egui::Key::ArrowDown)
                } else {
                    None
                }
            });
            if let Some(next_index) = arrow_key.and_then(|key| {
                history_index_after_arrow_key(
                    self.version_ui.history_selected_index,
                    self.version_ui.history_versions.len(),
                    key,
                )
            }) {
                pending_selected_index = Some(next_index);
            }
        }
        let escape_pressed = ctx.input(|input| input.key_pressed(egui::Key::Escape));
        let close_history_on_escape = escape_pressed && !self.version_ui.history_reset_confirm_open;
        let close_confirm_on_escape = escape_pressed && self.version_ui.history_reset_confirm_open;
        let history_label_now = Utc::now();
        if self.version_ui.history_selected_index == 0 {
            let _recomputed_active_snapshot = self.sync_active_snapshot_cache();
        } else {
            let _recomputed_history_preview = self.sync_history_preview_cache();
        }

        with_muted_modal_chrome(ctx, || {
            egui::Window::new("History")
                .open(&mut keep_open)
                .default_width(1080.0)
                .default_height(760.0)
                .show(ctx, |ui| {
                    let reset_transition_active = self.reset_transition_active();
                    let can_go_newer = self.version_ui.history_selected_index > 0;
                    let can_go_older = self.version_ui.history_selected_index
                        < self.version_ui.history_versions.len();

                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(can_go_newer, egui::Button::new("← Newer"))
                            .clicked()
                        {
                            pending_selected_index =
                                Some(self.version_ui.history_selected_index.saturating_sub(1));
                        }
                        if ui
                            .add_enabled(can_go_older, egui::Button::new("Older →"))
                            .clicked()
                        {
                            pending_selected_index =
                                Some(self.version_ui.history_selected_index.saturating_add(1));
                        }
                        ui.separator();
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
                                for (idx, meta) in
                                    self.version_ui.history_versions.iter().enumerate()
                                {
                                    let row_index = idx.saturating_add(1);
                                    let selected_row =
                                        self.version_ui.history_selected_index == row_index;
                                    let label = format_history_row_label(history_label_now, meta);
                                    let response = ui
                                        .selectable_label(selected_row, label)
                                        .on_hover_text(meta.created_at.to_rfc3339());
                                    if response.clicked() {
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

                        right.horizontal_wrapped(|ui| {
                            let can_act_on_snapshot = self.version_ui.history_selected_index > 0
                                && self.version_ui.history_snapshot.is_some();
                            let action_state = history_snapshot_action_state(
                                can_act_on_snapshot,
                                reset_transition_active,
                                self.can_queue_history_reset(),
                                self.history_reset_flush_needed(),
                                self.history_reset_queue_block_reason(),
                            );
                            if ui
                                .add_enabled(
                                    action_state.can_duplicate,
                                    egui::Button::new("Duplicate as New Paste"),
                                )
                                .clicked()
                            {
                                pending_duplicate = true;
                            }
                            if ui
                                .add_enabled(
                                    action_state.can_open_reset_confirm,
                                    egui::Button::new(action_state.reset_label),
                                )
                                .clicked()
                            {
                                pending_open_reset_confirm = true;
                            }
                            if let Some(helper) = action_state.helper {
                                let helper_text = match helper {
                                    HistoryActionHelper::ResetInProgress => {
                                        "Reset in progress; current paste is temporarily read-only."
                                    }
                                    HistoryActionHelper::SaveBeforeReset => {
                                        "Local changes will be saved before reset."
                                    }
                                    HistoryActionHelper::Blocked(reason) => reason,
                                };
                                ui.label(
                                    RichText::new(helper_text).small().color(COLOR_TEXT_MUTED),
                                );
                            }
                        });
                        right.add_space(8.0);

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

                        if self.version_ui.history_selected_index == 0 {
                            let mode = history_preview_render_mode(
                                self.version_ui.active_snapshot_cache_text.len(),
                            );
                            match mode {
                                HistoryPreviewRenderMode::InlineTextEdit => {
                                    egui::ScrollArea::vertical()
                                        .max_height(HISTORY_PREVIEW_MAX_HEIGHT)
                                        .auto_shrink([false, false])
                                        .show(right, |ui| {
                                            ui.add(
                                                egui::TextEdit::multiline(
                                                    &mut self.version_ui.active_snapshot_cache_text,
                                                )
                                                .font(egui::TextStyle::Monospace)
                                                .desired_width(f32::INFINITY)
                                                .desired_rows(30)
                                                .interactive(false),
                                            );
                                        });
                                }
                                HistoryPreviewRenderMode::FastRows => {
                                    render_large_history_preview(
                                        right,
                                        self.version_ui.active_snapshot_cache_text.as_str(),
                                        &self.version_ui.active_snapshot_preview_lines,
                                    );
                                }
                            }
                        } else {
                            let mode = history_preview_render_mode(
                                self.version_ui.history_preview_text.len(),
                            );
                            match mode {
                                HistoryPreviewRenderMode::InlineTextEdit => {
                                    egui::ScrollArea::vertical()
                                        .max_height(HISTORY_PREVIEW_MAX_HEIGHT)
                                        .auto_shrink([false, false])
                                        .show(right, |ui| {
                                            ui.add(
                                                egui::TextEdit::multiline(
                                                    &mut self.version_ui.history_preview_text,
                                                )
                                                .font(egui::TextStyle::Monospace)
                                                .desired_width(f32::INFINITY)
                                                .desired_rows(30)
                                                .interactive(false),
                                            );
                                        });
                                }
                                HistoryPreviewRenderMode::FastRows => {
                                    render_large_history_preview(
                                        right,
                                        self.version_ui.history_preview_text.as_str(),
                                        &self.version_ui.history_preview_lines,
                                    );
                                }
                            }
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
            self.open_history_reset_confirm();
        }
        if close_history_on_escape {
            keep_open = false;
        }
        if !keep_open {
            self.close_history_modal();
        }

        if self.version_ui.history_reset_confirm_open {
            if close_confirm_on_escape {
                self.version_ui.clear_history_reset_confirm();
            }
            let mut confirm_open = self.version_ui.history_reset_confirm_open;
            with_muted_modal_chrome(ctx, || {
                egui::Window::new("Confirm reset")
                    .collapsible(false)
                    .resizable(false)
                    .open(&mut confirm_open)
                    .show(ctx, |ui| {
                        ui.label(
                            "Reset current paste to this snapshot? This discards newer history.",
                        );
                        if let Some(reason) = self.history_reset_queue_block_reason() {
                            ui.add_space(6.0);
                            ui.label(RichText::new(reason).small().color(COLOR_TEXT_MUTED));
                        } else if self.history_reset_flush_needed() {
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new(
                                    "Current local changes will be saved before reset starts.",
                                )
                                .small()
                                .color(COLOR_TEXT_MUTED),
                            );
                        }
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("Cancel").clicked() {
                                self.version_ui.clear_history_reset_confirm();
                            }
                            let reset_button_label = if self.history_reset_flush_needed() {
                                "Save and Reset"
                            } else {
                                "Reset --hard"
                            };
                            if ui
                                .add_enabled(
                                    self.can_queue_history_reset(),
                                    egui::Button::new(reset_button_label),
                                )
                                .clicked()
                            {
                                self.reset_selected_history_version();
                            }
                        });
                    });
            });
            if !(confirm_open && self.version_ui.history_reset_confirm_open) {
                self.version_ui.clear_history_reset_confirm();
            }
        }

        if !self.version_ui.history_modal_open {
            self.close_history_modal();
        }

        if self.selected_id.as_deref() != Some(selected_id.as_str()) {
            self.close_history_modal();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        format_history_relative_time, format_history_row_label, history_index_after_arrow_key,
        history_preview_render_mode, history_snapshot_action_state, HistoryActionHelper,
        HistoryPreviewRenderMode, MAX_INLINE_HISTORY_TEXTEDIT_BYTES,
    };
    use chrono::{Duration, TimeZone, Utc};
    use eframe::egui;
    use localpaste_core::models::paste::VersionMeta;

    #[test]
    fn history_preview_switches_to_fast_rows_for_large_bodies() {
        assert_eq!(
            history_preview_render_mode(MAX_INLINE_HISTORY_TEXTEDIT_BYTES),
            HistoryPreviewRenderMode::InlineTextEdit
        );
        assert_eq!(
            history_preview_render_mode(MAX_INLINE_HISTORY_TEXTEDIT_BYTES.saturating_add(1)),
            HistoryPreviewRenderMode::FastRows
        );
    }

    #[test]
    fn history_action_state_keeps_duplicate_safe_and_dirty_reset_queueable() {
        let dirty = history_snapshot_action_state(true, false, true, true, None);
        assert!(dirty.can_duplicate);
        assert!(dirty.can_open_reset_confirm);
        assert_eq!(dirty.reset_label, "Save and Reset Current Paste");
        assert_eq!(dirty.helper, Some(HistoryActionHelper::SaveBeforeReset));

        let blocked = history_snapshot_action_state(true, false, false, false, Some("busy"));
        assert!(
            blocked.can_duplicate,
            "duplicate should stay available when only reset is blocked"
        );
        assert!(!blocked.can_open_reset_confirm);
        assert_eq!(blocked.helper, Some(HistoryActionHelper::Blocked("busy")));

        let blocked_while_flush_needed =
            history_snapshot_action_state(true, false, false, true, Some("save in progress"));
        assert_eq!(
            blocked_while_flush_needed.helper,
            Some(HistoryActionHelper::Blocked("save in progress")),
            "a hard block should not be described as a queued save-before-reset"
        );

        let in_flight = history_snapshot_action_state(true, true, false, false, None);
        assert!(!in_flight.can_duplicate);
        assert!(!in_flight.can_open_reset_confirm);
        assert_eq!(in_flight.helper, Some(HistoryActionHelper::ResetInProgress));

        let no_snapshot = history_snapshot_action_state(false, false, true, true, None);
        assert!(!no_snapshot.can_duplicate);
        assert!(
            !no_snapshot.can_open_reset_confirm,
            "reset must not arm without a loaded historical snapshot"
        );
        assert!(no_snapshot.helper.is_none());
    }

    #[test]
    fn history_row_label_uses_relative_time_size_and_language() {
        let now = Utc.with_ymd_and_hms(2026, 6, 15, 12, 0, 0).unwrap();
        let meta = VersionMeta {
            version_id_ms: 1,
            created_at: now - Duration::minutes(14),
            content_hash: "hash".to_string(),
            len: 1234,
            language: Some("rust".to_string()),
            language_is_manual: false,
        };

        assert_eq!(
            format_history_row_label(now, &meta),
            "14 min ago | 1.2 KB | rust"
        );
    }

    #[test]
    fn history_relative_time_clamps_future_timestamps_to_just_now() {
        let now = Utc.with_ymd_and_hms(2026, 6, 15, 12, 0, 0).unwrap();

        assert_eq!(
            format_history_relative_time(now, now + Duration::minutes(5)),
            "just now"
        );
    }

    #[test]
    fn history_arrow_navigation_stays_in_bounds() {
        assert_eq!(
            history_index_after_arrow_key(0, 3, egui::Key::ArrowDown),
            Some(1)
        );
        assert_eq!(
            history_index_after_arrow_key(1, 3, egui::Key::ArrowUp),
            Some(0)
        );
        assert_eq!(
            history_index_after_arrow_key(0, 3, egui::Key::ArrowUp),
            None
        );
        assert_eq!(
            history_index_after_arrow_key(3, 3, egui::Key::ArrowDown),
            None
        );
    }
}
