//! Virtual editor command reducer and mutation application pipeline.

use super::highlight::VirtualEditHint;
use super::virtual_editor::{
    EditIntent, RecordedEdit, VirtualEditDelta, VirtualInputCommand, WrapLayoutCache,
};
use super::{LocalPasteApp, VirtualApplyResult};
use eframe::egui;
use std::ops::Range;
use std::time::Instant;
use tracing::info;

impl LocalPasteApp {
    fn apply_virtual_layout_delta_with_recovery(
        &mut self,
        delta: VirtualEditDelta,
        mut galley_apply_ms: Option<&mut f32>,
    ) -> bool {
        if self
            .virtual_layout
            .apply_delta(&self.virtual_editor_buffer, delta)
        {
            let galley_started = galley_apply_ms.as_ref().map(|_| Instant::now());
            self.virtual_galley_cache
                .apply_delta(delta, self.virtual_editor_buffer.line_count());
            if let (Some(slot), Some(started)) = (galley_apply_ms.as_mut(), galley_started) {
                **slot = started.elapsed().as_secs_f32() * 1000.0;
            }
            return true;
        }
        let rebuilt = self
            .virtual_layout
            .rebuild_with_cached_geometry(&self.virtual_editor_buffer);
        if !rebuilt {
            self.virtual_layout = WrapLayoutCache::default();
        }
        let galley_started = galley_apply_ms.as_ref().map(|_| Instant::now());
        self.virtual_galley_cache.evict_all();
        if let (Some(slot), Some(started)) = (galley_apply_ms.as_mut(), galley_started) {
            **slot = started.elapsed().as_secs_f32() * 1000.0;
        }
        rebuilt
    }

    fn cancel_virtual_ime_preedit_if_active(&mut self, now: Instant) -> bool {
        let had_preedit = self.virtual_editor_state.ime.preedit_range.is_some()
            || !self.virtual_editor_state.ime.preedit_text.is_empty();
        let changed = if let Some(range) = self.virtual_editor_state.ime.preedit_range.take() {
            self.replace_virtual_range(range, "", EditIntent::Other, false, now)
        } else {
            false
        };
        if had_preedit {
            self.virtual_editor_state.ime.preedit_text.clear();
            self.virtual_editor_state.ime.enabled = false;
        }
        changed
    }

    /// Replaces a virtual-editor char range and updates layout/history/perf state.
    ///
    /// # Arguments
    /// - `range`: Global char range to replace.
    /// - `replacement`: Replacement UTF-8 text.
    /// - `intent`: Edit intent used for history coalescing behavior.
    /// - `record_history`: Whether to append this edit to undo history.
    /// - `now`: Timestamp used for edit coalescing and telemetry.
    ///
    /// # Returns
    /// `true` when an edit was applied, otherwise `false`.
    pub(super) fn replace_virtual_range(
        &mut self,
        range: Range<usize>,
        replacement: &str,
        intent: EditIntent,
        record_history: bool,
        now: Instant,
    ) -> bool {
        let start = range.start.min(self.virtual_editor_buffer.len_chars());
        let end = range.end.min(self.virtual_editor_buffer.len_chars());
        if start == end && replacement.is_empty() {
            return false;
        }
        let start_line = self.virtual_editor_buffer.char_to_line_col(start).0;
        let deleted = self.virtual_editor_buffer.slice_chars(start..end);
        let deleted_chars = end.saturating_sub(start);
        let inserted_chars = replacement.chars().count();
        let inserted_newlines = replacement.chars().filter(|ch| *ch == '\n').count();
        let deleted_newlines = deleted.chars().filter(|ch| *ch == '\n').count();
        let touched_lines = inserted_newlines.max(deleted_newlines).saturating_add(1);
        let before_cursor =
            self.clamp_virtual_cursor_for_render(self.virtual_editor_state.cursor());
        let perf_enabled = self.perf_log_enabled;
        let rope_started = perf_enabled.then(Instant::now);
        let delta = self
            .virtual_editor_buffer
            .replace_char_range(start..end, replacement);
        let rope_apply_ms =
            rope_started.map_or(0.0, |started| started.elapsed().as_secs_f32() * 1000.0);
        if let Some(delta) = delta {
            let mut galley_apply_ms = 0.0f32;
            let layout_started = perf_enabled.then(Instant::now);
            let layout_recovered = self.apply_virtual_layout_delta_with_recovery(
                delta,
                perf_enabled.then_some(&mut galley_apply_ms),
            );
            let layout_apply_ms =
                layout_started.map_or(0.0, |started| started.elapsed().as_secs_f32() * 1000.0);
            self.highlight_edit_hint = Some(VirtualEditHint {
                start_line,
                touched_lines,
                inserted_chars,
                deleted_chars,
            });
            if perf_enabled {
                info!(
                    target: "localpaste_gui::perf",
                    event = "virtual_edit_apply",
                    revision = self.virtual_editor_buffer.revision(),
                    start_char = start,
                    end_char = end,
                    inserted_chars = inserted_chars,
                    deleted_chars = deleted_chars,
                    rope_apply_ms = rope_apply_ms,
                    layout_apply_ms = layout_apply_ms,
                    galley_apply_ms = galley_apply_ms,
                    layout_recovered = layout_recovered,
                    "virtual editor mutation + cache patch timings"
                );
            }
        }
        let after_cursor = start.saturating_add(inserted_chars);
        let after_cursor = self.clamp_virtual_cursor_for_render(after_cursor);
        self.virtual_editor_state
            .set_cursor(after_cursor, self.virtual_editor_buffer.len_chars());
        if record_history {
            self.virtual_editor_history.record_edit(RecordedEdit {
                start,
                deleted,
                inserted: replacement.to_string(),
                intent,
                before_cursor,
                after_cursor,
                at: now,
            });
        }
        true
    }

