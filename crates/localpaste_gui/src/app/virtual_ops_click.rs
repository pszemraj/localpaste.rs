//! Large-editor click streak handling for word/line selection gestures.

use super::util::word_range_at;
use super::{LocalPasteApp, EDITOR_DOUBLE_CLICK_DISTANCE, EDITOR_DOUBLE_CLICK_WINDOW};
use eframe::egui::{
    text::{CCursor, CCursorRange},
    text_edit::TextEditOutput,
};
use std::time::Instant;

impl LocalPasteApp {
    /// Applies double/triple-click selection behavior in the large-buffer editor.
    ///
    /// Double-click selects a word and triple-click selects the full line.
    ///
    /// # Arguments
    /// - `output`: Current text-edit response/output snapshot.
    /// - `text`: Source text backing the editor widget.
    /// - `is_large_buffer`: Whether large-buffer virtualized behavior is active.
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
}
