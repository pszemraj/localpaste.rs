//! State transitions for backend events, selection, and autosave flow.

use super::highlight::EditorLayoutCache;
use super::{LocalPasteApp, SaveStatus, StatusMessage, STATUS_TTL};
use crate::backend::{CoreCmd, CoreEvent, PasteSummary};
use std::time::Instant;
use tracing::warn;

impl LocalPasteApp {
    pub(super) fn apply_event(&mut self, event: CoreEvent) {
        match event {
            CoreEvent::PasteList { items } => {
                self.pastes = items;
                let selection_valid = self
                    .selected_id
                    .as_ref()
                    .map(|id| self.pastes.iter().any(|p| p.id == *id))
                    .unwrap_or(false);
                if !selection_valid {
                    if let Some(first) = self.pastes.first() {
                        self.select_paste(first.id.clone());
                    } else {
                        self.clear_selection();
                    }
                }
            }
            CoreEvent::PasteLoaded { paste } => {
                if self.selected_id.as_deref() == Some(paste.id.as_str()) {
                    self.selected_content.reset(paste.content.clone());
                    self.reset_virtual_editor(paste.content.as_str());
                    self.editor_cache = EditorLayoutCache::default();
                    self.editor_lines.reset();
                    self.virtual_selection.clear();
                    self.clear_highlight_state();
                    self.selected_paste = Some(paste);
                    self.save_status = SaveStatus::Saved;
                    self.last_edit_at = None;
                    self.save_in_flight = false;
                }
            }
            CoreEvent::PasteCreated { paste } => {
                let summary = PasteSummary::from_paste(&paste);
                self.pastes.insert(0, summary);
                self.select_paste(paste.id.clone());
                self.selected_content.reset(paste.content.clone());
                self.reset_virtual_editor(paste.content.as_str());
                self.editor_cache = EditorLayoutCache::default();
                self.editor_lines.reset();
                self.virtual_selection.clear();
                self.clear_highlight_state();
                self.selected_paste = Some(paste);
                self.save_status = SaveStatus::Saved;
                self.last_edit_at = None;
                self.save_in_flight = false;
                self.focus_editor_next = true;
                self.set_status("Created new paste.");
            }
            CoreEvent::PasteSaved { paste } => {
                if let Some(item) = self.pastes.iter_mut().find(|item| item.id == paste.id) {
                    *item = PasteSummary::from_paste(&paste);
                }
                if self.selected_id.as_deref() == Some(paste.id.as_str()) {
                    let mut updated = paste;
                    updated.content = self.active_snapshot();
                    self.selected_paste = Some(updated);
                    self.save_status = SaveStatus::Saved;
                    self.last_edit_at = None;
                    self.save_in_flight = false;
                }
            }
            CoreEvent::PasteDeleted { id } => {
                self.pastes.retain(|paste| paste.id != id);
                if self.selected_id.as_deref() == Some(id.as_str()) {
                    self.clear_selection();
                    self.set_status("Paste deleted.");
                } else {
                    self.set_status("Paste deleted; list refreshed.");
                }
                self.request_refresh();
            }
            CoreEvent::PasteMissing { id } => {
                self.pastes.retain(|paste| paste.id != id);
                if self.selected_id.as_deref() == Some(id.as_str()) {
                    self.clear_selection();
                    self.set_status("Selected paste was deleted; list refreshed.");
                } else {
                    self.set_status("Paste was deleted; list refreshed.");
                }
                self.request_refresh();
            }
            CoreEvent::Error { message } => {
                warn!("backend error: {}", message);
                self.set_status(message);
                if self.save_status == SaveStatus::Saving {
                    self.save_status = SaveStatus::Dirty;
                }
                self.save_in_flight = false;
            }
        }
    }

    pub(super) fn request_refresh(&mut self) {
        let _ = self.backend.cmd_tx.send(CoreCmd::ListAll { limit: 512 });
        self.last_refresh_at = Instant::now();
    }

    pub(super) fn select_paste(&mut self, id: String) {
        if let Some(prev) = self.selected_id.take() {
            self.locks.unlock(&prev);
        }
        self.selected_id = Some(id.clone());
        self.locks.lock(&id);
        self.selected_paste = None;
        self.selected_content.reset(String::new());
        self.reset_virtual_editor("");
        self.editor_cache = EditorLayoutCache::default();
        self.editor_lines.reset();
        self.virtual_selection.clear();
        self.clear_highlight_state();
        self.save_status = SaveStatus::Saved;
        self.last_edit_at = None;
        self.save_in_flight = false;
        let _ = self.backend.cmd_tx.send(CoreCmd::GetPaste { id });
    }

    pub(super) fn clear_selection(&mut self) {
        if let Some(prev) = self.selected_id.take() {
            self.locks.unlock(&prev);
        }
        self.selected_paste = None;
        self.selected_content.reset(String::new());
        self.reset_virtual_editor("");
        self.editor_cache = EditorLayoutCache::default();
        self.editor_lines.reset();
        self.virtual_selection.clear();
        self.clear_highlight_state();
        self.save_status = SaveStatus::Saved;
        self.last_edit_at = None;
        self.save_in_flight = false;
    }

    pub(super) fn set_status(&mut self, text: impl Into<String>) {
        self.status = Some(StatusMessage {
            text: text.into(),
            expires_at: Instant::now() + STATUS_TTL,
        });
    }

    pub(super) fn create_new_paste(&mut self) {
        self.create_new_paste_with_content(String::new());
    }

    pub(super) fn create_new_paste_with_content(&mut self, content: String) {
        let _ = self.backend.cmd_tx.send(CoreCmd::CreatePaste { content });
    }

    pub(super) fn delete_selected(&mut self) {
        if let Some(id) = self.selected_id.clone() {
            self.locks.unlock(&id);
            let _ = self.backend.cmd_tx.send(CoreCmd::DeletePaste { id });
        }
    }

    pub(super) fn mark_dirty(&mut self) {
        if self.selected_id.is_some() {
            self.save_status = SaveStatus::Dirty;
            self.last_edit_at = Some(Instant::now());
        }
    }

    pub(super) fn maybe_autosave(&mut self) {
        if self.save_in_flight || self.save_status != SaveStatus::Dirty {
            return;
        }
        let Some(last_edit) = self.last_edit_at else {
            return;
        };
        if last_edit.elapsed() < self.autosave_delay {
            return;
        }
        let Some(id) = self.selected_id.clone() else {
            return;
        };
        let content = self.active_snapshot();
        self.save_in_flight = true;
        self.save_status = SaveStatus::Saving;
        let _ = self
            .backend
            .cmd_tx
            .send(CoreCmd::UpdatePaste { id, content });
    }

    pub(super) fn selected_index(&self) -> Option<usize> {
        let id = self.selected_id.as_ref()?;
        self.pastes.iter().position(|paste| paste.id == *id)
    }
}
