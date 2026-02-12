//! State transitions for backend events, selection, and autosave flow.

use super::highlight::EditorLayoutCache;
use super::{
    LocalPasteApp, PaletteCopyAction, SaveStatus, SidebarCollection, StatusMessage, ToastMessage,
    SEARCH_DEBOUNCE, STATUS_TTL, TOAST_LIMIT, TOAST_TTL,
};
use crate::backend::{CoreCmd, CoreEvent, PasteSummary};
use chrono::{Duration as ChronoDuration, Utc};
use localpaste_core::models::paste::Paste;
use std::time::Instant;
use tracing::warn;

impl LocalPasteApp {
    pub(super) fn apply_event(&mut self, event: CoreEvent) {
        match event {
            CoreEvent::PasteList { items } => {
                self.all_pastes = items;
                if self.search_query.trim().is_empty() {
                    self.recompute_visible_pastes();
                    self.ensure_selection_after_list_update();
                }
            }
            CoreEvent::PasteLoaded { paste } => {
                if self.selected_id.as_deref() == Some(paste.id.as_str()) {
                    self.sync_editor_metadata(&paste);
                    self.selected_content.reset(paste.content.clone());
                    self.reset_virtual_editor(paste.content.as_str());
                    self.editor_cache = EditorLayoutCache::default();
                    self.editor_lines.reset();
                    self.virtual_selection.clear();
                    self.clear_highlight_state();
                    self.selected_paste = Some(paste);
                    self.try_complete_pending_copy();
                    self.save_status = SaveStatus::Saved;
                    self.last_edit_at = None;
                    self.save_in_flight = false;
                }
            }
            CoreEvent::PasteCreated { paste } => {
                let summary = PasteSummary::from_paste(&paste);
                self.all_pastes.insert(0, summary.clone());
                self.pastes.insert(0, summary);
                self.select_paste(paste.id.clone());
                self.sync_editor_metadata(&paste);
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
                if let Some(item) = self.all_pastes.iter_mut().find(|item| item.id == paste.id) {
                    *item = PasteSummary::from_paste(&paste);
                }
                if let Some(item) = self.pastes.iter_mut().find(|item| item.id == paste.id) {
                    *item = PasteSummary::from_paste(&paste);
                }
                if self.selected_id.as_deref() == Some(paste.id.as_str()) {
                    self.sync_editor_metadata(&paste);
                    let mut updated = paste;
                    updated.content = self.active_snapshot();
                    self.selected_paste = Some(updated);
                    self.save_status = SaveStatus::Saved;
                    self.last_edit_at = None;
                    self.save_in_flight = false;
                }
            }
            CoreEvent::PasteMetaSaved { paste } => {
                if let Some(item) = self.all_pastes.iter_mut().find(|item| item.id == paste.id) {
                    *item = PasteSummary::from_paste(&paste);
                }
                if let Some(item) = self.pastes.iter_mut().find(|item| item.id == paste.id) {
                    *item = PasteSummary::from_paste(&paste);
                }
                if self.selected_id.as_deref() == Some(paste.id.as_str()) {
                    self.sync_editor_metadata(&paste);
                    self.selected_paste = Some(paste);
                }
            }
            CoreEvent::SearchResults { query: _, items } => {
                self.pastes = self.filter_by_collection(&items);
                self.ensure_selection_after_list_update();
            }
            CoreEvent::PasteDeleted { id } => {
                self.all_pastes.retain(|paste| paste.id != id);
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
                self.all_pastes.retain(|paste| paste.id != id);
                self.pastes.retain(|paste| paste.id != id);
                if self.selected_id.as_deref() == Some(id.as_str()) {
                    self.clear_selection();
                    self.set_status("Selected paste was deleted; list refreshed.");
                } else {
                    self.set_status("Paste was deleted; list refreshed.");
                }
                self.request_refresh();
            }
            CoreEvent::FoldersLoaded { items } => {
                self.folders = items;
            }
            CoreEvent::FolderSaved { folder: _ } => {
                self.request_folder_refresh();
                self.request_refresh();
            }
            CoreEvent::FolderDeleted { id: _ } => {
                self.request_folder_refresh();
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
        let _ = self.backend.cmd_tx.send(CoreCmd::ListPastes {
            limit: 512,
            folder_id: None,
        });
        self.last_refresh_at = Instant::now();
    }

    pub(super) fn request_folder_refresh(&mut self) {
        let _ = self.backend.cmd_tx.send(CoreCmd::ListFolders);
    }

    pub(super) fn set_search_query(&mut self, query: String) {
        if self.search_query == query {
            return;
        }
        self.search_query = query;
        self.search_last_input_at = Some(Instant::now());
    }

    pub(super) fn set_active_collection(&mut self, collection: SidebarCollection) {
        if self.active_collection == collection {
            return;
        }
        self.active_collection = collection;
        self.search_last_sent.clear();
        if self.search_query.trim().is_empty() {
            self.recompute_visible_pastes();
            self.ensure_selection_after_list_update();
        } else {
            self.search_last_input_at = Some(Instant::now() - SEARCH_DEBOUNCE);
        }
    }

    pub(super) fn maybe_dispatch_search(&mut self) {
        let query = self.search_query.trim().to_string();
        if query.is_empty() {
            if !self.search_last_sent.is_empty() {
                self.search_last_sent.clear();
                self.recompute_visible_pastes();
                self.ensure_selection_after_list_update();
            }
            return;
        }

        if self.search_last_sent == query {
            return;
        }
        let Some(last_input_at) = self.search_last_input_at else {
            return;
        };
        if last_input_at.elapsed() < SEARCH_DEBOUNCE {
            return;
        }

        let (folder_id, language) = self.search_backend_filters();
        let _ = self.backend.cmd_tx.send(CoreCmd::SearchPastes {
            query: query.clone(),
            limit: 512,
            folder_id,
            language,
        });
        self.search_last_sent = query;
    }

    pub(super) fn select_paste(&mut self, id: String) {
        if let Some(prev) = self.selected_id.take() {
            self.locks.unlock(&prev);
        }
        self.selected_id = Some(id.clone());
        self.locks.lock(&id);
        self.selected_paste = None;
        self.edit_name.clear();
        self.edit_language = None;
        self.edit_language_is_manual = false;
        self.edit_folder_id = None;
        self.edit_tags.clear();
        self.metadata_dirty = false;
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
        self.edit_name.clear();
        self.edit_language = None;
        self.edit_language_is_manual = false;
        self.edit_folder_id = None;
        self.edit_tags.clear();
        self.metadata_dirty = false;
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
        let text = text.into();
        self.status = Some(StatusMessage {
            text: text.clone(),
            expires_at: Instant::now() + STATUS_TTL,
        });
        self.push_toast(text);
    }

    fn push_toast(&mut self, text: String) {
        let now = Instant::now();
        if let Some(last) = self.toasts.back_mut() {
            if last.text == text {
                last.expires_at = now + TOAST_TTL;
                return;
            }
        }
        self.toasts.push_back(ToastMessage {
            text,
            expires_at: now + TOAST_TTL,
        });
        while self.toasts.len() > TOAST_LIMIT {
            self.toasts.pop_front();
        }
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

    pub(super) fn save_now(&mut self) {
        if self.save_in_flight || self.save_status != SaveStatus::Dirty {
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

    pub(super) fn save_metadata_now(&mut self) {
        if !self.metadata_dirty {
            return;
        }
        let Some(id) = self.selected_id.clone() else {
            return;
        };
        let folder_id = Some(self.edit_folder_id.clone().unwrap_or_default());
        let language = if self.edit_language_is_manual {
            self.edit_language.clone()
        } else {
            None
        };
        let tags = Some(parse_tags_csv(self.edit_tags.as_str()));
        let _ = self.backend.cmd_tx.send(CoreCmd::UpdatePasteMeta {
            id,
            name: Some(self.edit_name.clone()),
            language,
            language_is_manual: Some(self.edit_language_is_manual),
            folder_id,
            tags,
        });
        self.metadata_dirty = false;
    }

    pub(super) fn export_selected_paste(&mut self) {
        let Some(paste_id) = self.selected_paste.as_ref().map(|paste| paste.id.clone()) else {
            self.set_status("Nothing selected to export.");
            return;
        };
        let extension = language_extension(self.edit_language.as_deref());
        let default_name = format!("{}.{}", sanitize_filename(&self.edit_name), extension);
        let dialog = rfd::FileDialog::new()
            .set_file_name(default_name.as_str())
            .add_filter("Text", &[extension]);
        let Some(path) = dialog.save_file() else {
            return;
        };

        let content = self.active_snapshot();
        match std::fs::write(&path, content) {
            Ok(()) => {
                self.set_status(format!(
                    "Exported {} to {}",
                    paste_id,
                    path.to_string_lossy()
                ));
            }
            Err(err) => {
                self.set_status(format!("Export failed: {}", err));
            }
        }
    }

    pub(super) fn selected_index(&self) -> Option<usize> {
        let id = self.selected_id.as_ref()?;
        self.pastes.iter().position(|paste| paste.id == *id)
    }

    pub(super) fn folder_paste_count(&self, folder_id: Option<&str>) -> usize {
        self.all_pastes
            .iter()
            .filter(|paste| paste.folder_id.as_deref() == folder_id)
            .count()
    }

    fn search_backend_filters(&self) -> (Option<String>, Option<String>) {
        match &self.active_collection {
            SidebarCollection::Folder(id) => (Some(id.clone()), None),
            SidebarCollection::Language(lang) => (None, Some(lang.clone())),
            _ => (None, None),
        }
    }

    fn filter_by_collection(&self, items: &[PasteSummary]) -> Vec<PasteSummary> {
        let now = Utc::now();
        let recent_cutoff = now - ChronoDuration::days(7);
        items
            .iter()
            .filter(|item| match &self.active_collection {
                SidebarCollection::All => true,
                SidebarCollection::Recent => item.updated_at >= recent_cutoff,
                SidebarCollection::Unfiled => item.folder_id.is_none(),
                SidebarCollection::Language(lang) => item
                    .language
                    .as_deref()
                    .map(|v| v.eq_ignore_ascii_case(lang))
                    .unwrap_or(false),
                SidebarCollection::Folder(id) => item.folder_id.as_deref() == Some(id.as_str()),
            })
            .cloned()
            .collect()
    }

    fn recompute_visible_pastes(&mut self) {
        self.pastes = self.filter_by_collection(&self.all_pastes);
    }

    fn ensure_selection_after_list_update(&mut self) {
        let selection_valid = self
            .selected_id
            .as_ref()
            .map(|id| self.pastes.iter().any(|p| p.id == *id))
            .unwrap_or(false);
        if selection_valid {
            return;
        }
        if let Some(first) = self.pastes.first() {
            self.select_paste(first.id.clone());
        } else {
            self.clear_selection();
        }
    }

    pub(super) fn sync_editor_metadata(&mut self, paste: &Paste) {
        self.edit_name = paste.name.clone();
        self.edit_language = paste.language.clone();
        self.edit_language_is_manual = paste.language_is_manual;
        self.edit_folder_id = paste.folder_id.clone();
        self.edit_tags = paste.tags.join(", ");
        self.metadata_dirty = false;
    }

    fn try_complete_pending_copy(&mut self) {
        let Some(action) = self.pending_copy_action.clone() else {
            return;
        };
        let Some(paste) = self.selected_paste.as_ref() else {
            return;
        };
        match action {
            PaletteCopyAction::Raw(id) => {
                if id != paste.id {
                    return;
                }
                self.clipboard_outgoing = Some(paste.content.clone());
                self.pending_copy_action = None;
                self.set_status("Copied paste content.");
            }
            PaletteCopyAction::Fenced(id) => {
                if id != paste.id {
                    return;
                }
                self.clipboard_outgoing = Some(format_fenced_block(
                    &paste.content,
                    paste.language.as_deref(),
                ));
                self.pending_copy_action = None;
                self.set_status("Copied fenced code block.");
            }
        }
    }
}

fn parse_tags_csv(input: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tag in input.split(',') {
        let trimmed = tag.trim();
        if trimmed.is_empty() {
            continue;
        }
        if out
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(trimmed))
        {
            continue;
        }
        out.push(trimmed.to_string());
    }
    out
}

fn language_extension(language: Option<&str>) -> &'static str {
    match language
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "rust" => "rs",
        "python" => "py",
        "javascript" => "js",
        "typescript" => "ts",
        "json" => "json",
        "yaml" => "yaml",
        "toml" => "toml",
        "markdown" => "md",
        "html" => "html",
        "css" => "css",
        "sql" => "sql",
        "shell" => "sh",
        _ => "txt",
    }
}

fn sanitize_filename(value: &str) -> String {
    let mut out: String = value
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ => ch,
        })
        .collect();
    out = out.trim().to_string();
    if out.is_empty() {
        "localpaste-export".to_string()
    } else {
        out
    }
}

fn format_fenced_block(content: &str, language: Option<&str>) -> String {
    let lang = language.unwrap_or("text");
    format!("```{}\n{}\n```", lang, content)
}
