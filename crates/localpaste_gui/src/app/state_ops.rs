//! State transitions for backend events, selection, and autosave flow.

mod filters;

use super::highlight::EditorLayoutCache;
use super::util::format_fenced_code_block;
use super::{
    LocalPasteApp, PaletteCopyAction, SaveStatus, SidebarCollection, StatusMessage, ToastMessage,
    PALETTE_SEARCH_LIMIT, SEARCH_DEBOUNCE, STATUS_TTL, TOAST_LIMIT, TOAST_TTL,
};
use crate::backend::{CoreCmd, CoreErrorSource, CoreEvent, PasteSummary};
use chrono::{Duration as ChronoDuration, Local, Utc};
use localpaste_core::{
    models::paste::Paste, DEFAULT_LIST_PASTES_LIMIT, DEFAULT_SEARCH_PASTES_LIMIT,
};
use std::collections::BTreeSet;
use std::time::Instant;
use tracing::warn;

use self::filters::{
    is_code_summary, is_config_summary, is_link_summary, is_log_summary, language_extension,
    normalize_language_filter_value, parse_tags_csv, sanitize_filename,
};

impl LocalPasteApp {
    fn send_update_paste_or_mark_failed(&mut self, command: CoreCmd, mode: &str) -> bool {
        if self.backend.cmd_tx.send(command).is_ok() {
            return true;
        }
        self.save_in_flight = false;
        self.save_status = SaveStatus::Dirty;
        self.save_request_revision = None;
        self.last_edit_at = Some(Instant::now());
        self.set_status(format!("{mode} failed: backend unavailable."));
        false
    }

    fn dispatch_content_save(&mut self, id: String, mode: &str) -> bool {
        self.save_request_revision = Some(self.active_revision());
        self.save_in_flight = true;
        self.save_status = SaveStatus::Saving;

        let command = if self.is_virtual_editor_mode() {
            CoreCmd::UpdatePasteVirtual {
                id,
                content: self.virtual_editor_buffer.rope().clone(),
            }
        } else {
            CoreCmd::UpdatePaste {
                id,
                content: self.selected_content.to_string(),
            }
        };

        self.send_update_paste_or_mark_failed(command, mode)
    }

