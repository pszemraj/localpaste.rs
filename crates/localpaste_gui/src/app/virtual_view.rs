//! Virtual editor selection state for the read-only preview.

use std::ops::Range;

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
pub(super) struct VirtualSelectionState {
    cursor: Option<VirtualCursor>,
    selection: Option<VirtualSelection>,
    drag_anchor: Option<VirtualCursor>,
}

impl VirtualSelectionState {
    pub(super) fn clear(&mut self) {
        self.cursor = None;
        self.selection = None;
        self.drag_anchor = None;
    }

    pub(super) fn set_cursor(&mut self, cursor: VirtualCursor) {
        self.cursor = Some(cursor);
        self.selection = None;
        self.drag_anchor = None;
    }

    pub(super) fn select_range(&mut self, start: VirtualCursor, end: VirtualCursor) {
        self.cursor = Some(end);
        self.selection = Some(VirtualSelection { start, end });
        self.drag_anchor = None;
    }

    pub(super) fn begin_drag(&mut self, anchor: VirtualCursor) {
        self.cursor = Some(anchor);
        self.drag_anchor = Some(anchor);
        self.selection = Some(VirtualSelection {
            start: anchor,
            end: anchor,
        });
    }

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

    pub(super) fn end_drag(&mut self) {
        self.drag_anchor = None;
    }

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
