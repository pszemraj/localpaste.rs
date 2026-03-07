//! Detached diff modal rendering.

use super::super::*;
use eframe::egui::{self, RichText};
use localpaste_core::diff::unified_diff_lines;

/// Maximum combined byte size allowed for inline diff preview generation.
pub(crate) const MAX_INLINE_DIFF_BYTES: usize = 1024 * 1024;
const DIFF_MODAL_HEIGHT: f32 = 360.0;

/// Cached inline diff preview state for the detached diff modal.
///
/// Large inputs collapse to a size warning instead of recomputing/rendering a
/// full inline diff on the UI thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InlineDiffPreview {
    TooLarge { lhs_bytes: usize, rhs_bytes: usize },
    NoChanges,
    Lines(Vec<String>),
}

/// Builds a bounded inline diff preview classification for two snapshot bodies.
///
/// # Arguments
/// - `lhs`: Current editor snapshot shown on the left side of the modal.
/// - `rhs`: Loaded comparison target content shown on the right side.
///
/// # Returns
/// A cached-friendly preview state describing whether the modal should show a
/// line diff, a no-op message, or a size cap warning.
pub(crate) fn inline_diff_preview(lhs: &str, rhs: &str) -> InlineDiffPreview {
    if lhs.len().saturating_add(rhs.len()) > MAX_INLINE_DIFF_BYTES {
        return InlineDiffPreview::TooLarge {
            lhs_bytes: lhs.len(),
            rhs_bytes: rhs.len(),
        };
    }

    if lhs == rhs {
        return InlineDiffPreview::NoChanges;
    }

    let diff_lines = unified_diff_lines(lhs, rhs);
    if diff_lines.is_empty() {
        InlineDiffPreview::NoChanges
    } else {
        InlineDiffPreview::Lines(diff_lines)
    }
}

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
        let _recomputed_diff_preview = self.sync_diff_preview_cache();

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

                            // Preview generation is cached by active buffer revision + diff target.
                            // Repaints should not reclone large buffers or rerun the diff engine.
                            match self.version_ui.diff_preview.as_ref() {
                                None => {
                                    right.label(
                                        RichText::new("Preparing diff preview...")
                                            .color(COLOR_TEXT_MUTED),
                                    );
                                }
                                Some(InlineDiffPreview::TooLarge {
                                    lhs_bytes,
                                    rhs_bytes,
                                }) => {
                                    right.add_space(8.0);
                                    right.label(
                                        RichText::new(
                                            "Diff preview capped for now; payloads are too large.",
                                        )
                                        .color(egui::Color32::YELLOW),
                                    );
                                    right.label(format!(
                                        "left={} bytes, right={} bytes",
                                        lhs_bytes, rhs_bytes
                                    ));
                                }
                                Some(InlineDiffPreview::NoChanges) => {
                                    right.label(
                                        RichText::new("No changes.").color(COLOR_TEXT_MUTED),
                                    );
                                }
                                Some(InlineDiffPreview::Lines(diff_lines)) => {
                                    egui::ScrollArea::vertical()
                                        .max_height(DIFF_MODAL_HEIGHT)
                                        .show(right, |ui| {
                                            for line in diff_lines {
                                                let color = match line.chars().next() {
                                                    Some('-') => egui::Color32::LIGHT_RED,
                                                    Some('+') => egui::Color32::LIGHT_GREEN,
                                                    _ => COLOR_TEXT_SECONDARY,
                                                };
                                                ui.label(
                                                    RichText::new(line).monospace().color(color),
                                                );
                                            }
                                        });
                                }
                            }
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

#[cfg(test)]
mod tests {
    use super::{inline_diff_preview, InlineDiffPreview, MAX_INLINE_DIFF_BYTES};

    #[test]
    fn inline_diff_preview_classifies_changes_size_and_identity() {
        assert_eq!(
            inline_diff_preview("same", "same"),
            InlineDiffPreview::NoChanges
        );

        let changed = inline_diff_preview("alpha", "beta");
        assert!(
            matches!(changed, InlineDiffPreview::Lines(lines) if !lines.is_empty()),
            "changed content should produce inline diff lines"
        );

        let large = "x".repeat(MAX_INLINE_DIFF_BYTES);
        assert_eq!(
            inline_diff_preview(large.as_str(), "y"),
            InlineDiffPreview::TooLarge {
                lhs_bytes: MAX_INLINE_DIFF_BYTES,
                rhs_bytes: 1,
            }
        );
    }
}
