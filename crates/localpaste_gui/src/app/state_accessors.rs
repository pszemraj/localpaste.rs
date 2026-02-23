//! Small state accessors shared across editor modes.

use super::editor::EditorMode;
use super::LocalPasteApp;

impl LocalPasteApp {
    /// Returns whether the app is currently in interactive virtual-editor mode.
    ///
    /// # Returns
    /// `true` when the active editor mode is [`EditorMode::VirtualEditor`].
    pub(super) fn is_virtual_editor_mode(&self) -> bool {
        self.editor_mode == EditorMode::VirtualEditor
    }

    /// Returns active buffer length in bytes for the current editor mode.
    ///
    /// # Returns
    /// UTF-8 byte count from virtual buffer or text-edit buffer.
    pub(super) fn active_text_len_bytes(&self) -> usize {
        match self.editor_mode {
            EditorMode::VirtualEditor => self.virtual_editor_buffer.len_bytes(),
            EditorMode::VirtualPreview => self.selected_content.len(),
        }
    }

    /// Returns active buffer length in characters for the current editor mode.
    ///
    /// # Returns
    /// Character count from virtual buffer or text-edit buffer.
    pub(super) fn active_text_chars(&self) -> usize {
        match self.editor_mode {
            EditorMode::VirtualEditor => self.virtual_editor_buffer.len_chars(),
            EditorMode::VirtualPreview => self.selected_content.chars_len(),
        }
    }

    /// Returns active edit revision for the current editor mode.
    ///
    /// # Returns
    /// Monotonic revision counter for active buffer.
    pub(super) fn active_revision(&self) -> u64 {
        match self.editor_mode {
            EditorMode::VirtualEditor => self.virtual_editor_buffer.revision(),
            EditorMode::VirtualPreview => self.selected_content.revision(),
        }
    }

    /// Returns an owned snapshot of active editor text.
    ///
    /// # Returns
    /// Current content as a new [`String`].
    pub(super) fn active_snapshot(&self) -> String {
        match self.editor_mode {
            EditorMode::VirtualEditor => self.virtual_editor_buffer.to_string(),
            EditorMode::VirtualPreview => self.selected_content.to_string(),
        }
    }
}
