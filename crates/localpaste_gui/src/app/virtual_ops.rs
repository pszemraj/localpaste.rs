//! Virtual editor operations: selection, cursor motion, editing, and command application.

use super::editor::EditorMode;
use super::highlight::VirtualEditHint;
use super::util::word_range_at;
use super::virtual_editor::{
    EditIntent, RecordedEdit, VirtualEditorHistory, VirtualEditorState, VirtualGalleyCache,
    VirtualInputCommand, WrapLayoutCache,
};
use super::{
    is_editor_word_char, next_virtual_click_count, LocalPasteApp, VirtualApplyResult,
    EDITOR_DOUBLE_CLICK_DISTANCE, EDITOR_DOUBLE_CLICK_WINDOW,
};
use eframe::egui::{
    self,
    text::{CCursor, CCursorRange},
    text_edit::TextEditOutput,
};
use std::ops::Range;
use std::time::Instant;
use tracing::info;

impl LocalPasteApp {
    pub(super) fn handle_large_editor_click(
        &mut self,
        output: &TextEditOutput,
        text: &str,
        is_large_buffer: bool,
    ) {
        if !is_large_buffer || !output.response.clicked() {
            return;
        }
        let now = Instant::now();
        let click_pos = output.response.interact_pointer_pos();
        let continued = if let (Some(last_at), Some(last_pos), Some(pos)) = (
            self.last_editor_click_at,
            self.last_editor_click_pos,
            click_pos,
        ) {
            now.duration_since(last_at) <= EDITOR_DOUBLE_CLICK_WINDOW
                && last_pos.distance(pos) <= EDITOR_DOUBLE_CLICK_DISTANCE
        } else {
            false
        };
        if continued {
            self.last_editor_click_count = self.last_editor_click_count.saturating_add(1).min(3);
        } else {
            self.last_editor_click_count = 1;
        }
        self.last_editor_click_at = Some(now);
        self.last_editor_click_pos = click_pos;

        let Some(range) = output.cursor_range else {
            return;
        };
        let mut state = output.state.clone();
        match self.last_editor_click_count {
            2 => {
                let Some((start, end)) = word_range_at(text, range.primary.index) else {
                    return;
                };
                state.cursor.set_char_range(Some(CCursorRange::two(
                    CCursor::new(start),
                    CCursor::new(end),
                )));
            }
            3 => {
                let (start, end) = self.selected_content.line_range_chars(range.primary.index);
                state.cursor.set_char_range(Some(CCursorRange::two(
                    CCursor::new(start),
                    CCursor::new(end),
                )));
            }
            _ => return,
        }
        state.store(&output.response.ctx, output.response.id);
    }

    pub(super) fn virtual_selection_text(&mut self) -> Option<String> {
        let (start, end) = self.virtual_selection.selection_bounds()?;
        let text = self.selected_content.as_str();
        self.editor_lines
            .ensure_for(self.selected_content.revision(), text);
        let mut out = String::new();
        for line_idx in start.line..=end.line {
            let line = self.editor_lines.line_without_newline(text, line_idx);
            let line_chars = line.chars().count();
            let start_char = if line_idx == start.line {
                start.column.min(line_chars)
            } else {
                0
            };
            let end_char = if line_idx == end.line {
                end.column.min(line_chars)
            } else {
                line_chars
            };
            if start_char < end_char {
                let start_byte =
                    egui::text_selection::text_cursor_state::byte_index_from_char_index(
                        line, start_char,
                    );
                let end_byte = egui::text_selection::text_cursor_state::byte_index_from_char_index(
                    line, end_char,
                );
                out.push_str(&line[start_byte..end_byte]);
            }
            if line_idx < end.line {
                out.push('\n');
            }
        }
        if out.is_empty() {
            None
        } else {
            Some(out)
        }
    }

    pub(super) fn is_virtual_editor_mode(&self) -> bool {
        self.editor_mode == EditorMode::VirtualEditor
    }

    pub(super) fn active_text_len_bytes(&self) -> usize {
        if self.is_virtual_editor_mode() {
            self.virtual_editor_buffer.len_bytes()
        } else {
            self.selected_content.len()
        }
    }

    pub(super) fn active_text_chars(&self) -> usize {
        if self.is_virtual_editor_mode() {
            self.virtual_editor_buffer.len_chars()
        } else {
            self.selected_content.chars_len()
        }
    }

    pub(super) fn active_revision(&self) -> u64 {
        if self.is_virtual_editor_mode() {
            self.virtual_editor_buffer.revision()
        } else {
            self.selected_content.revision()
        }
    }

