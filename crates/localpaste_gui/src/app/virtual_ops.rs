//! Virtual editor operations for selection and cursor/navigation behavior.

use super::virtual_editor::{
    VirtualEditorHistory, VirtualEditorState, VirtualGalleyCache, WrapBoundaryAffinity,
    WrapLayoutCache,
};
use super::{is_editor_word_char, next_virtual_click_count, LocalPasteApp};
use eframe::egui;
use std::ops::Range;
use std::time::Instant;

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
    /// Clamps the active cursor after layout changes that shorten renderable line spans.
    ///
    /// # Returns
    /// `true` when the cursor was adjusted.
    pub(super) fn clamp_virtual_cursor_state_for_render(&mut self) -> bool {
        let cursor = self.virtual_editor_state.cursor();
        let clamped = self.clamp_virtual_cursor_for_render(cursor);
        if clamped == cursor {
            return false;
        }
        self.virtual_editor_state
            .set_cursor(clamped, self.virtual_editor_buffer.len_chars());
        true
    }

    /// Returns the renderable char count for a line, honoring layout cache truncation.
    ///
    /// # Returns
    /// Renderable character count for `line`.
    pub(super) fn virtual_line_render_chars(&self, line: usize) -> usize {
        let full_chars = self.virtual_editor_buffer.line_len_chars(line);
        let cached_chars = self.virtual_layout.line_chars(line);
        if cached_chars == 0 {
            full_chars
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

    /// Clamps a global cursor index to the currently renderable portion of its line.
    ///
    /// # Returns
    /// A cursor index guaranteed to land within buffer bounds and rendered line extent.
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
    /// Derives preferred wrapped-row column for subsequent vertical cursor moves.
    ///
    /// # Returns
    /// Preferred column within the active wrap row.
    ///
    /// # Panics
    /// Panics only if wrap metrics become inconsistent with cached layout state.
    pub(super) fn virtual_preferred_column_for_cursor(&self, cursor: usize) -> usize {
        let metrics = self.virtual_cursor_wrap_metrics(cursor);
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

    /// Resolves wrap-boundary affinity after a vertical cursor move.
    ///
    /// # Arguments
    /// - `cursor`: New global cursor index.
    /// - `desired_col_in_row`: Requested visual column within the wrapped row.
    /// - `up`: Whether the movement direction was upward.
    ///
    /// # Returns
    /// Boundary affinity to preserve expected row-end/start caret behavior.
    pub(super) fn vertical_boundary_affinity_for_target(
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

    /// Returns the preview selection as text using line-aware slicing semantics.
    ///
    /// # Returns
    /// Selected text joined with `\n`, or `None` when selection is empty.
    ///
    /// # Panics
    /// Panics only if internal cursor/line indices become inconsistent with `selected_content`.
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

    /// Resets virtual editor buffer/state/caches to match a fresh text snapshot.
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

    /// Restarts the caret blink timer from the current instant.
    pub(super) fn reset_virtual_caret_blink(&mut self) {
        self.virtual_caret_phase_start = Instant::now();
    }

    /// Clears virtual preview click streak tracking.
    pub(super) fn reset_virtual_click_streak(&mut self) {
        self.last_virtual_click_at = None;
        self.last_virtual_click_pos = None;
        self.last_virtual_click_line = None;
        self.last_virtual_click_count = 0;
    }

    /// Records a click in the virtual preview and returns the updated streak count.
    ///
    /// # Arguments
    /// - `line_idx`: Line index of the click target.
    /// - `pointer_pos`: Pointer position in viewport coordinates.
    ///
    /// # Returns
    /// Click streak count in `[1, 3]` for single/double/triple click behavior.
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

    /// Returns the currently selected virtual-editor range as a UTF-8 snapshot.
    ///
    /// # Returns
    /// Selected text when a non-empty selection exists.
    pub(super) fn virtual_selected_text(&self) -> Option<String> {
        let range = self.virtual_editor_state.selection_range()?;
        Some(self.virtual_editor_buffer.slice_chars(range))
    }

    /// Selects the full physical line, including newline when present.
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

    /// Finds the previous word boundary for word-left navigation.
    ///
    /// Semantics are intentionally code-editor oriented:
    /// - Skip any non-word characters (whitespace, punctuation/operators, newlines)
    /// - Then land at the *start* of the previous identifier-like run
    ///
    /// This avoids the common "two-step over punctuation" bug where `foo.|bar`
    /// requires an extra keypress to cross `.`.
    ///
    /// # Returns
    /// Cursor index positioned at the previous token start boundary.
    pub(super) fn virtual_word_left(&self, cursor: usize) -> usize {
        let rope = self.virtual_editor_buffer.rope();
        let mut idx = self
            .clamp_virtual_cursor_for_render(cursor)
            .min(self.virtual_editor_buffer.len_chars());
        if idx == 0 {
            return 0;
        }

        // Skip separators/whitespace first…
        while idx > 0 && !is_editor_word_char(rope.char(idx - 1)) {
            idx -= 1;
        }

        // …then skip the word run itself.
        while idx > 0 && is_editor_word_char(rope.char(idx - 1)) {
            idx -= 1;
        }

        idx
    }

    fn virtual_word_right_start(&self, cursor: usize) -> usize {
        let rope = self.virtual_editor_buffer.rope();
        let len = self.virtual_editor_buffer.len_chars();
        let mut idx = self.clamp_virtual_cursor_for_render(cursor).min(len);
        if idx >= len {
            return len;
        }

        // Only consult render boundaries when crossing lines (or if the line is capped).
        let (mut line_end, mut capped) = self.virtual_line_render_boundary(idx);

        // 1) Skip the remainder of the current word (if any).
        while idx < len {
            if capped && idx >= line_end {
                return line_end;
            }
            let ch = rope.char(idx);
            if !is_editor_word_char(ch) {
                break;
            }
            idx += 1;

            // If we stepped over the newline into the next line, refresh render boundary info.
            if !capped && idx > line_end {
                (line_end, capped) = self.virtual_line_render_boundary(idx);
            }
        }

        // 2) Skip separators/whitespace to the start of the next word.
        while idx < len {
            if capped && idx >= line_end {
                return line_end;
            }
            let ch = rope.char(idx);
            if is_editor_word_char(ch) {
                break;
            }
            idx += 1;

            if !capped && idx > line_end {
                (line_end, capped) = self.virtual_line_render_boundary(idx);
            }
        }

        idx
    }

    /// Finds the next word boundary for word-right navigation.
    ///
    /// Platform conventions differ:
    /// - **Windows/Linux**: `Ctrl+Right` lands at the *start* of the next word.
    /// - **macOS**: `Option+Right` lands at the *end* of the next word.
    ///
    /// We follow those conventions while keeping the underlying definition of
    /// "word character" consistent (`is_editor_word_char`).
    ///
    /// # Returns
    /// Cursor index positioned at the next platform-appropriate word boundary.
    pub(super) fn virtual_word_right(&self, cursor: usize) -> usize {
        if cfg!(target_os = "macos") {
            self.virtual_word_right_end(cursor)
        } else {
            self.virtual_word_right_start(cursor)
        }
    }

    /// Returns the end-of-next-word boundary.
    ///
    /// Used by word-delete-forward and by macOS word-right movement semantics.
    ///
    /// # Returns
    /// Cursor index positioned at the end of the next identifier-like token.
    pub(super) fn virtual_word_right_end(&self, cursor: usize) -> usize {
        let rope = self.virtual_editor_buffer.rope();
        let len = self.virtual_editor_buffer.len_chars();
        let mut idx = self.clamp_virtual_cursor_for_render(cursor).min(len);
        if idx >= len {
            return len;
        }

        // Only consult render boundaries when crossing lines (or if the line is capped).
        let (mut line_end, mut capped) = self.virtual_line_render_boundary(idx);

        // If we're currently in a word, move to the end of this word.
        if is_editor_word_char(rope.char(idx)) {
            while idx < len {
                if capped && idx >= line_end {
                    return line_end;
                }
                let ch = rope.char(idx);
                if !is_editor_word_char(ch) {
                    break;
                }
                idx += 1;
                if !capped && idx > line_end {
                    (line_end, capped) = self.virtual_line_render_boundary(idx);
                }
            }
            return idx;
        }

        // Otherwise: skip separators/whitespace to the start of the next word…
        while idx < len {
            if capped && idx >= line_end {
                return line_end;
            }
            let ch = rope.char(idx);
            if is_editor_word_char(ch) {
                break;
            }
            idx += 1;
            if !capped && idx > line_end {
                (line_end, capped) = self.virtual_line_render_boundary(idx);
            }
        }

        // …then to the end of that word.
        while idx < len {
            if capped && idx >= line_end {
                return line_end;
            }
            let ch = rope.char(idx);
            if !is_editor_word_char(ch) {
                break;
            }
            idx += 1;
            if !capped && idx > line_end {
                (line_end, capped) = self.virtual_line_render_boundary(idx);
            }
        }

        idx
    }

    /// Computes the cursor target for vertical movement across wrapped rows/lines.
    ///
    /// # Arguments
    /// - `cursor`: Current global cursor index.
    /// - `desired_col_in_row`: Preferred visual column within wrapped rows.
    /// - `up`: `true` for upward navigation, `false` for downward.
    /// - `boundary_affinity`: Wrap-boundary tie-breaker for exact boundary positions.
    ///
    /// # Returns
    /// Global cursor index for the resolved vertical target.
    ///
    /// # Panics
    /// Panics only if wrap-layout caches become internally inconsistent.
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

    /// Returns local selection bounds for a rendered line segment, if selected.
    ///
    /// # Arguments
    /// - `line_start`: Global start char index of the rendered line segment.
    /// - `line_chars`: Character length of the rendered line segment.
    ///
    /// # Returns
    /// Local `[start, end)` selection bounds within the line segment.
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
}