    pub(super) fn apply_event(&mut self, event: CoreEvent) {
        match event {
            CoreEvent::PasteList { items } => {
                self.query_perf.list_results_applied =
                    self.query_perf.list_results_applied.saturating_add(1);
                if let Some(sent_at) = self.query_perf.list_last_sent_at.take() {
                    self.query_perf.list_last_roundtrip_ms =
                        Some(sent_at.elapsed().as_secs_f32() * 1000.0);
                }
                let list_changed = self.all_pastes != items;
                self.all_pastes = items;
                if self.search_query.trim().is_empty() {
                    self.recompute_visible_pastes();
                    self.ensure_selection_after_list_update();
                } else if list_changed {
                    // External API/CLI writes arrive via list refresh, not local save events.
                    // Force one fresh backend search so active query results stay in sync.
                    self.search_last_sent.clear();
                    self.search_last_input_at = Some(Instant::now() - SEARCH_DEBOUNCE);
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
                    self.save_request_revision = None;
                    self.metadata_save_in_flight = false;
                }
            }
            CoreEvent::PasteCreated { paste } => {
                let summary = PasteSummary::from_paste(&paste);
                self.all_pastes.insert(0, summary.clone());
                self.pastes.insert(0, summary);
                self.select_paste(paste.id.clone());
                // Keep current editor/save state untouched when switching is deferred.
                if self.selected_id.as_deref() == Some(paste.id.as_str()) {
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
                    self.save_request_revision = None;
                    self.metadata_save_in_flight = false;
                    self.focus_editor_next = true;
                    self.set_status("Created new paste.");
                }
            }
            CoreEvent::PasteSaved { paste } => {
                let requested_revision = self.save_request_revision.take();
                if let Some(item) = self.all_pastes.iter_mut().find(|item| item.id == paste.id) {
                    *item = PasteSummary::from_paste(&paste);
                }
                if let Some(item) = self.pastes.iter_mut().find(|item| item.id == paste.id) {
                    *item = PasteSummary::from_paste(&paste);
                }
                if !self.search_query.trim().is_empty() {
                    // Content saves can update metadata used by metadata-only search
                    // (language auto-detect, recency ordering), so force redispatch.
                    self.search_last_sent.clear();
                    self.search_last_input_at = Some(Instant::now() - SEARCH_DEBOUNCE);
                }
                if self.selected_id.as_deref() == Some(paste.id.as_str()) {
                    let has_newer_local_edits = if self.is_virtual_editor_mode() {
                        // `save_request_revision` can be cleared after a partial deferred-switch
                        // failure even when a content-save command was already dispatched.
                        // Use snapshot comparison as a safe fallback for late save acks.
                        requested_revision
                            .map(|revision| self.active_revision() != revision)
                            .unwrap_or_else(|| self.active_snapshot() != paste.content)
                    } else {
                        self.selected_content.as_str() != paste.content
                    };
                    if !self.metadata_dirty && !self.metadata_save_in_flight {
                        self.sync_editor_metadata(&paste);
                    }
                    self.selected_paste = Some(paste);
                    self.save_in_flight = false;
                    if has_newer_local_edits {
                        // Keep autosave armed when this ack corresponds to an older snapshot.
                        self.save_status = SaveStatus::Dirty;
                        if self.last_edit_at.is_none() {
                            self.last_edit_at = Some(Instant::now());
                        }
                    } else {
                        self.save_status = SaveStatus::Saved;
                        self.last_edit_at = None;
                    }
                }
                self.try_apply_pending_selection();
            }
            CoreEvent::PasteMetaSaved { paste } => {
                self.metadata_save_in_flight = false;
                if let Some(item) = self.all_pastes.iter_mut().find(|item| item.id == paste.id) {
                    *item = PasteSummary::from_paste(&paste);
                }
                if self.selected_id.as_deref() == Some(paste.id.as_str()) {
                    self.sync_editor_metadata(&paste);
                    self.selected_paste = Some(paste.clone());
                }
                if self.search_query.trim().is_empty() {
                    self.recompute_visible_pastes();
                } else {
                    let visible = self.pastes.clone();
                    self.pastes = self.filter_by_collection(&visible);
                    // Metadata edits can change search inclusion/ranking; force a fresh
                    // backend search even when the query text itself is unchanged.
                    self.search_last_sent.clear();
                    self.search_last_input_at = Some(Instant::now() - SEARCH_DEBOUNCE);
                }
                self.ensure_selection_after_list_update();
                self.try_apply_pending_selection();
            }
            CoreEvent::SearchResults {
                query,
                folder_id,
                language,
                items,
            } => {
                // Drop stale search responses when query or backend filter context changed.
                let active_query = self.search_query.trim();
                let expected_sent_query = self.search_last_sent.trim();
                let (expected_folder_id, expected_language) = self.search_backend_filters();
                let response_language = normalize_language_filter_value(language.as_deref());
                if active_query.is_empty()
                    || query.trim() != active_query
                    || query.trim() != expected_sent_query
                    || folder_id != expected_folder_id
                    || response_language != expected_language
                {
                    self.query_perf.search_stale_drops =
                        self.query_perf.search_stale_drops.saturating_add(1);
                    return;
                }
                self.query_perf.search_results_applied =
                    self.query_perf.search_results_applied.saturating_add(1);
                if let Some(sent_at) = self.query_perf.search_last_sent_at.take() {
                    self.query_perf.search_last_roundtrip_ms =
                        Some(sent_at.elapsed().as_secs_f32() * 1000.0);
                }
                self.pastes = self.filter_by_collection(&items);
                self.ensure_selection_after_list_update();
            }
            CoreEvent::PaletteSearchResults { query, items } => {
                if !self.command_palette_open
                    || self.command_palette_query.trim().is_empty()
                    || query.trim() != self.command_palette_query.trim()
                {
                    return;
                }
                self.palette_search_results = items;
                if self.command_palette_selected >= self.palette_search_results.len() {
                    self.command_palette_selected =
                        self.palette_search_results.len().saturating_sub(1);
                }
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
                self.clear_pending_copy_for(id.as_str());
                if self.selected_id.as_deref() == Some(id.as_str()) {
                    self.clear_selection();
                    self.set_status("Selected paste was deleted; list refreshed.");
                } else {
                    self.set_status("Paste was deleted; list refreshed.");
                }
                self.request_refresh();
            }
            CoreEvent::PasteLoadFailed { id, message } => {
                self.clear_pending_copy_for(id.as_str());
                if self.selected_id.as_deref() == Some(id.as_str()) {
                    self.clear_selection();
                }
                self.set_status(message);
            }
            CoreEvent::FoldersLoaded { items: _ } => {}
            CoreEvent::FolderSaved { folder: _ } | CoreEvent::FolderDeleted { id: _ } => {
                self.request_refresh();
            }
            CoreEvent::Error { source, message } => {
                warn!("backend error ({:?}): {}", source, message);
                // Only mutate save-in-flight state for the matching request class.
                // Generic backend errors (search/list/folder ops) should not cancel
                // unrelated metadata/content saves that are still awaiting an ack.
                match source {
                    CoreErrorSource::SaveMetadata if self.metadata_save_in_flight => {
                        self.metadata_dirty = true;
                        self.metadata_save_in_flight = false;
                        if let Some(pending) = self.pending_selection_id.take() {
                            self.clear_pending_copy_for(pending.as_str());
                        }
                        if message.to_ascii_lowercase().contains("metadata") {
                            self.set_status(message);
                        } else {
                            self.set_status(format!("Metadata save failed: {}", message));
                        }
                    }
                    CoreErrorSource::SaveContent if self.save_in_flight => {
                        if self.save_status == SaveStatus::Saving {
                            self.save_status = SaveStatus::Dirty;
                        }
                        self.save_in_flight = false;
                        self.save_request_revision = None;
                        if let Some(pending) = self.pending_selection_id.take() {
                            self.clear_pending_copy_for(pending.as_str());
                        }
                        self.set_status(message);
                    }
                    _ => self.set_status(message),
                }
            }
        }
    }

