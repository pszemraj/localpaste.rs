//! Detached diff modal rendering.

use super::super::*;
use crate::app::text_coords::prefix_by_chars;
use eframe::egui::{self, RichText};
use localpaste_core::diff::DiffResponse;
use localpaste_core::MAX_DIFF_INPUT_BYTES;
use std::sync::Arc;

/// Maximum combined byte size allowed for inline diff preview generation.
pub(crate) const MAX_INLINE_DIFF_BYTES: usize = MAX_DIFF_INPUT_BYTES;
/// Maximum diff row count cached/rendered inline before preview is summarized.
pub(crate) const MAX_INLINE_DIFF_LINES: usize = 20_000;
/// Height cap shared by the candidate list and the line diff so the panes line up.
const DIFF_PANE_MAX_HEIGHT: f32 = 640.0;

/// Cached inline diff preview state for the detached diff modal.
///
/// Large inputs collapse to a size warning instead of recomputing/rendering a
/// full inline diff on the UI thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InlineDiffPreview {
    TooLarge { lhs_bytes: usize, rhs_bytes: usize },
    TooManyLines { line_count: usize },
    NoChanges,
    Lines(Arc<[String]>),
}

fn normalize_gui_diff_lines(mut lines: Vec<String>) -> Vec<String> {
    for line in &mut lines {
        let trimmed_len = line.trim_end_matches(['\r', '\n']).len();
        line.truncate(trimmed_len);
    }
    lines
}

