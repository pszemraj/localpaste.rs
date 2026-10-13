//! State transitions for backend events, selection, and autosave flow.

/// Filter helpers for sidebar collections, languages, and export filenames.
pub(super) mod filters;
mod list_projection;
mod search;
mod selection;
mod selection_load;

use super::util::parse_tags_csv;
use super::{
    ExportCompletion, LocalPasteApp, MetadataDraftSnapshot, SaveStatus, SidebarCollection,
    BACKEND_EVENT_POLL_INTERVAL, BACKEND_EVENT_POLL_WINDOW, PALETTE_SEARCH_LIMIT, SEARCH_DEBOUNCE,
};
use crate::backend::{CoreCmd, CoreErrorSource, CoreEvent, PasteSummary};
use eframe::egui;
use localpaste_core::{models::paste::Paste, DEFAULT_SEARCH_PASTES_LIMIT};
use std::collections::BTreeSet;
use std::time::{Duration, Instant};
use tracing::warn;

use self::filters::{
    language_extension, matches_active_filters, normalize_language_filter_value, sanitize_filename,
};

/// Observes native widget copies after egui's selectable-label end-pass writer.
#[derive(Default)]
struct NativeClipboardObserver {
    /// Existing request and output prefix from before this pass's user input.
    pass: Option<(u64, usize)>,
    /// Request whose delayed reply must be ignored before the next event drain.
    superseded_request: Option<u64>,
}

impl egui::Plugin for NativeClipboardObserver {
    fn debug_name(&self) -> &'static str {
        "localpaste_native_clipboard"
    }

    fn on_end_pass(&mut self, ui: &mut egui::Ui) {
        let ctx = &ui.ctx().clone();
        let Some((request_id, command_start)) = self.pass.take() else {
            return;
        };
        if ctx.output(|output| {
            output
                .commands
                .iter()
                .skip(command_start)
                .any(|command| matches!(command, egui::OutputCommand::CopyText(_)))
        }) {
            self.superseded_request = Some(request_id);
        }
    }
}

