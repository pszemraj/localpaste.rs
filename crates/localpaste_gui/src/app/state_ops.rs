//! State transitions for backend events, selection, and autosave flow.

mod filters;

use super::highlight::EditorLayoutCache;
use super::util::format_fenced_code_block;
use super::{
    ExportCompletion, LocalPasteApp, MetadataDraftSnapshot, PaletteCopyAction, SaveStatus,
    SidebarCollection, PALETTE_SEARCH_LIMIT, SEARCH_DEBOUNCE,
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
    fn send_backend_cmd_or_status(&mut self, command: CoreCmd, error_message: &str) -> bool {
        if self.backend.cmd_tx.send(command).is_ok() {
            return true;
        }
        self.set_status(error_message);
        false
    }

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

    /// Applies a backend event and synchronizes app state, selection, and save flags.
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
                    self.select_loaded_paste(paste);
                }
            }
            CoreEvent::PasteCreated { paste } => {
                let summary = PasteSummary::from_paste(&paste);
                self.all_pastes.insert(0, summary.clone());
                self.pastes.insert(0, summary);
                let has_unsaved_edits =
                    self.save_status == SaveStatus::Dirty || self.metadata_dirty;
                let save_in_progress = self.save_in_flight
                    || self.metadata_save_in_flight
                    || self.save_status == SaveStatus::Saving;
                if has_unsaved_edits || save_in_progress {
                    // Keep current editor/save state untouched when switching is deferred.
                    self.select_paste(paste.id.clone());
                    return;
                }
                self.select_loaded_paste(paste);
                self.pending_selection_id = None;
                self.focus_editor_next = true;
                self.set_status("Created new paste.");
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
                if self.search_query.trim().is_empty() {
                    // Keep smart collections/language filtering in sync with save-driven
                    // summary changes (language auto-detect, updated_at ordering).
                    self.recompute_visible_pastes();
                }
                self.try_apply_pending_selection();
                if self.search_query.trim().is_empty() {
                    self.ensure_selection_after_list_update();
                }
            }
            CoreEvent::PasteMetaSaved { paste } => {
                let requested_metadata = self.metadata_save_request.take();
                self.metadata_save_in_flight = false;
                if let Some(item) = self.all_pastes.iter_mut().find(|item| item.id == paste.id) {
                    *item = PasteSummary::from_paste(&paste);
                }
                if self.selected_id.as_deref() == Some(paste.id.as_str()) {
                    if self.metadata_matches_request(requested_metadata.as_ref()) {
                        self.sync_editor_metadata(&paste);
                    } else {
                        self.metadata_dirty = true;
                    }
                    self.selected_paste = Some(paste.clone());
                }
                if self.search_query.trim().is_empty() {
                    self.recompute_visible_pastes();
                } else {
                    self.retain_search_results_for_active_filters();
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
                // `command_palette_selected` is an absolute index across commands + results.
                // Clamp in that same combined space so async result updates never remap into commands.
                self.clamp_command_palette_selection_with_results_len(
                    self.palette_search_results.len(),
                );
            }
            CoreEvent::PasteDeleted { id } => {
                self.all_pastes.retain(|paste| paste.id != id);
                self.pastes.retain(|paste| paste.id != id);
                self.clear_pending_copy_for(id.as_str());
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
            CoreEvent::FoldersLoaded { items: _ }
            | CoreEvent::ShutdownComplete { flush_result: _ } => {}
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
                        self.metadata_save_request = None;
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

    /// Requests a fresh paste list from the backend and updates query perf counters.
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

    /// Updates the sidebar search query and starts debounce timing.
    pub(super) fn set_search_query(&mut self, query: String) {
        if self.search_query == query {
            return;
        }
        self.search_query = query;
        self.search_last_input_at = Some(Instant::now());
    }

    /// Updates command-palette query text and resets palette selection/search state.
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

    fn on_primary_filter_changed(&mut self) {
        self.search_last_sent.clear();
        if self.search_query.trim().is_empty() {
            self.recompute_visible_pastes();
            self.ensure_selection_after_list_update();
        } else {
            self.search_last_input_at = Some(Instant::now() - SEARCH_DEBOUNCE);
        }
    }

    /// Switches the active smart collection filter and triggers list/search refresh behavior.
    pub(super) fn set_active_collection(&mut self, collection: SidebarCollection) {
        if self.active_collection == collection {
            return;
        }
        self.active_collection = collection;
        self.on_primary_filter_changed();
    }

    /// Sets the active language filter after canonical normalization.
    pub(super) fn set_active_language_filter(&mut self, language: Option<String>) {
        let normalized = normalize_language_filter_value(language.as_deref());
        if self.active_language_filter == normalized {
            return;
        }
        self.active_language_filter = normalized;
        self.on_primary_filter_changed();
    }

    /// Builds sorted language filter options from the currently known paste summaries.
    ///
    /// # Returns
    /// Canonicalized language values in ascending sort order.
    pub(super) fn language_filter_options(&self) -> Vec<String> {
        let mut langs: BTreeSet<String> = BTreeSet::new();
        for paste in &self.all_pastes {
            if let Some(lang) = normalize_language_filter_value(paste.language.as_deref()) {
                langs.insert(lang);
            }
        }
        langs.into_iter().collect()
    }

    /// Dispatches a debounced sidebar search request when inputs and filters are ready.
    pub(super) fn maybe_dispatch_search(&mut self) {
        let query = self.search_query.trim().to_string();
        if query.is_empty() {
            let should_restore_list =
                self.search_last_input_at.take().is_some() || !self.search_last_sent.is_empty();
            if should_restore_list {
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

    /// Dispatches a debounced command-palette search request when applicable.
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

    /// Selects a paste by id, deferring selection when unsaved edits must be flushed first.
    ///
    /// # Returns
    /// `true` when selection was applied or successfully deferred, `false` on dispatch failure.
    pub(super) fn select_paste(&mut self, id: String) -> bool {
        if self.selected_id.as_deref() == Some(id.as_str()) {
            return true;
        }
        if self.save_status == SaveStatus::Dirty || self.metadata_dirty {
            self.queue_pending_selection(id);
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
                    self.metadata_save_request = None;
                }
                if let Some(pending) = self.pending_selection_id.take() {
                    self.clear_pending_copy_for(pending.as_str());
                }
                return false;
            }
            self.set_status("Saving current paste before switching...");
            return true;
        }
        let save_in_progress = self.save_in_flight
            || self.metadata_save_in_flight
            || self.save_status == SaveStatus::Saving;
        if save_in_progress {
            self.queue_pending_selection(id);
            self.set_status("Saving current paste before switching...");
            return true;
        }
        self.apply_selection_now(id)
    }

    fn queue_pending_selection(&mut self, id: String) {
        if self.pending_selection_id.as_deref() == Some(id.as_str()) {
            return;
        }
        if let Some(replaced) = self.pending_selection_id.replace(id) {
            self.clear_pending_copy_for(replaced.as_str());
        }
    }

    /// Applies a fully loaded paste into editor state and resets transient edit caches.
    pub(super) fn select_loaded_paste(&mut self, paste: Paste) {
        let id = paste.id.clone();
        if self.selected_id.as_deref() != Some(id.as_str()) {
            if !self.acquire_paste_lock(id.as_str()) {
                return;
            }
            if let Some(prev) = self.selected_id.replace(id.clone()) {
                self.release_paste_lock(prev.as_str());
            }
        }
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
        self.metadata_save_request = None;
    }

    fn reset_selection_editor_state(&mut self) {
        self.selected_paste = None;
        self.edit_name.clear();
        self.edit_language = None;
        self.edit_language_is_manual = false;
        self.edit_tags.clear();
        self.metadata_dirty = false;
        self.metadata_save_in_flight = false;
        self.metadata_save_request = None;
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

    fn apply_selection_now(&mut self, id: String) -> bool {
        // Acquire target lock before releasing current selection lock so failed
        // switches never drop the currently editable paste unexpectedly.
        if !self.acquire_paste_lock(id.as_str()) {
            return false;
        }
        if let Some(prev) = self.selected_id.replace(id.clone()) {
            self.release_paste_lock(prev.as_str());
        }
        self.reset_selection_editor_state();
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
        if self.save_status == SaveStatus::Saving {
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

    /// Clears active/pending selection and releases any held paste lock.
    pub(super) fn clear_selection(&mut self) {
        if let Some(pending) = self.pending_selection_id.take() {
            self.clear_pending_copy_for(pending.as_str());
        }
        if let Some(prev) = self.selected_id.take() {
            self.release_paste_lock(prev.as_str());
        }
        self.reset_selection_editor_state();
    }

    /// Creates a new empty paste.
    pub(super) fn create_new_paste(&mut self) {
        self.create_new_paste_with_content(String::new());
    }

    /// Creates a new paste pre-populated with `content`.
    pub(super) fn create_new_paste_with_content(&mut self, content: String) {
        let _sent = self.send_backend_cmd_or_status(
            CoreCmd::CreatePaste { content },
            "Create failed: backend unavailable.",
        );
    }

    /// Sends a delete command for `id` and reports whether dispatch succeeded.
    ///
    /// # Returns
    /// `true` when the backend command was queued, otherwise `false`.
    pub(super) fn send_delete_paste(&mut self, id: String) -> bool {
        self.send_backend_cmd_or_status(
            CoreCmd::DeletePaste { id },
            "Delete failed: backend unavailable.",
        )
    }

    /// Deletes the currently selected paste, if any.
    pub(super) fn delete_selected(&mut self) {
        if let Some(id) = self.selected_id.clone() {
            let _sent = self.send_delete_paste(id);
        }
    }

    /// Marks current editor content dirty and arms autosave timing.
    pub(super) fn mark_dirty(&mut self) {
        if self.selected_id.is_some() {
            self.save_status = SaveStatus::Dirty;
            self.last_edit_at = Some(Instant::now());
            if !self.is_virtual_editor_mode() {
                self.highlight_edit_hint = None;
            }
        }
    }

    /// Dispatches autosave once dirty content has been idle past the autosave delay.
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

    /// Forces immediate content save dispatch when the current paste is dirty.
    pub(super) fn save_now(&mut self) {
        if self.save_in_flight || self.save_status != SaveStatus::Dirty {
            return;
        }
        let Some(id) = self.selected_id.clone() else {
            return;
        };
        let _sent = self.dispatch_content_save(id, "Save");
    }

    /// Dispatches metadata save for the current editor metadata draft when needed.
    pub(super) fn save_metadata_now(&mut self) {
        if !self.metadata_dirty || self.metadata_save_in_flight {
            return;
        }
        let Some(id) = self.selected_id.clone() else {
            return;
        };
        let request = self.metadata_draft_snapshot();
        let language = if self.edit_language_is_manual {
            self.edit_language.clone()
        } else {
            None
        };
        let tags = Some(parse_tags_csv(self.edit_tags.as_str()));
        self.metadata_save_request = None;
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
        self.metadata_save_request = Some(request);
    }

    /// Starts asynchronous export of the selected paste to a user-chosen file path.
    pub(super) fn export_selected_paste(&mut self) {
        let Some(paste_id) = self.selected_paste.as_ref().map(|paste| paste.id.clone()) else {
            self.set_status("Nothing selected to export.");
            return;
        };
        if self.export_result_rx.is_some() {
            self.set_status("Export already in progress.");
            return;
        }
        let extension = language_extension(self.edit_language.as_deref());
        let default_name = format!("{}.{}", sanitize_filename(&self.edit_name), extension);
        let dialog = rfd::FileDialog::new()
            .set_file_name(default_name.as_str())
            .add_filter("Text", &[extension]);
        let Some(path) = dialog.save_file() else {
            return;
        };

        let content = self.active_snapshot();
        let path_for_write = path.clone();
        let completion = ExportCompletion {
            paste_id,
            path: path.to_string_lossy().to_string(),
            result: Ok(()),
        };
        let (tx, rx) = std::sync::mpsc::channel();
        self.export_result_rx = Some(rx);
        std::thread::spawn(move || {
            let mut completion = completion;
            completion.result =
                std::fs::write(&path_for_write, content).map_err(|err| err.to_string());
            let _ = tx.send(completion);
        });
        self.set_status("Export started...");
    }

    /// Returns the visible-list index of the selected paste.
    ///
    /// # Returns
    /// `Some(index)` when the selected paste is visible, otherwise `None`.
    pub(super) fn selected_index(&self) -> Option<usize> {
        let id = self.selected_id.as_ref()?;
        self.pastes.iter().position(|paste| paste.id == *id)
    }

    fn search_backend_filters(&self) -> (Option<String>, Option<String>) {
        (None, self.active_language_filter.clone())
    }

    fn matches_active_filters(
        item: &PasteSummary,
        active_collection: &SidebarCollection,
        active_language_filter: Option<&str>,
        today_local: chrono::NaiveDate,
        week_cutoff: chrono::DateTime<Utc>,
        recent_cutoff: chrono::DateTime<Utc>,
    ) -> bool {
        let collection_match = match active_collection {
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
        match active_language_filter {
            None => true,
            Some(lang) => {
                let canonical_filter = localpaste_core::detection::canonical::canonicalize(lang);
                item.language
                    .as_deref()
                    .map(localpaste_core::detection::canonical::canonicalize)
                    .map(|value| value == canonical_filter)
                    .unwrap_or(false)
            }
        }
    }

    fn filter_by_collection(&self, items: &[PasteSummary]) -> Vec<PasteSummary> {
        let now = Utc::now();
        let today_local = Local::now().date_naive();
        let week_cutoff = now - ChronoDuration::days(7);
        let recent_cutoff = now - ChronoDuration::days(30);
        let active_language_filter = self.active_language_filter.as_deref();
        items
            .iter()
            .filter(|item| {
                Self::matches_active_filters(
                    item,
                    &self.active_collection,
                    active_language_filter,
                    today_local,
                    week_cutoff,
                    recent_cutoff,
                )
            })
            .cloned()
            .collect()
    }

    fn retain_search_results_for_active_filters(&mut self) {
        let now = Utc::now();
        let today_local = Local::now().date_naive();
        let week_cutoff = now - ChronoDuration::days(7);
        let recent_cutoff = now - ChronoDuration::days(30);
        let active_collection = self.active_collection.clone();
        let active_language_filter = self.active_language_filter.clone();
        self.pastes.retain(|item| {
            Self::matches_active_filters(
                item,
                &active_collection,
                active_language_filter.as_deref(),
                today_local,
                week_cutoff,
                recent_cutoff,
            )
        });
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

    fn metadata_draft_snapshot(&self) -> MetadataDraftSnapshot {
        MetadataDraftSnapshot {
            name: self.edit_name.clone(),
            language: self.edit_language.clone(),
            language_is_manual: self.edit_language_is_manual,
            tags_csv: self.edit_tags.clone(),
        }
    }

    fn metadata_matches_request(&self, request: Option<&MetadataDraftSnapshot>) -> bool {
        request
            .map(|snapshot| self.metadata_draft_snapshot() == *snapshot)
            .unwrap_or(!self.metadata_dirty)
    }

    /// Copies persisted paste metadata into editable metadata fields.
    pub(super) fn sync_editor_metadata(&mut self, paste: &Paste) {
        self.edit_name = paste.name.clone();
        self.edit_language = paste.language.clone();
        self.edit_language_is_manual = paste.language_is_manual;
        self.edit_tags = paste.tags.join(", ");
        self.metadata_dirty = false;
    }

    /// Completes deferred command-palette copy actions once target content is available.
    pub(super) fn try_complete_pending_copy(&mut self) {
        let Some(action) = self.pending_copy_action.clone() else {
            return;
        };
        let Some(paste) = self.selected_paste.as_ref() else {
            return;
        };
        match action {
            PaletteCopyAction::Raw(id) if id == paste.id => {
                let content = if self.selected_id.as_deref() == Some(id.as_str()) {
                    self.active_snapshot()
                } else {
                    paste.content.clone()
                };
                self.clipboard_outgoing = Some(content);
                self.pending_copy_action = None;
                self.set_status("Copied paste content.");
            }
            PaletteCopyAction::Fenced(id) if id == paste.id => {
                let (content, language) = if self.selected_id.as_deref() == Some(id.as_str()) {
                    (
                        self.active_snapshot(),
                        self.edit_language.as_deref().or(paste.language.as_deref()),
                    )
                } else {
                    (paste.content.clone(), paste.language.as_deref())
                };
                self.clipboard_outgoing = Some(format_fenced_code_block(&content, language));
                self.pending_copy_action = None;
                self.set_status("Copied fenced code block.");
            }
            _ => {}
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
