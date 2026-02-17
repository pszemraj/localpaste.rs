//! Selection, caret, focus, and IME state for the virtual editor.

use std::ops::Range;

/// IME composition state tracked by the virtual editor.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ImeState {
    pub(crate) enabled: bool,
    pub(crate) preedit_range: Option<Range<usize>>,
    pub(crate) preedit_text: String,
}

/// Affinity for caret positions that land exactly on internal wrap boundaries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum WrapBoundaryAffinity {
    /// Boundary belongs to the previous visual row (caret draws at row end).
    Upstream,
    /// Boundary belongs to the next visual row (caret draws at row start).
    #[default]
    Downstream,
}

/// Mutable editor interaction state independent of rendering.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct VirtualEditorState {
    cursor: usize,
    anchor: Option<usize>,
    preferred_column: Option<usize>,
    wrap_boundary_affinity: WrapBoundaryAffinity,
    pub(crate) has_focus: bool,
    pub(crate) ime: ImeState,
}

impl VirtualEditorState {
    /// Returns the current caret position in global char coordinates.
    pub(crate) fn cursor(&self) -> usize {
        self.cursor
    }

    /// Returns the preferred visual column for vertical movement.
    pub(crate) fn preferred_column(&self) -> Option<usize> {
        self.preferred_column
    }

    /// Returns wrap-boundary affinity for the current cursor.
    pub(crate) fn wrap_boundary_affinity(&self) -> WrapBoundaryAffinity {
        self.wrap_boundary_affinity
    }

    /// Sets the cursor, clearing any active selection.
    pub(crate) fn set_cursor(&mut self, char_index: usize, text_len: usize) {
        self.cursor = char_index.min(text_len);
        self.anchor = None;
        self.preferred_column = None;
        self.wrap_boundary_affinity = WrapBoundaryAffinity::Downstream;
    }

    /// Moves cursor to a new char index.
    pub(crate) fn move_cursor(&mut self, new_index: usize, text_len: usize, select: bool) {
        let clamped = new_index.min(text_len);
        if select {
            if self.anchor.is_none() {
                self.anchor = Some(self.cursor);
            }
        } else {
            self.anchor = None;
        }
        self.cursor = clamped;
    }

    /// Selects the entire buffer.
    pub(crate) fn select_all(&mut self, text_len: usize) {
        self.anchor = Some(0);
        self.cursor = text_len;
        self.wrap_boundary_affinity = WrapBoundaryAffinity::Downstream;
    }

    /// Returns a normalized selected range, if any.
    pub(crate) fn selection_range(&self) -> Option<Range<usize>> {
        let anchor = self.anchor?;
        if anchor == self.cursor {
            return None;
        }
        if anchor < self.cursor {
            Some(anchor..self.cursor)
        } else {
            Some(self.cursor..anchor)
        }
    }

    /// Updates preferred visual column for subsequent vertical motions.
    pub(crate) fn set_preferred_column(&mut self, column: usize) {
        self.preferred_column = Some(column);
    }

    /// Sets wrap-boundary affinity for subsequent vertical navigation/rendering.
    pub(crate) fn set_wrap_boundary_affinity(&mut self, affinity: WrapBoundaryAffinity) {
        self.wrap_boundary_affinity = affinity;
    }

    /// Clears preferred visual column hint.
    pub(crate) fn clear_preferred_column(&mut self) {
        self.preferred_column = None;
        self.wrap_boundary_affinity = WrapBoundaryAffinity::Downstream;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_range_normalizes_direction() {
        let mut state = VirtualEditorState::default();
        state.set_cursor(8, 100);
        state.move_cursor(3, 100, true);
        assert_eq!(state.selection_range(), Some(3..8));
    }

    #[test]
    fn select_all_sets_full_range() {
        let mut state = VirtualEditorState::default();
        state.select_all(42);
        assert_eq!(state.selection_range(), Some(0..42));
    }
}