    pub(super) fn request_refresh(&mut self) {
        let sent_at = Instant::now();
        if self
            .backend
            .cmd_tx
            .send(CoreCmd::ListPastes {
                limit: DEFAULT_LIST_PASTES_LIMIT,
                folder_id: None,
            })
            .is_err()
        {
            self.set_status("List failed: backend unavailable.");
            return;
        }
        self.query_perf.list_requests_sent = self.query_perf.list_requests_sent.saturating_add(1);
        self.query_perf.list_last_sent_at = Some(sent_at);
        self.last_refresh_at = sent_at;
    }

    pub(super) fn set_search_query(&mut self, query: String) {
        if self.search_query == query {
            return;
        }
        self.search_query = query;
        self.search_last_input_at = Some(Instant::now());
    }

    pub(super) fn set_command_palette_query(&mut self, query: String) {
        if self.command_palette_query == query {
            return;
        }
        self.command_palette_query = query;
        self.command_palette_selected = 0;
        self.palette_search_last_input_at = Some(Instant::now());
        if self.command_palette_query.trim().is_empty() {
            self.palette_search_last_sent.clear();
            self.palette_search_results.clear();
        }
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

    pub(super) fn set_active_language_filter(&mut self, language: Option<String>) {
        let normalized = normalize_language_filter_value(language.as_deref());
        if self.active_language_filter == normalized {
            return;
        }
        self.active_language_filter = normalized;
        self.search_last_sent.clear();
        if self.search_query.trim().is_empty() {
            self.recompute_visible_pastes();
            self.ensure_selection_after_list_update();
        } else {
            self.search_last_input_at = Some(Instant::now() - SEARCH_DEBOUNCE);
        }
    }

    pub(super) fn language_filter_options(&self) -> Vec<String> {
        let mut langs: BTreeSet<String> = BTreeSet::new();
        for paste in &self.all_pastes {
            if let Some(lang) = normalize_language_filter_value(paste.language.as_deref()) {
                langs.insert(lang);
            }
        }
        langs.into_iter().collect()
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
            self.query_perf.search_skipped_cached =
                self.query_perf.search_skipped_cached.saturating_add(1);
            return;
        }
        let Some(last_input_at) = self.search_last_input_at else {
            return;
        };
        if last_input_at.elapsed() < SEARCH_DEBOUNCE {
            self.query_perf.search_skipped_debounce =
                self.query_perf.search_skipped_debounce.saturating_add(1);
            return;
        }

        let (folder_id, language) = self.search_backend_filters();
        if self
            .backend
            .cmd_tx
            .send(CoreCmd::SearchPastes {
                query: query.clone(),
                limit: DEFAULT_SEARCH_PASTES_LIMIT,
                folder_id,
                language,
            })
            .is_err()
        {
            self.set_status("Search failed: backend unavailable.");
            return;
        }
        self.search_last_sent = query;
        self.query_perf.search_requests_sent =
            self.query_perf.search_requests_sent.saturating_add(1);
        self.query_perf.search_last_sent_at = Some(Instant::now());
    }