/// Builds an inline diff preview classification from a worker-produced diff response.
///
/// # Arguments
/// - `diff`: Worker-computed line diff payload for the active modal request.
///
/// # Returns
/// A cached-friendly preview state describing whether the modal should show a
/// line diff or a no-op message. Oversized previews are gated before dispatch.
pub(crate) fn inline_diff_preview_from_response(diff: DiffResponse) -> InlineDiffPreview {
    if diff.equal || diff.unified.is_empty() {
        return InlineDiffPreview::NoChanges;
    }

    let lines = normalize_gui_diff_lines(diff.unified);
    if lines.len() > MAX_INLINE_DIFF_LINES {
        return InlineDiffPreview::TooManyLines {
            line_count: lines.len(),
        };
    }

    InlineDiffPreview::Lines(lines.into())
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
            self.close_diff_modal();
            return;
        };

        let mut keep_open = true;
        let close_on_escape =
            ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        let mut pending_target: Option<String> = None;
        let _recomputed_diff_preview = self.sync_diff_preview_cache();

        egui::Window::new("Diff")
            .open(&mut keep_open)
            .default_width(1180.0)
            .default_height(760.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Compare current paste against:");
                    if self.version_ui.diff_query_focus_requested
                        && super::focus_visible_query(ui, egui::Id::new(DIFF_QUERY_INPUT_ID))
                    {
                        self.version_ui.diff_query_focus_requested = false;
                    }
                    ui.add(
                        egui::TextEdit::singleline(&mut self.version_ui.diff_query)
                            .id(egui::Id::new(DIFF_QUERY_INPUT_ID))
                            .desired_width(280.0)
                            .muted_hint("name, id, tag, language"),
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
                        .id_salt("diff_candidates")
                        .max_height(DIFF_PANE_MAX_HEIGHT)
                        .show(left, |ui| {
                            for item in self.diff_candidates() {
                                let selected_row = self.version_ui.diff_target_id.as_deref()
                                    == Some(item.id.as_str());
                                let language = paste_list_language_label(&item);
                                if super::list_row(ui, selected_row, &item.name, &language)
                                    .clicked()
                                {
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
                                        "left={}, right={}",
                                        count_label(*lhs_bytes, "byte", "bytes"),
                                        count_label(*rhs_bytes, "byte", "bytes")
                                    ));
                                }
                                Some(InlineDiffPreview::NoChanges) => {
                                    right.label(
                                        RichText::new("No changes.").color(COLOR_TEXT_MUTED),
                                    );
                                }
                                Some(InlineDiffPreview::TooManyLines { line_count }) => {
                                    right.add_space(8.0);
                                    right.label(
                                        RichText::new(
                                            "Diff preview capped for now; too many changed lines to render inline.",
                                        )
                                        .color(egui::Color32::YELLOW),
                                    );
                                    right.label(format!(
                                        "{} diff rows exceed the inline preview cap of {}.",
                                        line_count, MAX_INLINE_DIFF_LINES
                                    ));
                                }
                                Some(InlineDiffPreview::Lines(diff_lines)) => {
                                    let row_height =
                                        right.text_style_height(&egui::TextStyle::Monospace).max(1.0);
                                    egui::ScrollArea::vertical()
                                        .id_salt("diff_preview")
                                        .max_height(DIFF_PANE_MAX_HEIGHT)
                                        .auto_shrink([false, false])
                                        .show_rows(right, row_height, diff_lines.len(), |ui, range| {
                                            ui.set_min_width(ui.available_width());
                                            let gap = ui.spacing().item_spacing.y;
                                            for idx in range {
                                                let line = &diff_lines[idx];
                                                let (color, tint) =
                                                    match line.as_bytes().first().copied() {
                                                        Some(b'-') => (
                                                            egui::Color32::LIGHT_RED,
                                                            Some(COLOR_DIFF_REMOVED_BG),
                                                        ),
                                                        Some(b'+') => (
                                                            egui::Color32::LIGHT_GREEN,
                                                            Some(COLOR_DIFF_ADDED_BG),
                                                        ),
                                                        _ => (COLOR_TEXT_SECONDARY, None),
                                                    };
                                                let (row, _) = ui.allocate_exact_size(
                                                    egui::vec2(ui.available_width(), row_height),
                                                    egui::Sense::hover(),
                                                );
                                                if let Some(tint) = tint {
                                                    // Cover the row gap too, so adjacent changes form one band.
                                                    ui.painter().rect_filled(
                                                        row.expand2(egui::vec2(0.0, gap / 2.0)),
                                                        0.0,
                                                        tint,
                                                    );
                                                }
                                                let render_line =
                                                    prefix_by_chars(line, MAX_RENDER_CHARS_PER_LINE);
                                                // A left-to-right child keeps indentation; `put` would center.
                                                ui.new_child(
                                                    egui::UiBuilder::new().max_rect(row).layout(
                                                        egui::Layout::left_to_right(
                                                            egui::Align::Center,
                                                        ),
                                                    ),
                                                )
                                                .add(
                                                    egui::Label::new(
                                                        RichText::new(render_line)
                                                            .monospace()
                                                            .color(color),
                                                    )
                                                    .truncate(),
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
        if close_on_escape || !keep_open {
            self.close_diff_modal();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        inline_diff_preview_from_response, InlineDiffPreview, MAX_INLINE_DIFF_BYTES,
        MAX_INLINE_DIFF_LINES,
    };
    use localpaste_core::diff::{unified_diff_lines, DiffResponse};

    #[test]
    fn inline_diff_preview_from_response_classifies_changes_and_identity() {
        assert_eq!(
            inline_diff_preview_from_response(DiffResponse {
                equal: true,
                unified: Vec::new(),
            }),
            InlineDiffPreview::NoChanges
        );

        let changed = inline_diff_preview_from_response(DiffResponse {
            equal: false,
            unified: unified_diff_lines("alpha", "beta"),
        });
        assert!(
            matches!(changed, InlineDiffPreview::Lines(lines) if !lines.is_empty()),
            "changed content should produce inline diff lines"
        );

        assert_eq!(
            InlineDiffPreview::TooLarge {
                lhs_bytes: MAX_INLINE_DIFF_BYTES,
                rhs_bytes: 1,
            },
            InlineDiffPreview::TooLarge {
                lhs_bytes: MAX_INLINE_DIFF_BYTES,
                rhs_bytes: 1,
            }
        );
    }

    #[test]
    fn inline_diff_preview_from_response_trims_line_endings_for_gui_rows() {
        let preview = inline_diff_preview_from_response(DiffResponse {
            equal: false,
            unified: unified_diff_lines("old\n", "new\n"),
        });
        let InlineDiffPreview::Lines(lines) = preview else {
            panic!("changed content should produce diff lines");
        };
        assert!(
            lines
                .iter()
                .all(|line| !line.ends_with('\n') && !line.ends_with('\r')),
            "GUI preview rows should not keep raw trailing newlines"
        );
    }

    #[test]
    fn inline_diff_preview_from_response_caps_large_line_counts() {
        let preview = inline_diff_preview_from_response(DiffResponse {
            equal: false,
            unified: (0..=MAX_INLINE_DIFF_LINES)
                .map(|idx| format!("+line-{idx}\n"))
                .collect(),
        });

        assert_eq!(
            preview,
            InlineDiffPreview::TooManyLines {
                line_count: MAX_INLINE_DIFF_LINES + 1,
            }
        );
    }

    #[test]
    fn too_large_preview_state_is_reserved_for_pre_dispatch_size_gates() {
        assert_eq!(
            InlineDiffPreview::TooLarge {
                lhs_bytes: MAX_INLINE_DIFF_BYTES,
                rhs_bytes: 1,
            },
            InlineDiffPreview::TooLarge {
                lhs_bytes: MAX_INLINE_DIFF_BYTES,
                rhs_bytes: 1,
            }
        );
    }
}
