//! Line indentation as one reversible edit, retaining directional selections.

use super::{virtual_editor::EditIntent, LocalPasteApp};
use std::time::Instant;

impl LocalPasteApp {
    /// Indent selected lines, or remove one leading tab/up to four spaces.
    ///
    /// # Arguments
    /// - `unindent`: Remove indentation instead of inserting spaces.
    /// - `now`: History timestamp.
    ///
    /// # Returns
    /// Whether the buffer changed.
    pub(super) fn indent_virtual_lines(&mut self, unindent: bool, now: Instant) -> bool {
        let cursor = self.virtual_editor_state.cursor();
        let anchor = self.virtual_editor_state.anchor();
        let selection = self.virtual_editor_state.selection_range();
        if !unindent && selection.is_none() {
            return self.replace_virtual_range(
                cursor..cursor,
                "    ",
                EditIntent::Other,
                true,
                now,
            );
        }
        let range = selection.unwrap_or(cursor..cursor);
        let first = self.virtual_editor_buffer.char_to_line_col(range.start).0;
        let (mut last, column) = self.virtual_editor_buffer.char_to_line_col(range.end);
        if range.end > range.start && column == 0 {
            last = last.saturating_sub(1).max(first);
        }
        let start = self.virtual_editor_buffer.line_col_to_char(first, 0);
        let end = if last + 1 < self.virtual_editor_buffer.line_count() {
            self.virtual_editor_buffer.line_col_to_char(last + 1, 0)
        } else {
            self.virtual_editor_buffer.len_chars()
        };
        let original = self.virtual_editor_buffer.slice_chars(start..end);
        let mut replacement = String::new();
        let mut edits = Vec::new();
        let mut line_start = start;
        // Use the buffer's line model (including CR and Unicode separators),
        // retaining terminators without indenting the next unselected line.
        for line in self
            .virtual_editor_buffer
            .rope()
            .lines_at(first)
            .take(last - first + 1)
        {
            let removed = if !unindent {
                0
            } else if line.chars().next() == Some('\t') {
                1
            } else {
                line.chars().take(4).take_while(|ch| *ch == ' ').count()
            };
            let added = if unindent { 0 } else { 4 };
            edits.push((line_start, removed, added));
            if !unindent {
                replacement.push_str("    ");
            }
            replacement.extend(line.chars().skip(removed));
            line_start += line.len_chars();
        }
        if replacement == original {
            return false;
        }
        let map_position = |position: usize| {
            let mut mapped = position;
            for &(start, removed, added) in &edits {
                if position >= start {
                    mapped = mapped.saturating_sub((position - start).min(removed)) + added;
                }
            }
            mapped
        };
        self.replace_virtual_range(start..end, &replacement, EditIntent::Other, true, now);
        let mapped_cursor = map_position(cursor);
        let mapped_anchor = anchor.map(map_position);
        self.virtual_editor_state.restore_selection(
            mapped_cursor,
            mapped_anchor,
            self.virtual_editor_buffer.len_chars(),
        );
        self.virtual_editor_history
            .finish_selection_edit(anchor, mapped_anchor, mapped_cursor);
        true
    }
}
