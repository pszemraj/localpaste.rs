//! Small state accessors shared across editor modes.

use super::editor::EditorMode;
use super::LocalPasteApp;
use crate::backend::PasteSummary;

impl LocalPasteApp {
    /// Returns whether a detached version-history or diff window currently owns the workflow.
    ///
    /// # Returns
    /// `true` when History, Diff, or reset confirmation is open.
    pub(super) fn version_overlay_open(&self) -> bool {
        self.version_ui.history_modal_open
            || self.version_ui.diff_modal_open
            || self.version_ui.history_reset_confirm_open
    }

    /// Returns whether a modal keyboard overlay should block background editor routing.
    ///
    /// Non-modal chrome like the properties drawer is intentionally excluded so
    /// the editor can remain live beside it.
    ///
    /// # Returns
    /// `true` when a modal keyboard-owning surface is open.
    pub(super) fn keyboard_overlay_open(&self) -> bool {
        self.command_palette_open || self.shortcut_help_open || self.version_overlay_open()
    }

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

    /// Returns the current selected list/search summary when available.
    ///
    /// # Returns
    /// The best available summary for the selected paste across visible,
    /// cached, and palette result projections.
    pub(super) fn selected_paste_summary(&self) -> Option<&PasteSummary> {
        let selected_id = self.selected_id.as_deref()?;
        // Active search/palette projections can be fresher than `all_pastes`,
        // which is only refreshed by full list updates.
        self.pastes
            .iter()
            .find(|item| item.id.as_str() == selected_id)
            .or_else(|| {
                self.palette_search_results
                    .iter()
                    .find(|item| item.id.as_str() == selected_id)
            })
            .or_else(|| {
                self.all_pastes
                    .iter()
                    .find(|item| item.id.as_str() == selected_id)
            })
    }
}