    pub(super) fn active_snapshot(&self) -> String {
        if self.is_virtual_editor_mode() {
            self.virtual_editor_buffer.to_string()
        } else {
            self.selected_content.to_string()
        }
    }

    pub(super) fn reset_virtual_editor(&mut self, text: &str) {
        self.virtual_editor_buffer.reset(text);
        self.virtual_editor_state = VirtualEditorState::default();
        self.virtual_editor_history = VirtualEditorHistory::default();
        self.virtual_layout = WrapLayoutCache::default();
        self.virtual_galley_cache = VirtualGalleyCache::default();
        self.virtual_line_scratch.clear();
        self.reset_virtual_caret_blink();
        self.highlight_edit_hint = None;
        self.virtual_drag_active = false;
        self.reset_virtual_click_streak();
    }

    pub(super) fn reset_virtual_caret_blink(&mut self) {
        self.virtual_caret_phase_start = Instant::now();
    }

    pub(super) fn reset_virtual_click_streak(&mut self) {
        self.last_virtual_click_at = None;
        self.last_virtual_click_pos = None;
        self.last_virtual_click_line = None;
        self.last_virtual_click_count = 0;
    }

    pub(super) fn register_virtual_click(
        &mut self,
        line_idx: usize,
        pointer_pos: egui::Pos2,
    ) -> u8 {
        let now = Instant::now();
        let count = next_virtual_click_count(
            self.last_virtual_click_at,
            self.last_virtual_click_pos,
            self.last_virtual_click_line,
            self.last_virtual_click_count,
            line_idx,
            pointer_pos,
            now,
        );
        self.last_virtual_click_at = Some(now);
        self.last_virtual_click_pos = Some(pointer_pos);
        self.last_virtual_click_line = Some(line_idx);
        self.last_virtual_click_count = count;
        count
    }

    pub(super) fn virtual_selected_text(&self) -> Option<String> {
        let range = self.virtual_editor_state.selection_range()?;
        Some(self.virtual_editor_buffer.slice_chars(range))
    }

    pub(super) fn virtual_select_line(&mut self, line_idx: usize) {
        let line_count = self.virtual_editor_buffer.line_count();
        if line_idx >= line_count {
            return;
        }
        let start = self.virtual_editor_buffer.line_col_to_char(line_idx, 0);
        let line_len = self.virtual_editor_buffer.line_len_chars(line_idx);
        let end_without_newline = self
            .virtual_editor_buffer
            .line_col_to_char(line_idx, line_len);
        let end = if line_idx + 1 < line_count {
            self.virtual_editor_buffer
                .rope()
                .line_to_char(line_idx + 1)
                .max(end_without_newline)
        } else {
            end_without_newline
        };

        self.virtual_editor_state
            .set_cursor(start, self.virtual_editor_buffer.len_chars());
        self.virtual_editor_state
            .move_cursor(end, self.virtual_editor_buffer.len_chars(), true);
        self.virtual_editor_state.clear_preferred_column();
    }

    pub(super) fn virtual_word_left(&self, cursor: usize) -> usize {
        if cursor == 0 {
            return 0;
        }
        let rope = self.virtual_editor_buffer.rope();
        let mut idx = cursor.min(self.virtual_editor_buffer.len_chars());
        while idx > 0 && rope.char(idx - 1).is_whitespace() {
            idx -= 1;
        }
        if idx == 0 {
            return 0;
        }
        let kind = is_editor_word_char(rope.char(idx - 1));
        while idx > 0 && is_editor_word_char(rope.char(idx - 1)) == kind {
            idx -= 1;
        }
        idx
    }

    pub(super) fn virtual_word_right(&self, cursor: usize) -> usize {
        let rope = self.virtual_editor_buffer.rope();
        let len = self.virtual_editor_buffer.len_chars();
        let mut idx = cursor.min(len);
        while idx < len && rope.char(idx).is_whitespace() {
            idx += 1;
        }
        if idx >= len {
            return len;
        }
        let kind = is_editor_word_char(rope.char(idx));
        while idx < len && is_editor_word_char(rope.char(idx)) == kind {
            idx += 1;
        }
        idx
    }