impl LocalPasteApp {
    /// Consumes background replies and advances timed work without rebuilding the UI.
    ///
    /// # Arguments
    /// - `ctx`: Context used only for clipboard observation and repaint scheduling.
    pub(super) fn poll_background_work(&mut self, ctx: &egui::Context) {
        self.reconcile_native_clipboard(ctx);
        let now = Instant::now();
        if self
            .status
            .as_ref()
            .is_some_and(|status| now >= status.expires_at)
        {
            self.status = None;
        }
        self.prune_expired_toasts(now);
        loop {
            match self.backend.evt_rx.try_recv() {
                Ok(event) => self.apply_event(event),
                Err(crossbeam_channel::TryRecvError::Empty) => break,
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    self.handle_backend_event_channel_disconnected();
                    break;
                }
            }
        }
        self.poll_export_result();
        while let Ok(reply) = self.highlight_worker.rx.try_recv() {
            self.queue_highlight_reply(reply);
        }
        if ctx.input(|input| input.viewport().visible()) == Some(false) {
            self.maybe_dispatch_palette_search();
            self.maybe_dispatch_search();
            self.maybe_autosave();
        }
        if let Some(delay) = self.backend_event_poll_repaint_after(Instant::now()) {
            ctx.request_repaint_after(delay);
        }
    }

    /// Reconciles native clipboard writes before draining delayed copy replies.
    ///
    /// # Arguments
    /// - `ctx`: Context owning the observer, registered after built-in label selection.
    pub(super) fn reconcile_native_clipboard(&mut self, ctx: &egui::Context) {
        let superseded = ctx
            .plugin_or_default::<NativeClipboardObserver>()
            .lock()
            .superseded_request
            .take();
        if superseded == Some(self.palette_copy_request_id) {
            self.pending_copy_action = None;
        }
    }

    /// Arms end-pass observation for native widget copies in this input pass.
    ///
    /// # Arguments
    /// - `ctx`: Context whose native widgets can copy text after app update returns.
    pub(super) fn observe_native_clipboard(&self, ctx: &egui::Context) {
        let command_start = ctx.output(|output| output.commands.len());
        let pass = self
            .pending_copy_action
            .as_ref()
            .map(|_| (self.palette_copy_request_id, command_start));
        let _ = ctx.with_plugin::<NativeClipboardObserver, _>(|observer| observer.pass = pass);
    }

    /// Queues clipboard text and supersedes any older detached copy request.
    ///
    /// # Arguments
    /// - `text`: Text produced by the newest completed copy action.
    pub(super) fn queue_clipboard_text(&mut self, text: String) {
        self.pending_copy_action = None;
        self.clipboard_outgoing = Some(text);
    }

    /// Emits queued clipboard text once, preserving immediate editor copy output.
    ///
    /// # Arguments
    /// - `ctx`: Egui context receiving the clipboard output command.
    pub(super) fn flush_clipboard_output(&mut self, ctx: &egui::Context) {
        if let Some(text) = self.clipboard_outgoing.take() {
            ctx.send_cmd(egui::OutputCommand::CopyText(text));
        }
    }

    /// Sends a backend command and arms short-term event polling for its reply.
    ///
    /// Commands sent in quiet frames arm prompt repainting so worker responses
    /// are drained before the slow external refresh interval.
    ///
    /// # Returns
    /// `true` when the command was queued and short polling was armed.
    pub(super) fn dispatch_backend_cmd(&mut self, command: CoreCmd) -> bool {
        if self.backend.cmd_tx.send(command).is_err() {
            return false;
        }
        self.backend_event_poll_until = Some(Instant::now() + BACKEND_EVENT_POLL_WINDOW);
        true
    }

    /// Returns the next short polling repaint delay while a backend reply is expected.
    ///
    /// # Returns
    /// `Some` with the remaining bounded polling delay, or `None` when no short polling window is active.
    pub(super) fn backend_event_poll_repaint_after(&mut self, now: Instant) -> Option<Duration> {
        let until = self.backend_event_poll_until?;
        if until <= now {
            self.backend_event_poll_until = None;
            return None;
        }
        Some(BACKEND_EVENT_POLL_INTERVAL.min(until.saturating_duration_since(now)))
    }

    /// Sends a backend command and reports a status message if dispatch fails.
    ///
    /// # Arguments
    /// - `command`: Backend command to queue.
    /// - `error_message`: Status text used when the backend channel is closed.
    ///
    /// # Returns
    /// `true` when the command was queued, otherwise `false`.
    pub(super) fn send_backend_cmd_or_status(
        &mut self,
        command: CoreCmd,
        error_message: &str,
    ) -> bool {
        if self.dispatch_backend_cmd(command) {
            return true;
        }
        self.notify(error_message);
        false
    }

    /// Clears UI state that can only complete via backend events after the event channel closes.
    pub(super) fn handle_backend_event_channel_disconnected(&mut self) {
        self.pending_picker_open = None;
        self.pending_picker_selection_pin = None;
        let picker_delete_pending = self.picker_delete_transition_active();
        self.clear_picker_delete_transition();
        self.cancel_pending_delete();
        let picker_search_pending = std::mem::take(&mut self.palette_search_pending);
        if picker_search_pending {
            self.fail_palette_search("Paste picker search canceled: backend unavailable.".into());
        }
        let picker_copy_pending = self.pending_copy_action.take().is_some();
        if !self.pending_undo_restore_tokens.is_empty() {
            self.pending_undo_restore_tokens.clear();
            self.notify("Undo delete canceled: backend unavailable.");
        } else if picker_delete_pending {
            self.notify("Delete canceled: backend unavailable.");
        } else if picker_copy_pending {
            self.notify("Paste copy canceled: backend unavailable.");
        } else if picker_search_pending {
            self.notify("Paste picker search canceled: backend unavailable.");
        }
    }

    fn send_update_paste_or_mark_failed(&mut self, command: CoreCmd, mode: &str) -> bool {
        if self.dispatch_backend_cmd(command) {
            return true;
        }
        self.save_in_flight = false;
        self.save_status = SaveStatus::Dirty;
        self.save_request_revision = None;
        self.last_edit_at = Some(Instant::now());
        self.notify(format!("{mode} failed: backend unavailable."));
        false
    }

    fn dispatch_content_save(&mut self, id: String, mode: &str) -> bool {
        self.save_request_revision = Some(self.active_revision());
        self.save_in_flight = true;
        self.save_status = SaveStatus::Saving;
        let protected_version_id_ms = self.protected_history_reset_version_for(id.as_str());

        let command = CoreCmd::UpdatePasteVirtual {
            id,
            content: self.virtual_editor_buffer.rope().clone(),
            protected_version_id_ms,
        };

        self.send_update_paste_or_mark_failed(command, mode)
    }

    /// Applies a backend event and synchronizes app state, selection, and save flags.
    pub(super) fn apply_event(&mut self, event: CoreEvent) {
        if !self.selection_load_is_current(&event) {
            return;
        }
        self.on_version_event(&event);
        match event {
            CoreEvent::PasteList { items } => {
                self.query_perf.list_results_applied =
                    self.query_perf.list_results_applied.saturating_add(1);
                if let Some(sent_at) = self.query_perf.list_last_sent_at.take() {
                    self.query_perf.list_last_roundtrip_ms =
                        Some(sent_at.elapsed().as_secs_f32() * 1000.0);
                }
                self.all_pastes = items;
                if self.search_query.trim().is_empty() {
                    self.recompute_visible_pastes();
                    self.ensure_selection_after_list_update();
                } else {
                    // External API/CLI writes arrive via list refresh, not local save events.
                    // Results also match bodies, which the list does not carry, so force one
                    // fresh backend search to keep active query results in sync.
                    self.search_last_sent.clear();
                    self.search_last_input_at = Some(Instant::now() - SEARCH_DEBOUNCE);
                }
            }
            CoreEvent::PasteLoaded { paste, .. } => {
                let paste_id = paste.id.clone();
                self.restored_selection_id = None;
                self.select_loaded_paste(paste);
                self.clear_picker_delete_transition_for_replacement(paste_id.as_str());
            }
            CoreEvent::PasteCopyLoaded { paste, request_id } => {
                self.apply_palette_copy_loaded(paste, request_id)
            }
            CoreEvent::PasteCopyMissing { id, request_id } => {
                self.apply_palette_copy_missing(id, request_id)
            }
            CoreEvent::PasteCopyLoadFailed {
                id,
                request_id,
                message,
            } => {
                self.apply_palette_copy_load_failed(id, request_id, message);
            }
            CoreEvent::DiffPreviewComputed { request_id, diff } => {
                self.apply_diff_preview_response(request_id, diff);
            }
            CoreEvent::PasteCreated { paste } => {
                let paste_id = paste.id.clone();
                let search_active = !self.search_query.trim().is_empty();
                // `all_pastes` is authoritative; `pastes` must stay a derived projection.
                self.upsert_cached_paste_summary(&paste);
                if search_active {
                    self.search_last_sent.clear();
                    self.search_last_input_at = Some(Instant::now() - SEARCH_DEBOUNCE);
                } else {
                    self.recompute_visible_pastes();
                }
                // A created paste always opens. One the sidebar does not list is pinned,
                // so refreshes and search replies keep it until the query or filters change.
                let paste_listed =
                    !search_active && self.pastes.iter().any(|item| item.id == paste_id);
                let has_unsaved_edits =
                    self.save_status == SaveStatus::Dirty || self.metadata_dirty;
                let save_in_progress = self.save_in_flight
                    || self.metadata_save_in_flight
                    || self.save_status == SaveStatus::Saving;
                if self.selection_transition_block_reason().is_some() {
                    self.queue_pending_selection(paste_id.clone());
                    if !paste_listed {
                        self.pin_queued_selection(paste_id);
                    }
                    self.notify("Created new paste; it opens once you close History or Diff.");
                    return;
                }
                if has_unsaved_edits || save_in_progress {
                    // Keep current editor/save state untouched when switching is deferred.
                    if self.select_paste(paste_id.clone()) && !paste_listed {
                        self.pin_queued_selection(paste_id);
                    }
                    return;
                }
                self.select_loaded_paste(paste);
                self.pending_selection_id = None;
                self.picker_selection_pin = (!paste_listed).then_some(paste_id);
                self.focus_editor_next = true;
                self.set_status(if paste_listed {
                    "Created new paste."
                } else if search_active {
                    "Created new paste; refreshing search results."
                } else {
                    "Created new paste; current filters hide it in the sidebar."
                });
            }
            CoreEvent::PasteSaved { paste } => {
                let paste_id = paste.id.clone();
                let requested_revision = self.save_request_revision.take();
                self.upsert_cached_paste_summary(&paste);
                if !self.search_query.trim().is_empty() {
                    // Content saves can update full-content search matches and summary
                    // metadata, so force redispatch.
                    self.search_last_sent.clear();
                    self.search_last_input_at = Some(Instant::now() - SEARCH_DEBOUNCE);
                }
                if self.selected_id.as_deref() == Some(paste_id.as_str()) {
                    // `save_request_revision` can be cleared after a partial deferred-switch
                    // failure even when a content-save command was already dispatched.
                    // Use snapshot comparison as a safe fallback for late save acks.
                    let has_newer_local_edits = requested_revision
                        .map(|revision| self.active_revision() != revision)
                        .unwrap_or_else(|| self.active_snapshot() != paste.content);
                    if !self.metadata_dirty && !self.metadata_save_in_flight {
                        self.sync_editor_metadata(&paste);
                        self.selected_paste = Some(paste);
                    } else {
                        self.adopt_saved_language(paste);
                    }
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
                    self.recompute_visible_pastes();
                }
                self.maybe_continue_queued_history_reset();
                self.maybe_continue_pending_delete();
                self.try_apply_pending_selection();
                if self.search_query.trim().is_empty() {
                    self.ensure_selection_after_list_update();
                }
            }
            CoreEvent::PasteMetaSaved { paste } => {
                let requested_metadata = self.metadata_save_request.take();
                self.metadata_save_in_flight = false;
                self.upsert_cached_paste_summary(&paste);
                if self.selected_id.as_deref() == Some(paste.id.as_str()) {
                    let draft_saved = self.metadata_matches_request(requested_metadata.as_ref());
                    self.selected_paste = Some(paste.clone());
                    if draft_saved {
                        self.sync_editor_metadata(&paste);
                    } else {
                        // Edits made after dispatch, or a language adopted from a content
                        // ack, stay dirty only where the draft differs from the saved row.
                        self.refresh_metadata_dirty();
                    }
                }
                if self.search_query.trim().is_empty() {
                    self.recompute_visible_pastes();
                } else {
                    self.retain_search_results_for_active_filters();
                    self.search_last_sent.clear();
                    self.search_last_input_at = Some(Instant::now() - SEARCH_DEBOUNCE);
                }
                self.ensure_selection_after_list_update();
                self.maybe_continue_queued_history_reset();
                self.maybe_continue_pending_delete();
                self.try_apply_pending_selection();
            }
            CoreEvent::SearchResults {
                collection,
                scope,
                query,
                folder_id,
                language,
                items,
            } => {
                // Drop stale search responses when query or backend filter context changed.
                if !self.sidebar_search_response_is_current(
                    &collection,
                    scope,
                    &query,
                    folder_id.as_deref(),
                    language.as_deref(),
                ) {
                    self.query_perf.search_stale_drops =
                        self.query_perf.search_stale_drops.saturating_add(1);
                    self.forget_sidebar_search_answered_after_edit(&query);
                    return;
                }
                self.search_error = None;
                self.query_perf.search_results_applied =
                    self.query_perf.search_results_applied.saturating_add(1);
                if let Some(sent_at) = self.query_perf.search_last_sent_at.take() {
                    self.query_perf.search_last_roundtrip_ms =
                        Some(sent_at.elapsed().as_secs_f32() * 1000.0);
                }
                self.pastes = self.filter_by_collection(&items).into();
                self.note_sidebar_rows_filter();
                self.ensure_selection_after_list_update();
            }
            CoreEvent::SearchFailed {
                collection,
                scope,
                query,
                folder_id,
                language,
                message,
            } => self.fail_sidebar_search(collection, scope, query, folder_id, language, message),
            CoreEvent::PaletteSearchResults {
                query,
                items,
                scope,
            } => {
                if self.paste_picker_query.trim().is_empty()
                    || !self.palette_search_response_is_current(scope, query.as_str())
                    || scope != self.paste_picker_sent_scope
                {
                    return;
                }
                self.palette_search_pending = false;
                self.palette_search_error = None;
                self.palette_search_last_sent = query;
                self.paste_picker_sent_scope = scope;
                self.palette_search_results = items;
                self.clamp_paste_picker_selection(self.palette_search_results.len());
            }
            CoreEvent::PaletteSearchFailed {
                scope,
                query,
                message,
            } => {
                if !self.palette_search_response_is_current(scope, query.as_str())
                    || self.palette_search_last_sent != query
                    || scope != self.paste_picker_sent_scope
                {
                    return;
                }
                self.fail_palette_search(message);
            }
            CoreEvent::PasteDeleted { id, undo_token } => {
                self.apply_paste_deleted(id, undo_token);
            }
            CoreEvent::PasteDeleteFailed { id, message } => {
                self.clear_picker_delete_transition_for_deleted(id.as_str());
                if self.pending_delete_id.as_deref() == Some(id.as_str()) {
                    self.cancel_pending_delete();
                }
                self.notify(message);
            }
            CoreEvent::PasteRestored { paste, undo_token } => {
                let paste_id = paste.id.clone();
                self.pending_undo_restore_tokens.remove(&undo_token);
                self.remove_undo_toast(&undo_token);
                self.upsert_cached_paste_summary(&paste);
                if !self.search_query.trim().is_empty() {
                    self.search_last_sent.clear();
                    self.search_last_input_at = Some(Instant::now() - SEARCH_DEBOUNCE);
                } else {
                    self.recompute_visible_pastes();
                }
                let can_select_restored = self.selection_transition_block_reason().is_none()
                    && !self.save_in_flight
                    && !self.metadata_save_in_flight
                    && self.save_status == SaveStatus::Saved
                    && !self.metadata_dirty;
                if can_select_restored {
                    self.select_loaded_paste(paste);
                    self.pending_selection_id = None;
                } else {
                    self.queue_pending_selection(paste_id);
                }
                self.set_status("Restored deleted paste.");
                self.request_refresh();
            }
            CoreEvent::PasteRestoreFailed {
                undo_token,
                message,
                retryable,
            } => {
                self.pending_undo_restore_tokens.remove(&undo_token);
                if !retryable {
                    self.remove_undo_toast(&undo_token);
                }
                self.notify(message);
            }
            CoreEvent::PasteUndoEvicted { undo_token } => {
                self.pending_undo_restore_tokens.remove(&undo_token);
                self.remove_undo_toast(&undo_token);
            }
            CoreEvent::PasteMissing { id } | CoreEvent::PasteSelectionMissing { id, .. } => {
                self.clear_picker_delete_transition_for_replacement(id.as_str());
                if self
                    .picker_delete_transition
                    .as_ref()
                    .is_some_and(|transition| transition.replacement_id.is_none())
                {
                    self.clear_picker_delete_transition_for_deleted(id.as_str());
                }
                self.all_pastes.retain(|paste| paste.id != id);
                self.pastes.retain(|paste| paste.id != id);
                self.clear_picker_selection_context_for(id.as_str());
                let restored = self.take_restored_selection(id.as_str());
                if self.selected_id.as_deref() == Some(id.as_str()) {
                    self.clear_selection();
                    if !restored {
                        self.notify("Selected paste was deleted; list refreshed.");
                    }
                } else {
                    self.notify("Paste was deleted; list refreshed.");
                }
                self.request_refresh();
            }
            CoreEvent::DiffTargetMissing { id } => {
                let diff_target_was_active =
                    self.version_ui.diff_target_id.as_deref() == Some(id.as_str());
                self.all_pastes.retain(|paste| paste.id != id);
                self.pastes.retain(|paste| paste.id != id);
                self.clear_picker_selection_context_for(id.as_str());
                if self.selected_id.as_deref() == Some(id.as_str()) {
                    self.clear_selection();
                    self.notify("Selected paste was deleted; list refreshed.");
                } else if diff_target_was_active {
                    self.version_ui.clear_diff_target_state();
                    self.notify("Comparison paste was deleted; list refreshed.");
                }
                self.request_refresh();
            }
            CoreEvent::PasteLoadFailed { id, message, .. } => {
                self.clear_picker_delete_transition_for_replacement(id.as_str());
                self.clear_picker_selection_context_for(id.as_str());
                self.clear_selection();
                self.notify(message);
            }
            CoreEvent::PasteVersionsLoaded { .. }
            | CoreEvent::PasteVersionLoaded { .. }
            | CoreEvent::PasteVersionLoadFailed { .. }
            | CoreEvent::PasteResetToVersion { .. }
            | CoreEvent::DiffTargetLoaded { .. }
            | CoreEvent::DiffTargetLoadFailed { .. }
            | CoreEvent::FoldersLoaded { items: _ }
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
                        if let Some(id) = self.pending_delete_id.clone() {
                            self.clear_picker_delete_transition_for_deleted(id.as_str());
                        }
                        self.cancel_queued_history_reset();
                        self.cancel_pending_delete();
                        self.metadata_dirty = true;
                        self.metadata_save_in_flight = false;
                        self.metadata_save_request = None;
                        self.clear_pending_selection_request();
                        if message.to_ascii_lowercase().contains("metadata") {
                            self.notify(message);
                        } else {
                            self.notify(format!("Metadata save failed: {}", message));
                        }
                    }
                    CoreErrorSource::SaveContent if self.save_in_flight => {
                        if let Some(id) = self.pending_delete_id.clone() {
                            self.clear_picker_delete_transition_for_deleted(id.as_str());
                        }
                        self.cancel_queued_history_reset();
                        self.cancel_pending_delete();
                        if self.save_status == SaveStatus::Saving {
                            self.save_status = SaveStatus::Dirty;
                        }
                        self.save_in_flight = false;
                        self.save_request_revision = None;
                        self.clear_pending_selection_request();
                        self.notify(message);
                    }
                    _ => self.notify(message),
                }
            }
        }
    }

    /// Requests a fresh paste list from the backend and updates query perf counters.
    pub(super) fn request_refresh(&mut self) {
        let sent_at = Instant::now();
        if !self.dispatch_backend_cmd(CoreCmd::ListPastes { folder_id: None }) {
            self.notify("List failed: backend unavailable.");
            return;
        }
        self.query_perf.list_requests_sent = self.query_perf.list_requests_sent.saturating_add(1);
        self.query_perf.list_last_sent_at = Some(sent_at);
        self.last_refresh_at = sent_at;
    }

    /// Creates a new empty paste.
    pub(super) fn create_new_paste(&mut self) {
        self.create_new_paste_with_content(String::new());
    }

    /// Creates a new paste pre-populated with `content`.
    pub(super) fn create_new_paste_with_content(&mut self, content: String) {
        if self.mutation_shortcut_block_reason().is_some() {
            self.set_mutation_shortcut_blocked_status();
            return;
        }
        let _sent = self.send_backend_cmd_or_status(
            CoreCmd::CreatePaste { content },
            "Create failed: backend unavailable.",
        );
    }

    /// Marks current editor content dirty and arms autosave timing.
    pub(super) fn mark_dirty(&mut self) {
        // Reset is authoritative once queued; the selected paste must stop accepting
        // local dirty-state transitions until the backend replies.
        if self.reset_transition_active() || self.history_reset_flush_active() {
            return;
        }
        if self.selected_id.is_some() {
            self.save_status = SaveStatus::Dirty;
            self.last_edit_at = Some(Instant::now());
        }
    }

    /// Dispatches autosave once dirty content has been idle past the autosave delay.
    pub(super) fn maybe_autosave(&mut self) {
        if self.nav_probe_seed_active() {
            return;
        }
        if self.save_block_reason().is_some() {
            return;
        }
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
        if self.nav_probe_seed_active() {
            return;
        }
        if self.save_block_reason().is_some() {
            self.set_save_blocked_status();
            return;
        }
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
        if self.save_block_reason().is_some() {
            self.set_save_blocked_status();
            return;
        }
        if !self.metadata_dirty || self.metadata_save_in_flight {
            return;
        }
        let Some(id) = self.selected_id.clone() else {
            return;
        };
        let request = self.metadata_draft_snapshot();
        // Only a staged language change is sent. The backend reads
        // `language_is_manual: false` on a locked row as a switch back to auto and
        // clears the language, which would discard one detected since this paste loaded.
        let language_changed = self
            .selected_paste
            .as_ref()
            .is_none_or(|loaded| !self.draft_language_matches(loaded));
        let (language, language_is_manual) = if language_changed {
            let language = self
                .edit_language
                .clone()
                .filter(|_| self.edit_language_is_manual);
            (language, Some(self.edit_language_is_manual))
        } else {
            (None, None)
        };
        let tags = Some(parse_tags_csv(self.edit_tags.as_str()));
        self.metadata_save_request = None;
        if !self.dispatch_backend_cmd(CoreCmd::UpdatePasteMeta {
            id,
            name: Some(self.edit_name.clone()),
            language,
            language_is_manual,
            folder_id: None,
            tags,
        }) {
            self.notify("Metadata save failed: backend unavailable.");
            return;
        }
        self.metadata_save_in_flight = true;
        self.metadata_save_request = Some(request);
    }

    /// Starts asynchronous export of the selected paste to a user-chosen file path.
    pub(super) fn export_selected_paste(&mut self) {
        let Some(paste_id) = self.selected_paste.as_ref().map(|paste| paste.id.clone()) else {
            self.notify("Nothing selected to export.");
            return;
        };
        if self.export_result_rx.is_some() {
            self.notify("Export already in progress.");
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

    /// Compares the draft's language fields with `paste`'s.
    ///
    /// # Returns
    /// `true` when they are equal, so the draft stages no language change.
    pub(in crate::app) fn draft_language_matches(&self, paste: &Paste) -> bool {
        self.edit_language == paste.language
            && self.edit_language_is_manual == paste.language_is_manual
    }

    /// Takes a content-save ack as the loaded paste while metadata edits are pending.
    ///
    /// The ack can carry a language the backend resolved from the saved content.
    /// The draft adopts it unless the draft stages a language change of its own;
    /// name and tag drafts stay as typed, and `metadata_dirty` is recomputed.
    fn adopt_saved_language(&mut self, paste: Paste) {
        let language_staged = self
            .selected_paste
            .as_ref()
            .is_some_and(|loaded| !self.draft_language_matches(loaded));
        if !language_staged {
            self.edit_language = paste.language.clone();
            self.edit_language_is_manual = paste.language_is_manual;
        }
        self.selected_paste = Some(paste);
        self.refresh_metadata_dirty();
    }
}