    pub(super) fn maybe_dispatch_palette_search(&mut self) {
        if !self.command_palette_open {
            return;
        }

        let query = self.command_palette_query.trim().to_string();
        if query.is_empty() {
            if !self.palette_search_last_sent.is_empty() || !self.palette_search_results.is_empty()
            {
                self.palette_search_last_sent.clear();
                self.palette_search_results.clear();
            }
            return;
        }

        if self.palette_search_last_sent == query {
            return;
        }
        let Some(last_input_at) = self.palette_search_last_input_at else {
            return;
        };
        if last_input_at.elapsed() < SEARCH_DEBOUNCE {
            return;
        }

        if self
            .backend
            .cmd_tx
            .send(CoreCmd::SearchPalette {
                query: query.clone(),
                limit: PALETTE_SEARCH_LIMIT,
            })
            .is_err()
        {
            self.set_status("Command palette search failed: backend unavailable.");
            return;
        }
        self.palette_search_last_sent = query;
    }

    pub(super) fn select_paste(&mut self, id: String) -> bool {
        if self.selected_id.as_deref() == Some(id.as_str()) {
            return true;
        }
        if self.save_status == SaveStatus::Dirty || self.metadata_dirty {
            self.pending_selection_id = Some(id.clone());
            let content_save_needed = self.save_status == SaveStatus::Dirty;
            let metadata_save_needed = self.metadata_dirty;
            if content_save_needed {
                self.save_now();
            }
            if metadata_save_needed {
                self.save_metadata_now();
            }

            let content_save_dispatched = !content_save_needed || self.save_in_flight;
            let metadata_save_dispatched = !metadata_save_needed || self.metadata_save_in_flight;
            if !content_save_dispatched || !metadata_save_dispatched {
                // If one save dispatch succeeded and the next failed, treat the whole
                // deferred switch attempt as failed and roll back save in-flight flags.
                if content_save_needed && self.save_in_flight {
                    self.save_in_flight = false;
                    self.save_status = SaveStatus::Dirty;
                    self.save_request_revision = None;
                    if self.last_edit_at.is_none() {
                        self.last_edit_at = Some(Instant::now());
                    }
                }
                if metadata_save_needed && self.metadata_save_in_flight {
                    self.metadata_save_in_flight = false;
                    self.metadata_dirty = true;
                }
                if let Some(pending) = self.pending_selection_id.take() {
                    self.clear_pending_copy_for(pending.as_str());
                }
                return false;
            }
            self.set_status("Saving current paste before switching...");
            return true;
        }
        self.apply_selection_now(id)
    }

    fn apply_selection_now(&mut self, id: String) -> bool {
        if let Some(prev) = self.selected_id.take() {
            self.locks.unlock(&prev);
        }
        self.selected_id = Some(id.clone());
        self.locks.lock(&id);
        self.selected_paste = None;
        self.edit_name.clear();
        self.edit_language = None;
        self.edit_language_is_manual = false;
        self.edit_tags.clear();
        self.metadata_dirty = false;
        self.metadata_save_in_flight = false;
        self.selected_content.reset(String::new());
        self.reset_virtual_editor("");
        self.editor_cache = EditorLayoutCache::default();
        self.editor_lines.reset();
        self.virtual_selection.clear();
        self.clear_highlight_state();
        self.save_status = SaveStatus::Saved;
        self.last_edit_at = None;
        self.save_in_flight = false;
        self.save_request_revision = None;
        if self.backend.cmd_tx.send(CoreCmd::GetPaste { id }).is_err() {
            self.clear_selection();
            self.set_status("Get paste failed: backend unavailable.");
            return false;
        }
        true
    }

    fn try_apply_pending_selection(&mut self) {
        if self.save_in_flight || self.metadata_save_in_flight {
            return;
        }
        if self.save_status == SaveStatus::Dirty || self.metadata_dirty {
            return;
        }
        let Some(pending) = self.pending_selection_id.take() else {
            return;
        };
        let _ = self.apply_selection_now(pending);
    }