    pub(super) fn virtual_move_vertical_target(
        &self,
        cursor: usize,
        desired_col_in_row: usize,
        up: bool,
    ) -> usize {
        let cols = self.virtual_layout.wrap_columns().max(1);
        let (line, col) = self.virtual_editor_buffer.char_to_line_col(cursor);
        let cursor_display_col =
            self.virtual_layout
                .line_char_to_display_column(&self.virtual_editor_buffer, line, col);
        let rows = self.virtual_layout.line_visual_rows(line).max(1);
        let row = (cursor_display_col / cols).min(rows.saturating_sub(1));
        let line_count = self.virtual_editor_buffer.line_count();

        let target_line_and_row: Option<(usize, usize)> = if up {
            if row > 0 {
                Some((line, row - 1))
            } else if line > 0 {
                let prev_line = line - 1;
                let prev_rows = self.virtual_layout.line_visual_rows(prev_line).max(1);
                Some((prev_line, prev_rows.saturating_sub(1)))
            } else {
                None
            }
        } else if row + 1 < rows {
            Some((line, row + 1))
        } else if line + 1 < line_count {
            Some((line + 1, 0usize))
        } else {
            None
        };

        let Some((target_line, target_row)) = target_line_and_row else {
            return if up {
                0
            } else {
                self.virtual_editor_buffer.len_chars()
            };
        };
        let target_line_cols = self
            .virtual_layout
            .line_columns(&self.virtual_editor_buffer, target_line);
        let row_start = target_row.saturating_mul(cols);
        let desired_col_in_row = desired_col_in_row.min(cols.saturating_sub(1));
        let target_display_col = if row_start >= target_line_cols {
            target_line_cols
        } else {
            row_start
                + desired_col_in_row
                    .min(target_line_cols.saturating_sub(row_start).saturating_sub(1))
        };
        let target_line_char = self.virtual_layout.line_display_column_to_char(
            &self.virtual_editor_buffer,
            target_line,
            target_display_col,
        );
        self.virtual_editor_buffer
            .line_col_to_char(target_line, target_line_char)
    }

