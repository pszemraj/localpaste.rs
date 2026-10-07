//! Sidebar and paste-picker queries, scopes, and dispatch.

use super::*;

impl LocalPasteApp {
    /// Updates the sidebar search query and starts debounce timing.
    pub(in crate::app) fn set_search_query(&mut self, query: String) {
        if self.search_query == query {
            return;
        }
        self.search_query = query;
        self.picker_selection_pin = None;
        self.search_last_input_at = Some(Instant::now());
    }

    /// Updates command-palette query text and resets palette selection/search state.
    pub(in crate::app) fn set_paste_picker_query(&mut self, query: String) {
        if self.paste_picker_query == query {
            return;
        }
        self.paste_picker_query = query;
        self.reset_paste_picker_results();
        self.palette_search_last_input_at = Some(Instant::now());
    }

    /// Clears stale picker results and selection while callers choose dispatch timing.
    pub(in crate::app) fn reset_paste_picker_results(&mut self) {
        self.reset_paste_picker_selection();
        self.palette_search_last_sent.clear();
        self.palette_search_results.clear();
        self.palette_search_pending = false;
        self.palette_search_error = None;
    }

    /// Retain a picker failure until explicit Retry or new query/scope input.
    pub(super) fn fail_palette_search(&mut self, message: String) {
        self.palette_search_pending = false;
        self.palette_search_results.clear();
        self.palette_search_last_sent.clear();
        self.palette_search_last_input_at = None;
        self.palette_search_error = Some(message.clone());
        if self.status.as_ref().map(|status| status.text.as_str()) != Some(message.as_str()) {
            self.set_status(message);
        }
    }

    /// Checks whether a paste-picker response belongs to the active request.
    ///
    /// # Arguments
    /// - `scope`: Scope recorded on the backend response.
    /// - `query`: Query recorded on the backend response.
    ///
    /// # Returns
    /// `true` when the picker remains open and its active query and scope match
    /// the response.
    pub(super) fn palette_search_response_is_current(
        &self,
        scope: super::super::SearchScope,
        query: &str,
    ) -> bool {
        self.paste_picker_open
            && scope == self.paste_picker_scope
            && query.trim() == self.paste_picker_query.trim()
    }

    /// Invalidate old query context and dispatch or restore the current projection.
    pub(super) fn on_primary_filter_changed(&mut self) {
        self.picker_selection_pin = None;
        self.refresh_search_projection();
    }

    /// Refreshes search rows after filters change, respecting a retained document pin.
    fn refresh_search_projection(&mut self) {
        self.search_last_sent.clear();
        if self.search_query.trim().is_empty() {
            self.recompute_visible_pastes();
            self.ensure_selection_after_list_update();
        } else {
            self.search_last_input_at = Some(Instant::now() - SEARCH_DEBOUNCE);
        }
    }

    /// Switches the active smart collection filter and triggers list/search refresh behavior.
    pub(in crate::app) fn set_active_collection(&mut self, collection: SidebarCollection) {
        if self.active_collection == collection {
            return;
        }
        self.active_collection = collection;
        self.on_primary_filter_changed();
    }

    /// Sets the active language filter after canonical normalization.
    pub(in crate::app) fn set_active_language_filter(&mut self, language: Option<String>) {
        let normalized = normalize_language_filter_value(language.as_deref());
        if self.active_language_filter == normalized {
            return;
        }
        self.active_language_filter = normalized;
        self.on_primary_filter_changed();
    }

    /// Builds sorted language filter options from the currently known paste summaries.
    /// # Returns
    /// Canonicalized language values in ascending sort order.
    pub(in crate::app) fn language_filter_options(&self) -> Vec<String> {
        let mut langs: BTreeSet<String> = BTreeSet::new();
        for paste in &self.all_pastes {
            if let Some(lang) = normalize_language_filter_value(paste.language.as_deref()) {
                langs.insert(lang);
            }
        }
        langs.into_iter().collect()
    }

    /// Dispatches a debounced sidebar search request when inputs and filters are ready.
    pub(in crate::app) fn maybe_dispatch_search(&mut self) {
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

        if self.search_last_sent == query && self.search_sent_scope == self.search_scope {
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
        if !self.dispatch_backend_cmd(CoreCmd::SearchPastes {
            scope: self.search_scope,
            query: query.clone(),
            limit: DEFAULT_SEARCH_PASTES_LIMIT,
            collection: self.active_collection.clone(),
            folder_id,
            language,
        }) {
            // Avoid per-frame retry storms/toast spam while backend is unavailable.
            // Re-arm debounce so we retry on a bounded cadence.
            self.search_last_input_at = Some(Instant::now());
            const SEARCH_UNAVAILABLE: &str = "Search failed: backend unavailable.";
            if self.status.as_ref().map(|status| status.text.as_str()) != Some(SEARCH_UNAVAILABLE) {
                self.set_status(SEARCH_UNAVAILABLE);
            }
            return;
        }
        self.search_last_sent = query;
        self.search_sent_scope = self.search_scope;
        self.query_perf.search_requests_sent =
            self.query_perf.search_requests_sent.saturating_add(1);
        self.query_perf.search_last_sent_at = Some(Instant::now());
    }

    /// Dispatches a debounced command-palette search request when applicable.
    pub(in crate::app) fn maybe_dispatch_palette_search(&mut self) {
        if !self.paste_picker_open {
            return;
        }

        let query = self.paste_picker_query.trim().to_string();
        if query.is_empty() {
            if !self.palette_search_last_sent.is_empty() || !self.palette_search_results.is_empty()
            {
                self.palette_search_last_sent.clear();
                self.palette_search_results.clear();
            }
            self.palette_search_pending = false;
            self.palette_search_error = None;
            return;
        }

        if self.palette_search_last_sent == query
            && self.paste_picker_sent_scope == self.paste_picker_scope
        {
            return;
        }
        let Some(last_input_at) = self.palette_search_last_input_at else {
            return;
        };
        if last_input_at.elapsed() < SEARCH_DEBOUNCE {
            return;
        }

        if !self.dispatch_backend_cmd(CoreCmd::SearchPalette {
            scope: self.paste_picker_scope,
            query: query.clone(),
            limit: PALETTE_SEARCH_LIMIT,
        }) {
            // Leave the error visible until the user requests another attempt.
            const PALETTE_SEARCH_UNAVAILABLE: &str =
                "Paste picker search failed: backend unavailable.";
            self.fail_palette_search(PALETTE_SEARCH_UNAVAILABLE.into());
            return;
        }
        self.palette_search_last_sent = query;
        self.paste_picker_sent_scope = self.paste_picker_scope;
        self.palette_search_pending = true;
    }

    /// Updates sidebar scope and discards results from the previous field context.
    pub(in crate::app) fn set_search_scope(&mut self, scope: super::super::SearchScope) {
        if self.search_scope != scope {
            let selection = self.selected_id.clone();
            self.search_scope = scope;
            self.pastes.clear();
            // Install the pin before an empty query can recompute and select rows.
            self.picker_selection_pin = selection;
            self.refresh_search_projection();
        }
    }

    /// Updates picker scope without changing the sidebar query or scope.
    pub(in crate::app) fn set_paste_picker_scope(&mut self, scope: super::super::SearchScope) {
        if self.paste_picker_scope != scope {
            self.paste_picker_scope = scope;
            self.reset_paste_picker_results();
            self.palette_search_last_input_at = Some(Instant::now() - SEARCH_DEBOUNCE);
        }
    }
}
