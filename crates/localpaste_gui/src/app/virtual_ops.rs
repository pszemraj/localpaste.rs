//! Virtual editor operations: selection, cursor motion, editing, and command application.

use super::highlight::VirtualEditHint;
use super::util::word_range_at;
use super::virtual_editor::{
    EditIntent, RecordedEdit, VirtualEditDelta, VirtualEditorHistory, VirtualEditorState,
    VirtualGalleyCache, VirtualInputCommand, WrapBoundaryAffinity, WrapLayoutCache,
};
use super::{
    is_editor_word_char, next_virtual_click_count, LocalPasteApp, VirtualApplyResult,
    EDITOR_DOUBLE_CLICK_DISTANCE, EDITOR_DOUBLE_CLICK_WINDOW, MAX_RENDER_CHARS_PER_LINE,
};
use eframe::egui::{
    self,
    text::{CCursor, CCursorRange},
    text_edit::TextEditOutput,
};
use std::ops::Range;
use std::time::Instant;
use tracing::info;

#[derive(Clone, Copy, Debug)]
struct VirtualCursorWrapMetrics {
    line: usize,
    display_col: usize,
    line_cols: usize,
    wrap_cols: usize,
}

fn is_internal_wrap_boundary(display_col: usize, wrap_cols: usize, line_cols: usize) -> bool {
    display_col > 0 && display_col % wrap_cols == 0 && display_col < line_cols
}

impl LocalPasteApp {
    fn clamp_virtual_cursor_state_for_render(&mut self) -> bool {
        let cursor = self.virtual_editor_state.cursor();
        let clamped = self.clamp_virtual_cursor_for_render(cursor);
        if clamped == cursor {
            return false;
        }
        self.virtual_editor_state
            .set_cursor(clamped, self.virtual_editor_buffer.len_chars());
        true
    }

    fn virtual_line_render_chars(&self, line: usize) -> usize {
        let full_chars = self.virtual_editor_buffer.line_len_chars(line);
        let cached_chars = self.virtual_layout.line_chars(line);
        if cached_chars == 0 {
            full_chars.min(MAX_RENDER_CHARS_PER_LINE)
        } else {
            cached_chars.min(full_chars)
        }
    }

    fn virtual_line_render_boundary(&self, idx: usize) -> (usize, bool) {
        let line = self.virtual_editor_buffer.char_to_line_col(idx).0;
        let render_chars = self.virtual_line_render_chars(line);
        let render_end = self
            .virtual_editor_buffer
            .line_col_to_char(line, render_chars);
        (
            render_end,
            render_chars < self.virtual_editor_buffer.line_len_chars(line),
        )
    }

    pub(super) fn clamp_virtual_cursor_for_render(&self, char_index: usize) -> usize {
        let clamped = char_index.min(self.virtual_editor_buffer.len_chars());
        let (line, column) = self.virtual_editor_buffer.char_to_line_col(clamped);
        let render_chars = self.virtual_line_render_chars(line);
        if column <= render_chars {
            clamped
        } else {
            self.virtual_editor_buffer
                .line_col_to_char(line, render_chars)
        }
    }

    fn virtual_cursor_wrap_metrics(&self, cursor: usize) -> VirtualCursorWrapMetrics {
        let cursor = self.clamp_virtual_cursor_for_render(cursor);
        let (line, col) = self.virtual_editor_buffer.char_to_line_col(cursor);
        let display_col =
            self.virtual_layout
                .line_char_to_display_column(&self.virtual_editor_buffer, line, col);
        let wrap_cols = self.virtual_layout.wrap_columns().max(1);
        let line_cols = self
            .virtual_layout
            .line_columns(&self.virtual_editor_buffer, line);
        VirtualCursorWrapMetrics {
            line,
            display_col,
            line_cols,
            wrap_cols,
        }
    }

    fn virtual_preferred_column_for_cursor(&self, cursor: usize) -> usize {
        let metrics = self.virtual_cursor_wrap_metrics(cursor);
        // Preserve wrap-boundary end-of-line intent (x == wrap width) so vertical
        // navigation does not collapse to column 0 on the next line.
        if metrics.display_col == metrics.line_cols
            && metrics.display_col > 0
            && metrics.display_col % metrics.wrap_cols == 0
        {
            metrics.wrap_cols
        } else {
            metrics.display_col % metrics.wrap_cols
        }
    }

    fn virtual_is_internal_wrap_boundary_cursor(&self, cursor: usize) -> bool {
        let metrics = self.virtual_cursor_wrap_metrics(cursor);
        is_internal_wrap_boundary(metrics.display_col, metrics.wrap_cols, metrics.line_cols)
    }

    fn vertical_boundary_affinity_for_target(
        &self,
        cursor: usize,
        desired_col_in_row: usize,
        up: bool,
    ) -> WrapBoundaryAffinity {
        let cols = self.virtual_layout.wrap_columns().max(1);
        if desired_col_in_row == cols && self.virtual_is_internal_wrap_boundary_cursor(cursor) && up
        {
            WrapBoundaryAffinity::Upstream
        } else {
            WrapBoundaryAffinity::Downstream
        }
    }

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
        let rope = self.virtual_editor_buffer.rope();
        let mut idx = self
            .clamp_virtual_cursor_for_render(cursor)
            .min(self.virtual_editor_buffer.len_chars());
        if idx == 0 {
            return 0;
        }

        while idx > 0 {
            let line_end = self.virtual_line_render_boundary(idx).0;
            if idx > line_end {
                idx = line_end;
                continue;
            }
            if !rope.char(idx - 1).is_whitespace() {
                break;
            }
            idx -= 1;
        }
        if idx == 0 {
            return 0;
        }
        let kind = is_editor_word_char(rope.char(idx - 1));

