//! Small state accessors shared across editor modes.

use super::editor::EditorMode;
use super::LocalPasteApp;

impl LocalPasteApp {
    pub(super) fn is_virtual_editor_mode(&self) -> bool {
        self.editor_mode == EditorMode::VirtualEditor
    }

    pub(super) fn active_text_len_bytes(&self) -> usize {
        match self.editor_mode {
            EditorMode::VirtualEditor => self.virtual_editor_buffer.len_bytes(),
            _ => self.selected_content.len(),
        }
    }

    pub(super) fn active_text_chars(&self) -> usize {
        match self.editor_mode {
            EditorMode::VirtualEditor => self.virtual_editor_buffer.len_chars(),
            _ => self.selected_content.chars_len(),
        }
    }

    pub(super) fn active_revision(&self) -> u64 {
        match self.editor_mode {
            EditorMode::VirtualEditor => self.virtual_editor_buffer.revision(),
            _ => self.selected_content.revision(),
        }
    }

    pub(super) fn active_snapshot(&self) -> String {
        match self.editor_mode {
            EditorMode::VirtualEditor => self.virtual_editor_buffer.to_string(),
            _ => self.selected_content.to_string(),
        }
    }
}
