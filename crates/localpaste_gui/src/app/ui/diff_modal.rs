//! Detached diff modal rendering.

use super::super::*;
use eframe::egui::{self, RichText};
use localpaste_core::diff::unified_diff_lines;

const MAX_INLINE_DIFF_BYTES: usize = 1024 * 1024;
const DIFF_MODAL_HEIGHT: f32 = 360.0;

impl LocalPasteApp {
    /// Renders the detached diff modal using current editor snapshot as left side.
    ///
    /// # Panics
    /// Panics if egui text layout internals fail while shaping modal content.
    pub(crate) fn render_diff_modal(&mut self, ctx: &egui::Context) {
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