    pub(super) fn virtual_selection_for_line(
        &self,
        line_start: usize,
        line_chars: usize,
    ) -> Option<Range<usize>> {
        let range = self.virtual_editor_state.selection_range()?;
        let line_end = line_start.saturating_add(line_chars);
        if range.end <= line_start || range.start >= line_end {
            return None;
        }
        let local_start = range.start.saturating_sub(line_start).min(line_chars);
        let local_end = range.end.saturating_sub(line_start).min(line_chars);
        if local_start >= local_end {
            return None;
        }
        Some(local_start..local_end)
    }

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
        let before_cursor = self.virtual_editor_state.cursor();
        let perf_enabled = self.perf_log_enabled;
        let rope_started = perf_enabled.then(Instant::now);
        let delta = self
            .virtual_editor_buffer
            .replace_char_range(start..end, replacement);
        let rope_apply_ms = rope_started
            .map(|started| started.elapsed().as_secs_f32() * 1000.0)
            .unwrap_or(0.0);
        if let Some(delta) = delta {
            let layout_started = perf_enabled.then(Instant::now);
            let _ = self
                .virtual_layout
                .apply_delta(&self.virtual_editor_buffer, delta);
            let layout_apply_ms = layout_started
                .map(|started| started.elapsed().as_secs_f32() * 1000.0)
                .unwrap_or(0.0);
            let galley_started = perf_enabled.then(Instant::now);
            self.virtual_galley_cache
                .apply_delta(delta, self.virtual_editor_buffer.line_count());
            let galley_apply_ms = galley_started
                .map(|started| started.elapsed().as_secs_f32() * 1000.0)
                .unwrap_or(0.0);
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
                    "virtual editor mutation + cache patch timings"
                );
            }
        }
        let after_cursor = start.saturating_add(inserted_chars);
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
                            self.virtual_word_right(cursor)
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
                    self.virtual_editor_state.move_cursor(
                        target,
                        self.virtual_editor_buffer.len_chars(),
                        *select,
                    );
                    self.virtual_editor_state.clear_preferred_column();
                }
                VirtualInputCommand::MoveHome { select } => {
                    let (line, _) = self
                        .virtual_editor_buffer
                        .char_to_line_col(self.virtual_editor_state.cursor());
                    let target = self.virtual_editor_buffer.line_col_to_char(line, 0);
                    self.virtual_editor_state.move_cursor(
                        target,
                        self.virtual_editor_buffer.len_chars(),
                        *select,
                    );
                    self.virtual_editor_state.clear_preferred_column();
                }
                VirtualInputCommand::MoveEnd { select } => {
                    let (line, _) = self
                        .virtual_editor_buffer
                        .char_to_line_col(self.virtual_editor_state.cursor());
                    let target = self
                        .virtual_editor_buffer
                        .line_col_to_char(line, self.virtual_editor_buffer.line_len_chars(line));
                    self.virtual_editor_state.move_cursor(
                        target,
                        self.virtual_editor_buffer.len_chars(),
                        *select,
                    );
                    self.virtual_editor_state.clear_preferred_column();
                }
                VirtualInputCommand::MoveUp { select } => {
                    let (line, col) = self
                        .virtual_editor_buffer
                        .char_to_line_col(self.virtual_editor_state.cursor());
                    let col = self.virtual_layout.line_char_to_display_column(
                        &self.virtual_editor_buffer,
                        line,
                        col,
                    );
                    let cols = self.virtual_layout.wrap_columns().max(1);
                    let preferred = self
                        .virtual_editor_state
                        .preferred_column()
                        .unwrap_or(col % cols);
                    self.virtual_editor_state.set_preferred_column(preferred);
                    let target = self.virtual_move_vertical_target(
                        self.virtual_editor_state.cursor(),
                        preferred,
                        true,
                    );
                    self.virtual_editor_state.move_cursor(
                        target,
                        self.virtual_editor_buffer.len_chars(),
                        *select,
                    );
                }
                VirtualInputCommand::MoveDown { select } => {
                    let (line, col) = self
                        .virtual_editor_buffer
                        .char_to_line_col(self.virtual_editor_state.cursor());
                    let col = self.virtual_layout.line_char_to_display_column(
                        &self.virtual_editor_buffer,
                        line,
                        col,
                    );
                    let cols = self.virtual_layout.wrap_columns().max(1);
                    let preferred = self
                        .virtual_editor_state
                        .preferred_column()
                        .unwrap_or(col % cols);
                    self.virtual_editor_state.set_preferred_column(preferred);
                    let target = self.virtual_move_vertical_target(
                        self.virtual_editor_state.cursor(),
                        preferred,
                        false,
                    );
                    self.virtual_editor_state.move_cursor(
                        target,
                        self.virtual_editor_buffer.len_chars(),
                        *select,
                    );
                }
                VirtualInputCommand::PageUp { select } => {
                    let rows = ((self.virtual_viewport_height / self.virtual_line_height.max(1.0))
                        .floor() as usize)
                        .max(1);
                    let mut target = self.virtual_editor_state.cursor();
                    let preferred = self.virtual_editor_state.preferred_column().unwrap_or(0);
                    self.virtual_editor_state.set_preferred_column(preferred);
                    for _ in 0..rows {
                        target = self.virtual_move_vertical_target(target, preferred, true);
                        if target == 0 {
                            break;
                        }
                    }
                    self.virtual_editor_state.move_cursor(
                        target,
                        self.virtual_editor_buffer.len_chars(),
                        *select,
                    );
                }
                VirtualInputCommand::PageDown { select } => {
                    let rows = ((self.virtual_viewport_height / self.virtual_line_height.max(1.0))
                        .floor() as usize)
                        .max(1);
                    let mut target = self.virtual_editor_state.cursor();
                    let preferred = self.virtual_editor_state.preferred_column().unwrap_or(0);
                    self.virtual_editor_state.set_preferred_column(preferred);
                    for _ in 0..rows {
                        let next = self.virtual_move_vertical_target(target, preferred, false);
                        target = next;
                        if target == self.virtual_editor_buffer.len_chars() {
                            break;
                        }
                    }
                    self.virtual_editor_state.move_cursor(
                        target,
                        self.virtual_editor_buffer.len_chars(),
                        *select,
                    );
                }
                VirtualInputCommand::Undo => {
                    self.virtual_editor_state.ime.preedit_range = None;
                    self.virtual_editor_state.ime.preedit_text.clear();
                    let delta = self.virtual_editor_history.undo(
                        &mut self.virtual_editor_buffer,
                        &mut self.virtual_editor_state,
                    );
                    if let Some(delta) = delta {
                        result.changed = true;
                        let _ = self
                            .virtual_layout
                            .apply_delta(&self.virtual_editor_buffer, delta);
                        self.virtual_galley_cache
                            .apply_delta(delta, self.virtual_editor_buffer.line_count());
                        self.highlight_edit_hint = None;
                    }
                }
                VirtualInputCommand::Redo => {
                    let delta = self.virtual_editor_history.redo(
                        &mut self.virtual_editor_buffer,
                        &mut self.virtual_editor_state,
                    );
                    if let Some(delta) = delta {
                        result.changed = true;
                        let _ = self
                            .virtual_layout
                            .apply_delta(&self.virtual_editor_buffer, delta);
                        self.virtual_galley_cache
                            .apply_delta(delta, self.virtual_editor_buffer.line_count());
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
                    self.virtual_editor_state.ime.enabled = false;
                    if let Some(range) = self.virtual_editor_state.ime.preedit_range.take() {
                        result.changed |=
                            self.replace_virtual_range(range, "", EditIntent::Other, false, now);
                    }
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