        while idx > 0 {
            let line_end = self.virtual_line_render_boundary(idx).0;
            if idx > line_end {
                idx = line_end;
                continue;
            }
            if is_editor_word_char(rope.char(idx - 1)) != kind {
                break;
            }
            idx -= 1;
        }
        idx
    }

    pub(super) fn virtual_word_right(&self, cursor: usize) -> usize {
        let rope = self.virtual_editor_buffer.rope();
        let len = self.virtual_editor_buffer.len_chars();
        let mut idx = self.clamp_virtual_cursor_for_render(cursor).min(len);
        while idx < len {
            let (line_end, capped) = self.virtual_line_render_boundary(idx);
            if capped && idx >= line_end {
                return line_end;
            }
            if !rope.char(idx).is_whitespace() {
                break;
            }
            idx += 1;
        }
        if idx >= len {
            return len;
        }

        let kind = is_editor_word_char(rope.char(idx));
        while idx < len {
            let (line_end, capped) = self.virtual_line_render_boundary(idx);
            if capped && idx >= line_end {
                return line_end;
            }
            if is_editor_word_char(rope.char(idx)) != kind {
                break;
            }
            idx += 1;
        }
        idx
    }

    pub(super) fn virtual_move_vertical_target(
        &self,
        cursor: usize,
        desired_col_in_row: usize,
        up: bool,
        boundary_affinity: WrapBoundaryAffinity,
    ) -> usize {
        let cursor = self.clamp_virtual_cursor_for_render(cursor);
        let metrics = self.virtual_cursor_wrap_metrics(cursor);
        let rows = self.virtual_layout.line_visual_rows(metrics.line).max(1);
        let on_internal_wrap_boundary =
            is_internal_wrap_boundary(metrics.display_col, metrics.wrap_cols, metrics.line_cols);
        let mut row = (metrics.display_col / metrics.wrap_cols).min(rows.saturating_sub(1));
        if on_internal_wrap_boundary && boundary_affinity == WrapBoundaryAffinity::Upstream {
            row = row.saturating_sub(1);
        }
        let line_count = self.virtual_editor_buffer.line_count();

        let target_line_and_row: Option<(usize, usize)> = if up {
            if row > 0 {
                Some((metrics.line, row - 1))
            } else if metrics.line > 0 {
                let prev_line = metrics.line - 1;
                let prev_rows = self.virtual_layout.line_visual_rows(prev_line).max(1);
                Some((prev_line, prev_rows.saturating_sub(1)))
            } else {
                None
            }
        } else if row + 1 < rows {
            Some((metrics.line, row + 1))
        } else if metrics.line + 1 < line_count {
            Some((metrics.line + 1, 0usize))
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
        let row_start = target_row.saturating_mul(metrics.wrap_cols);
        let desired_col_in_row = desired_col_in_row.min(metrics.wrap_cols);
        let target_display_col = if row_start >= target_line_cols {
            target_line_cols
        } else {
            // Clamp to the target row boundary so vertical navigation can land at
            // end-of-row for shorter rows instead of one column early.
            let row_len = target_line_cols
                .saturating_sub(row_start)
                .min(metrics.wrap_cols);
            row_start + desired_col_in_row.min(row_len)
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

    fn apply_virtual_layout_delta_with_recovery(&mut self, delta: VirtualEditDelta) -> bool {
        if self
            .virtual_layout
            .apply_delta(&self.virtual_editor_buffer, delta)
        {
            self.virtual_galley_cache
                .apply_delta(delta, self.virtual_editor_buffer.line_count());
            return true;
        }

        // `apply_delta` failed invariant checks. Rebuild immediately from cached
        // geometry when possible so subsequent cursor/layout operations in the
        // same input pass use consistent row metrics.
        let rebuilt = self
            .virtual_layout
            .rebuild_with_cached_geometry(&self.virtual_editor_buffer);
        if !rebuilt {
            self.virtual_layout = WrapLayoutCache::default();
        }
        self.virtual_galley_cache.evict_all();
        rebuilt
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
        // Persist undo anchors in the same renderable cursor domain used by
        // live navigation/caret drawing so undo cannot restore an off-screen
        // insertion point on render-capped lines.
        let before_cursor =
            self.clamp_virtual_cursor_for_render(self.virtual_editor_state.cursor());
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
            let layout_recovered = self.apply_virtual_layout_delta_with_recovery(delta);
            let layout_apply_ms = layout_started
                .map(|started| started.elapsed().as_secs_f32() * 1000.0)
                .unwrap_or(0.0);
            let galley_started = perf_enabled.then(Instant::now);
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
                VirtualInputCommand::MoveEnd { select } => {
                    let (line, _) = self
                        .virtual_editor_buffer
                        .char_to_line_col(self.virtual_editor_state.cursor());
                    let target = self
                        .virtual_editor_buffer
                        .line_col_to_char(line, self.virtual_line_render_chars(line));
                    let target = self.clamp_virtual_cursor_for_render(target);
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
                    self.virtual_editor_state.ime.preedit_range = None;
                    self.virtual_editor_state.ime.preedit_text.clear();
                    let delta = self.virtual_editor_history.undo(
                        &mut self.virtual_editor_buffer,
                        &mut self.virtual_editor_state,
                    );
                    if let Some(delta) = delta {
                        result.changed = true;
                        let _layout_ok = self.apply_virtual_layout_delta_with_recovery(delta);
                        let _cursor_clamped = self.clamp_virtual_cursor_state_for_render();
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
                        let _layout_ok = self.apply_virtual_layout_delta_with_recovery(delta);
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