    /// Applies normalized input commands to virtual editor state and buffer.
    ///
    /// # Arguments
    /// - `ctx`: Egui context used for clipboard output commands.
    /// - `commands`: Ordered command slice to apply.
    ///
    /// # Returns
    /// Aggregate mutation/copy/paste flags describing what was applied.
    ///
    /// # Errors
    /// This reducer does not return recoverable errors; command failures are handled in state.
    ///
    /// # Panics
    /// Panics only if underlying buffer/layout invariants are violated.
    pub(super) fn apply_virtual_commands(
        &mut self,
        ctx: &egui::Context,
        commands: &[VirtualInputCommand],
    ) -> VirtualApplyResult {
        if commands.is_empty() {
            return VirtualApplyResult::default();
        }
        let mut result = VirtualApplyResult::default();
        let now = Instant::now();
        for command in commands {
            let cursor_before = self.virtual_editor_state.cursor();
            let changed_before = result.changed;
            match command {
                VirtualInputCommand::SelectAll => {
                    self.virtual_editor_state
                        .select_all(self.virtual_editor_buffer.len_chars());
                }
                VirtualInputCommand::Copy => {
                    if let Some(selection) = self.virtual_selected_text() {
                        ctx.send_cmd(egui::OutputCommand::CopyText(selection));
                        result.copied = true;
                    }
                }
                VirtualInputCommand::Cut => {
                    result.changed |= self.cancel_virtual_ime_preedit_if_active(now);
                    if let Some(range) = self.virtual_editor_state.selection_range() {
                        if let Some(selection) = self.virtual_selected_text() {
                            ctx.send_cmd(egui::OutputCommand::CopyText(selection));
                            result.copied = true;
                        }
                        result.changed |=
                            self.replace_virtual_range(range, "", EditIntent::Cut, true, now);
                        if result.changed {
                            result.cut = true;
                        }
                    }
                }
                VirtualInputCommand::Paste(text) => {
                    result.changed |= self.cancel_virtual_ime_preedit_if_active(now);
                    let cursor = self.virtual_editor_state.cursor();
                    let range = self
                        .virtual_editor_state
                        .selection_range()
                        .unwrap_or(cursor..cursor);
                    result.changed |=
                        self.replace_virtual_range(range, text, EditIntent::Paste, true, now);
                    if !text.is_empty() {
                        result.pasted = true;
                    }
                }
                VirtualInputCommand::InsertText(text) => {
                    if text.is_empty() {
                        continue;
                    }
                    if self.virtual_editor_state.ime.preedit_range.is_some() {
                        continue;
                    }
                    let cursor = self.virtual_editor_state.cursor();
                    let range = self
                        .virtual_editor_state
                        .selection_range()
                        .unwrap_or(cursor..cursor);
                    result.changed |=
                        self.replace_virtual_range(range, text, EditIntent::Insert, true, now);
                    self.virtual_editor_state.clear_preferred_column();
                }
                VirtualInputCommand::InsertNewline => {
                    result.changed |= self.cancel_virtual_ime_preedit_if_active(now);
                    let cursor = self.virtual_editor_state.cursor();
                    let range = self
                        .virtual_editor_state
                        .selection_range()
                        .unwrap_or(cursor..cursor);
                    result.changed |=
                        self.replace_virtual_range(range, "\n", EditIntent::Insert, true, now);
                    self.virtual_editor_state.clear_preferred_column();
                }
                VirtualInputCommand::InsertTab => {
                    result.changed |= self.cancel_virtual_ime_preedit_if_active(now);
                    let cursor = self.virtual_editor_state.cursor();
                    let range = self
                        .virtual_editor_state
                        .selection_range()
                        .unwrap_or(cursor..cursor);
                    result.changed |=
                        self.replace_virtual_range(range, "    ", EditIntent::Insert, true, now);
                    self.virtual_editor_state.clear_preferred_column();
                }
                VirtualInputCommand::Backspace { word } => {
                    result.changed |= self.cancel_virtual_ime_preedit_if_active(now);
                    if let Some(range) = self.virtual_editor_state.selection_range() {
                        result.changed |= self.replace_virtual_range(
                            range,
                            "",
                            EditIntent::DeleteBackward,
                            true,
                            now,
                        );
                    } else {
                        let cursor = self.virtual_editor_state.cursor();
                        if cursor == 0 {
                            continue;
                        }
                        let start = if *word {
                            self.virtual_word_left(cursor)
                        } else {
                            cursor.saturating_sub(1)
                        };
                        result.changed |= self.replace_virtual_range(
                            start..cursor,
                            "",
                            EditIntent::DeleteBackward,
                            true,
                            now,
                        );
                    }
                    self.virtual_editor_state.clear_preferred_column();
                }
                VirtualInputCommand::DeleteForward { word } => {
                    result.changed |= self.cancel_virtual_ime_preedit_if_active(now);
                    if let Some(range) = self.virtual_editor_state.selection_range() {
                        result.changed |= self.replace_virtual_range(
                            range,
                            "",
                            EditIntent::DeleteForward,
                            true,
                            now,
                        );
                    } else {
                        let cursor = self.virtual_editor_state.cursor();
                        let end = if *word {
                            self.virtual_word_right_end(cursor)
                        } else {
                            cursor
                                .saturating_add(1)
                                .min(self.virtual_editor_buffer.len_chars())
                        };
                        if end > cursor {
                            result.changed |= self.replace_virtual_range(
                                cursor..end,
                                "",
                                EditIntent::DeleteForward,
                                true,
                                now,
                            );
                        }
                    }
                    self.virtual_editor_state.clear_preferred_column();
                }
                VirtualInputCommand::DeleteToLineStart => {
                    result.changed |= self.cancel_virtual_ime_preedit_if_active(now);
                    if let Some(range) = self.virtual_editor_state.selection_range() {
                        result.changed |= self.replace_virtual_range(
                            range,
                            "",
                            EditIntent::DeleteBackward,
                            true,
                            now,
                        );
                    } else {
                        let cursor = self.virtual_editor_state.cursor();
                        let (line, _) = self.virtual_editor_buffer.char_to_line_col(cursor);
                        let start = self.virtual_editor_buffer.line_col_to_char(line, 0);
                        if start < cursor {
                            result.changed |= self.replace_virtual_range(
                                start..cursor,
                                "",
                                EditIntent::DeleteBackward,
                                true,
                                now,
                            );
                        }
                    }
                    self.virtual_editor_state.clear_preferred_column();
                }
                VirtualInputCommand::DeleteToLineEnd => {
                    result.changed |= self.cancel_virtual_ime_preedit_if_active(now);
                    if let Some(range) = self.virtual_editor_state.selection_range() {
                        result.changed |= self.replace_virtual_range(
                            range,
                            "",
                            EditIntent::DeleteForward,
                            true,
                            now,
                        );
                    } else {
                        let cursor = self.virtual_editor_state.cursor();
                        let (line, _) = self.virtual_editor_buffer.char_to_line_col(cursor);
                        let end = self
                            .virtual_editor_buffer
                            .line_col_to_char(line, self.virtual_line_render_chars(line));
                        if end > cursor {
                            result.changed |= self.replace_virtual_range(
                                cursor..end,
                                "",
                                EditIntent::DeleteForward,
                                true,
                                now,
                            );
                        }
                    }
                    self.virtual_editor_state.clear_preferred_column();
                }

                VirtualInputCommand::MoveLeft { select, word } => {
                    let cursor = self.virtual_editor_state.cursor();
                    let target = if !select {
                        if let Some(range) = self.virtual_editor_state.selection_range() {
                            range.start
                        } else if *word {
                            self.virtual_word_left(cursor)
                        } else {
                            cursor.saturating_sub(1)
                        }
                    } else if *word {
                        self.virtual_word_left(cursor)
                    } else {
                        cursor.saturating_sub(1)
                    };
                    let target = self.clamp_virtual_cursor_for_render(target);
                    self.virtual_editor_state.move_cursor(
                        target,
                        self.virtual_editor_buffer.len_chars(),
                        *select,
                    );
                    self.virtual_editor_state.clear_preferred_column();
                }
                VirtualInputCommand::MoveRight { select, word } => {
                    let cursor = self.virtual_editor_state.cursor();
                    let target = if !select {
                        if let Some(range) = self.virtual_editor_state.selection_range() {
                            range.end
                        } else if *word {
                            self.virtual_word_right(cursor)
                        } else {
                            cursor
                                .saturating_add(1)
                                .min(self.virtual_editor_buffer.len_chars())
                        }
                    } else if *word {
                        self.virtual_word_right(cursor)
                    } else {
                        cursor
                            .saturating_add(1)
                            .min(self.virtual_editor_buffer.len_chars())
                    };
                    let target = self.clamp_virtual_cursor_for_render(target);
                    self.virtual_editor_state.move_cursor(
                        target,
                        self.virtual_editor_buffer.len_chars(),
                        *select,
                    );
                    self.virtual_editor_state.clear_preferred_column();
                }
                VirtualInputCommand::MoveHome { select } => {
                    let (target, _) =
                        self.virtual_visual_row_bounds(self.virtual_editor_state.cursor());
                    let target = self.clamp_virtual_cursor_for_render(target);
                    self.virtual_editor_state.move_cursor(
                        target,
                        self.virtual_editor_buffer.len_chars(),
                        *select,
                    );
                    self.virtual_editor_state.clear_preferred_column();
                }
                VirtualInputCommand::MoveEnd { select } => {
                    let (_, target) =
                        self.virtual_visual_row_bounds(self.virtual_editor_state.cursor());
                    let target = self.clamp_virtual_cursor_for_render(target);
                    self.virtual_editor_state.move_cursor(
                        target,
                        self.virtual_editor_buffer.len_chars(),
                        *select,
                    );
                    self.virtual_editor_state.clear_preferred_column();
                }
                VirtualInputCommand::MoveLineHome { select } => {
                    let (line, _) = self
                        .virtual_editor_buffer
                        .char_to_line_col(self.virtual_editor_state.cursor());
                    let target = self.clamp_virtual_cursor_for_render(
                        self.virtual_editor_buffer.line_col_to_char(line, 0),
                    );
                    self.virtual_editor_state.move_cursor(
                        target,
                        self.virtual_editor_buffer.len_chars(),
                        *select,
                    );
                    self.virtual_editor_state.clear_preferred_column();
                }
                VirtualInputCommand::MoveLineEnd { select } => {
                    let (line, _) = self
                        .virtual_editor_buffer
                        .char_to_line_col(self.virtual_editor_state.cursor());
                    let target = self.clamp_virtual_cursor_for_render(
                        self.virtual_editor_buffer.line_col_to_char(
                            line,
                            self.virtual_editor_buffer.line_len_chars(line),
                        ),
                    );
                    self.virtual_editor_state.move_cursor(
                        target,
                        self.virtual_editor_buffer.len_chars(),
                        *select,
                    );
                    self.virtual_editor_state.clear_preferred_column();
                }
                VirtualInputCommand::MoveDocHome { select } => {
                    let target = self.clamp_virtual_cursor_for_render(0);
                    self.virtual_editor_state.move_cursor(
                        target,
                        self.virtual_editor_buffer.len_chars(),
                        *select,
                    );
                    self.virtual_editor_state.clear_preferred_column();
                }
                VirtualInputCommand::MoveDocEnd { select } => {
                    let target = self
                        .clamp_virtual_cursor_for_render(self.virtual_editor_buffer.len_chars());
                    self.virtual_editor_state.move_cursor(
                        target,
                        self.virtual_editor_buffer.len_chars(),
                        *select,
                    );
                    self.virtual_editor_state.clear_preferred_column();
                }

                VirtualInputCommand::MoveUp { select } => {
                    let preferred =
                        self.virtual_editor_state
                            .preferred_column()
                            .unwrap_or_else(|| {
                                self.virtual_preferred_column_for_cursor(
                                    self.virtual_editor_state.cursor(),
                                )
                            });
                    self.virtual_editor_state.set_preferred_column(preferred);
                    let affinity = self.virtual_editor_state.wrap_boundary_affinity();
                    let target = self.virtual_move_vertical_target(
                        self.virtual_editor_state.cursor(),
                        preferred,
                        true,
                        affinity,
                    );
                    let target = self.clamp_virtual_cursor_for_render(target);
                    self.virtual_editor_state.move_cursor(
                        target,
                        self.virtual_editor_buffer.len_chars(),
                        *select,
                    );
                    self.virtual_editor_state.set_wrap_boundary_affinity(
                        self.vertical_boundary_affinity_for_target(target, preferred, true),
                    );
                }
                VirtualInputCommand::MoveDown { select } => {
                    let preferred =
                        self.virtual_editor_state
                            .preferred_column()
                            .unwrap_or_else(|| {
                                self.virtual_preferred_column_for_cursor(
                                    self.virtual_editor_state.cursor(),
                                )
                            });
                    self.virtual_editor_state.set_preferred_column(preferred);
                    let affinity = self.virtual_editor_state.wrap_boundary_affinity();
                    let target = self.virtual_move_vertical_target(
                        self.virtual_editor_state.cursor(),
                        preferred,
                        false,
                        affinity,
                    );
                    let target = self.clamp_virtual_cursor_for_render(target);
                    self.virtual_editor_state.move_cursor(
                        target,
                        self.virtual_editor_buffer.len_chars(),
                        *select,
                    );
                    self.virtual_editor_state.set_wrap_boundary_affinity(
                        self.vertical_boundary_affinity_for_target(target, preferred, false),
                    );
                }
                VirtualInputCommand::PageUp { select } => {
                    let rows = ((self.virtual_viewport_height / self.virtual_line_height.max(1.0))
                        .floor() as usize)
                        .max(1);
                    let mut target = self.virtual_editor_state.cursor();
                    let preferred = self
                        .virtual_editor_state
                        .preferred_column()
                        .unwrap_or_else(|| self.virtual_preferred_column_for_cursor(target));
                    self.virtual_editor_state.set_preferred_column(preferred);
                    let mut affinity = self.virtual_editor_state.wrap_boundary_affinity();
                    for _ in 0..rows {
                        target =
                            self.virtual_move_vertical_target(target, preferred, true, affinity);
                        target = self.clamp_virtual_cursor_for_render(target);
                        affinity =
                            self.vertical_boundary_affinity_for_target(target, preferred, true);
                        if target == 0 {
                            break;
                        }
                    }
                    self.virtual_editor_state.move_cursor(
                        target,
                        self.virtual_editor_buffer.len_chars(),
                        *select,
                    );
                    self.virtual_editor_state
                        .set_wrap_boundary_affinity(affinity);
                }
                VirtualInputCommand::PageDown { select } => {
                    let rows = ((self.virtual_viewport_height / self.virtual_line_height.max(1.0))
                        .floor() as usize)
                        .max(1);
                    let mut target = self.virtual_editor_state.cursor();
                    let preferred = self
                        .virtual_editor_state
                        .preferred_column()
                        .unwrap_or_else(|| self.virtual_preferred_column_for_cursor(target));
                    self.virtual_editor_state.set_preferred_column(preferred);
                    let mut affinity = self.virtual_editor_state.wrap_boundary_affinity();
                    for _ in 0..rows {
                        let next =
                            self.virtual_move_vertical_target(target, preferred, false, affinity);
                        target = self.clamp_virtual_cursor_for_render(next);
                        affinity =
                            self.vertical_boundary_affinity_for_target(target, preferred, false);
                        if target == self.virtual_editor_buffer.len_chars() {
                            break;
                        }
                    }
                    self.virtual_editor_state.move_cursor(
                        target,
                        self.virtual_editor_buffer.len_chars(),
                        *select,
                    );
                    self.virtual_editor_state
                        .set_wrap_boundary_affinity(affinity);
                }
                VirtualInputCommand::Undo => {
                    result.changed |= self.cancel_virtual_ime_preedit_if_active(now);
                    let delta = self.virtual_editor_history.undo(
                        &mut self.virtual_editor_buffer,
                        &mut self.virtual_editor_state,
                    );
                    if let Some(delta) = delta {
                        result.changed = true;
                        let _layout_ok = self.apply_virtual_layout_delta_with_recovery(delta, None);
                        let _cursor_clamped = self.clamp_virtual_cursor_state_for_render();
                        self.highlight_edit_hint = None;
                    }
                }
                VirtualInputCommand::Redo => {
                    result.changed |= self.cancel_virtual_ime_preedit_if_active(now);
                    let delta = self.virtual_editor_history.redo(
                        &mut self.virtual_editor_buffer,
                        &mut self.virtual_editor_state,
                    );
                    if let Some(delta) = delta {
                        result.changed = true;
                        let _layout_ok = self.apply_virtual_layout_delta_with_recovery(delta, None);
                        let _cursor_clamped = self.clamp_virtual_cursor_state_for_render();
                        self.highlight_edit_hint = None;
                    }
                }
                VirtualInputCommand::ImeEnabled => {
                    self.virtual_editor_state.ime.enabled = true;
                }
                VirtualInputCommand::ImePreedit(text) => {
                    self.virtual_editor_state.ime.enabled = true;
                    let existing_preedit_range =
                        self.virtual_editor_state.ime.preedit_range.clone();
                    if text.is_empty() && existing_preedit_range.is_none() {
                        self.virtual_editor_state.ime.preedit_text.clear();
                        continue;
                    }
                    let cursor = self.virtual_editor_state.cursor();
                    let range = existing_preedit_range
                        .clone()
                        .or_else(|| self.virtual_editor_state.selection_range())
                        .unwrap_or(cursor..cursor);
                    if self.virtual_editor_state.ime.preedit_text == *text
                        && existing_preedit_range.as_ref() == Some(&range)
                    {
                        continue;
                    }
                    result.changed |= self.replace_virtual_range(
                        range.clone(),
                        text,
                        EditIntent::Other,
                        false,
                        now,
                    );
                    self.virtual_editor_state.clear_preferred_column();
                    if text.is_empty() {
                        self.virtual_editor_state.ime.preedit_range = None;
                        self.virtual_editor_state.ime.preedit_text.clear();
                        continue;
                    }
                    let end = range.start.saturating_add(text.chars().count());
                    self.virtual_editor_state.ime.preedit_range = Some(range.start..end);
                    self.virtual_editor_state.ime.preedit_text = text.clone();
                }
                VirtualInputCommand::ImeCommit(text) => {
                    let cursor = self.virtual_editor_state.cursor();
                    let range = self
                        .virtual_editor_state
                        .ime
                        .preedit_range
                        .clone()
                        .or_else(|| self.virtual_editor_state.selection_range())
                        .unwrap_or(cursor..cursor);
                    result.changed |=
                        self.replace_virtual_range(range, text, EditIntent::ImeCommit, true, now);
                    self.virtual_editor_state.ime.preedit_range = None;
                    self.virtual_editor_state.ime.preedit_text.clear();
                    self.virtual_editor_state.ime.enabled = false;
                    self.virtual_editor_state.clear_preferred_column();
                }
                VirtualInputCommand::ImeDisabled => {
                    result.changed |= self.cancel_virtual_ime_preedit_if_active(now);
                    self.virtual_editor_state.ime.enabled = false;
                    self.virtual_editor_state.ime.preedit_text.clear();
                    self.virtual_editor_state.clear_preferred_column();
                }
            }
            if self.virtual_editor_state.cursor() != cursor_before
                || result.changed != changed_before
            {
                self.reset_virtual_caret_blink();
            }
        }
        result
    }
}
