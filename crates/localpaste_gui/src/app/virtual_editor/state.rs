//! Selection, caret, focus, and IME state for the virtual editor.

use std::ops::Range;

/// IME composition state tracked by the virtual editor.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct ImeState {
    pub(super) enabled: bool,
    pub(super) preedit_range: Option<Range<usize>>,
    pub(super) preedit_text: String,
}

/// Mutable editor interaction state independent of rendering.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct VirtualEditorState {
    cursor: usize,
    anchor: Option<usize>,
    preferred_column: Option<usize>,
    pub(super) has_focus: bool,
    pub(super) ime: ImeState,
}

impl VirtualEditorState {
    /// Returns the current caret position in global char coordinates.
    pub(super) fn cursor(&self) -> usize {
        self.cursor
    }

    /// Returns the preferred visual column for vertical movement.
    pub(super) fn preferred_column(&self) -> Option<usize> {
        self.preferred_column
    }

    /// Sets the cursor, clearing any active selection.
    pub(super) fn set_cursor(&mut self, char_index: usize, text_len: usize) {
        self.cursor = char_index.min(text_len);
        self.anchor = None;
        self.preferred_column = None;
    }

    /// Moves cursor to a new char index.
    pub(super) fn move_cursor(&mut self, new_index: usize, text_len: usize, select: bool) {
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
    pub(super) fn select_all(&mut self, text_len: usize) {
        self.anchor = Some(0);
        self.cursor = text_len;
    }

    /// Clears active selection, keeping cursor in place.
    pub(super) fn clear_selection(&mut self) {
        self.anchor = None;
    }

    /// True when the state has a non-empty selection.
    pub(super) fn has_selection(&self) -> bool {
        self.selection_range().is_some()
    }

    /// Returns a normalized selected range, if any.
    pub(super) fn selection_range(&self) -> Option<Range<usize>> {
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

    /// Collapse selection to its start edge if selection is active.
    pub(super) fn collapse_to_selection_start(&mut self) {
        if let Some(range) = self.selection_range() {
            self.cursor = range.start;
            self.anchor = None;
        }
    }

    /// Collapse selection to its end edge if selection is active.
    pub(super) fn collapse_to_selection_end(&mut self) {
        if let Some(range) = self.selection_range() {
            self.cursor = range.end;
            self.anchor = None;
        }
    }

    /// Updates preferred visual column for subsequent vertical motions.
    pub(super) fn set_preferred_column(&mut self, column: usize) {
        self.preferred_column = Some(column);
    }

    /// Clears preferred visual column hint.
    pub(super) fn clear_preferred_column(&mut self) {
        self.preferred_column = None;
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
