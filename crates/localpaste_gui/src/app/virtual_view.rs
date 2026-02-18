//! Virtual editor selection state for the read-only preview.

use std::ops::Range;

/// Cursor position in `(line, column)` coordinates for preview selection state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct VirtualCursor {
    pub(super) line: usize,
    pub(super) column: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct VirtualSelection {
    start: VirtualCursor,
    end: VirtualCursor,
}

#[derive(Default)]
/// Selection/drag state for the read-only virtual preview pane.
pub(super) struct VirtualSelectionState {
    cursor: Option<VirtualCursor>,
    selection: Option<VirtualSelection>,
    drag_anchor: Option<VirtualCursor>,
}

impl VirtualSelectionState {
    /// Clears cursor, selection, and drag state.
    pub(super) fn clear(&mut self) {
        self.cursor = None;
        self.selection = None;
        self.drag_anchor = None;
    }

    /// Sets a single cursor position and clears any active range selection.
    pub(super) fn set_cursor(&mut self, cursor: VirtualCursor) {
        self.cursor = Some(cursor);
        self.selection = None;
        self.drag_anchor = None;
    }

    /// Sets an explicit selection range and tracks `end` as the active cursor.
    ///
    /// # Arguments
    /// - `start`: Selection anchor cursor.
    /// - `end`: Active cursor endpoint.
    pub(super) fn select_range(&mut self, start: VirtualCursor, end: VirtualCursor) {
        self.cursor = Some(end);
        self.selection = Some(VirtualSelection { start, end });
        self.drag_anchor = None;
    }

    /// Starts drag-selection at `anchor`.
    pub(super) fn begin_drag(&mut self, anchor: VirtualCursor) {
        self.cursor = Some(anchor);
        self.drag_anchor = Some(anchor);
        self.selection = Some(VirtualSelection {
            start: anchor,
            end: anchor,
        });
    }

    /// Extends drag-selection to `cursor` when a drag is active.
    pub(super) fn update_drag(&mut self, cursor: VirtualCursor) {
        let Some(anchor) = self.drag_anchor else {
            return;
        };
        self.cursor = Some(cursor);
        self.selection = Some(VirtualSelection {
            start: anchor,
            end: cursor,
        });
    }

    /// Ends drag mode while preserving the current selection.
    pub(super) fn end_drag(&mut self) {
        self.drag_anchor = None;
    }

    /// Returns the selected character range for a specific rendered line.
    ///
    /// # Arguments
    /// - `line_idx`: Zero-based line index in the preview.
    /// - `line_chars`: Visible character length of that line.
    ///
    /// # Returns
    /// Local selected character range for the line, or `None` when unselected.
    pub(super) fn selection_for_line(
        &self,
        line_idx: usize,
        line_chars: usize,
    ) -> Option<Range<usize>> {
        let selection = self.selection?;
        let (start, end) = normalize_selection(selection);
        if line_idx < start.line || line_idx > end.line {
            return None;
        }
        let start_col = if line_idx == start.line {
            start.column.min(line_chars)
        } else {
            0
        };
        let end_col = if line_idx == end.line {
            end.column.min(line_chars)
        } else {
            line_chars
        };
        if start_col >= end_col {
            return None;
        }
        Some(start_col..end_col)
    }

    /// Returns normalized selection endpoints, if a selection is active.
    ///
    /// # Returns
    /// Ordered `(start, end)` cursors when a selection exists.
    pub(super) fn selection_bounds(&self) -> Option<(VirtualCursor, VirtualCursor)> {
        self.selection.map(normalize_selection)
    }
}

fn normalize_selection(selection: VirtualSelection) -> (VirtualCursor, VirtualCursor) {
    let a = selection.start;
    let b = selection.end;
    if a.line < b.line || (a.line == b.line && a.column <= b.column) {
        (a, b)
    } else {
        (b, a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_for_line_clamps_and_orders() {
        let mut state = VirtualSelectionState::default();
        state.select_range(
            VirtualCursor { line: 2, column: 4 },
            VirtualCursor { line: 1, column: 1 },
        );

        let line1 = state.selection_for_line(1, 10).expect("line 1");
        assert_eq!(line1, 1..10);

        let line2 = state.selection_for_line(2, 6).expect("line 2");
        assert_eq!(line2, 0..4);

        assert!(state.selection_for_line(0, 5).is_none());
        assert!(state.selection_for_line(3, 5).is_none());
    }
}