    pub(super) fn clear_selection(&mut self) {
        if let Some(pending) = self.pending_selection_id.take() {
            self.clear_pending_copy_for(pending.as_str());
        }
        if let Some(prev) = self.selected_id.take() {
            self.locks.unlock(&prev);
        }
        self.selected_paste = None;
        self.edit_name.clear();
        self.edit_language = None;
        self.edit_language_is_manual = false;
        self.edit_tags.clear();
        self.metadata_dirty = false;
        self.metadata_save_in_flight = false;
        self.selected_content.reset(String::new());
        self.reset_virtual_editor("");
        self.editor_cache = EditorLayoutCache::default();
        self.editor_lines.reset();
        self.virtual_selection.clear();
        self.clear_highlight_state();
        self.save_status = SaveStatus::Saved;
        self.last_edit_at = None;
        self.save_in_flight = false;
        self.save_request_revision = None;
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
        if self
            .backend
            .cmd_tx
            .send(CoreCmd::CreatePaste { content })
            .is_err()
        {
            self.set_status("Create failed: backend unavailable.");
        }
    }

    pub(super) fn delete_selected(&mut self) {
        if let Some(id) = self.selected_id.clone() {
            if self
                .backend
                .cmd_tx
                .send(CoreCmd::DeletePaste { id })
                .is_err()
            {
                self.set_status("Delete failed: backend unavailable.");
            }
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
        let _sent = self.dispatch_content_save(id, "Autosave");
    }

    pub(super) fn save_now(&mut self) {
        if self.save_in_flight || self.save_status != SaveStatus::Dirty {
            return;
        }
        let Some(id) = self.selected_id.clone() else {
            return;
        };
        let _sent = self.dispatch_content_save(id, "Save");
    }

    pub(super) fn save_metadata_now(&mut self) {
        if !self.metadata_dirty || self.metadata_save_in_flight {
            return;
        }
        let Some(id) = self.selected_id.clone() else {
            return;
        };
        let language = if self.edit_language_is_manual {
            self.edit_language.clone()
        } else {
            None
        };
        let tags = Some(parse_tags_csv(self.edit_tags.as_str()));
        if self
            .backend
            .cmd_tx
            .send(CoreCmd::UpdatePasteMeta {
                id,
                name: Some(self.edit_name.clone()),
                language,
                language_is_manual: Some(self.edit_language_is_manual),
                folder_id: None,
                tags,
            })
            .is_err()
        {
            self.set_status("Metadata save failed: backend unavailable.");
            return;
        }
        self.metadata_save_in_flight = true;
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

    fn search_backend_filters(&self) -> (Option<String>, Option<String>) {
        (None, self.active_language_filter.clone())
    }

    fn filter_by_collection(&self, items: &[PasteSummary]) -> Vec<PasteSummary> {
        let now = Utc::now();
        let today_local = Local::now().date_naive();
        let week_cutoff = now - ChronoDuration::days(7);
        let recent_cutoff = now - ChronoDuration::days(30);
        items
            .iter()
            .filter(|item| {
                let collection_match = match &self.active_collection {
                    SidebarCollection::All => true,
                    SidebarCollection::Today => {
                        item.updated_at.with_timezone(&Local).date_naive() == today_local
                    }
                    SidebarCollection::Week => item.updated_at >= week_cutoff,
                    SidebarCollection::Recent => item.updated_at >= recent_cutoff,
                    SidebarCollection::Unfiled => item.folder_id.is_none(),
                    SidebarCollection::Code => is_code_summary(item),
                    SidebarCollection::Config => is_config_summary(item),
                    SidebarCollection::Logs => is_log_summary(item),
                    SidebarCollection::Links => is_link_summary(item),
                };
                if !collection_match {
                    return false;
                }
                match &self.active_language_filter {
                    None => true,
                    Some(lang) => item
                        .language
                        .as_deref()
                        .map(|v| v.eq_ignore_ascii_case(lang))
                        .unwrap_or(false),
                }
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
                self.clipboard_outgoing = Some(format_fenced_code_block(
                    &paste.content,
                    paste.language.as_deref(),
                ));
                self.pending_copy_action = None;
                self.set_status("Copied fenced code block.");
            }
        }
    }

    fn clear_pending_copy_for(&mut self, id: &str) {
        let should_clear = matches!(
            self.pending_copy_action.as_ref(),
            Some(PaletteCopyAction::Raw(action_id) | PaletteCopyAction::Fenced(action_id))
                if action_id == id
        );
        if should_clear {
            self.pending_copy_action = None;
        }
    }
}
