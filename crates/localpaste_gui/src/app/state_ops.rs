//! State transitions for backend events, selection, and autosave flow.

use super::highlight::EditorLayoutCache;
use super::{
    LocalPasteApp, PaletteCopyAction, SaveStatus, SidebarCollection, StatusMessage, ToastMessage,
    LIST_PASTES_LIMIT, SEARCH_DEBOUNCE, SEARCH_PASTES_LIMIT, STATUS_TTL, TOAST_LIMIT, TOAST_TTL,
};
use crate::backend::{CoreCmd, CoreErrorSource, CoreEvent, PasteSummary};
use chrono::{Duration as ChronoDuration, Utc};
use localpaste_core::models::paste::Paste;
use std::collections::BTreeSet;
use std::time::Instant;
use tracing::warn;

impl LocalPasteApp {
    fn send_update_paste_or_mark_failed(
        &mut self,
        id: String,
        content: String,
        mode: &str,
    ) -> bool {
        if self
            .backend
            .cmd_tx
            .send(CoreCmd::UpdatePaste { id, content })
            .is_ok()
        {
            return true;
        }
        self.save_in_flight = false;
        self.save_status = SaveStatus::Dirty;
        self.last_edit_at = Some(Instant::now());
        self.set_status(format!("{mode} failed: backend unavailable."));
        false
    }

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
                    self.metadata_save_in_flight = false;
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
                self.metadata_save_in_flight = false;
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
                    let active_content = self.active_snapshot();
                    let has_newer_local_edits = active_content != paste.content;
                    if !self.metadata_dirty && !self.metadata_save_in_flight {
                        self.sync_editor_metadata(&paste);
                    }
                    let mut updated = paste;
                    updated.content = active_content;
                    self.selected_paste = Some(updated);
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
                    self.search_last_input_at = Some(Instant::now() - SEARCH_DEBOUNCE);
                }
                self.ensure_selection_after_list_update();
            }
            CoreEvent::SearchResults { query, items } => {
                // Drop stale search responses that no longer match the active query text.
                if self.search_query.trim().is_empty() || query.trim() != self.search_query.trim() {
                    return;
                }
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
            CoreEvent::FoldersLoaded { items: _ } => {}
            CoreEvent::FolderSaved { folder: _ } => {
                self.request_refresh();
            }
            CoreEvent::FolderDeleted { id: _ } => {
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
                        self.set_status(message);
                    }
                    _ => self.set_status(message),
                }
            }
        }
    }

    pub(super) fn request_refresh(&mut self) {
        let _ = self.backend.cmd_tx.send(CoreCmd::ListPastes {
            limit: LIST_PASTES_LIMIT,
            folder_id: None,
        });
        self.last_refresh_at = Instant::now();
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

    pub(super) fn set_active_language_filter(&mut self, language: Option<String>) {
        if self.active_language_filter == language {
            return;
        }
        self.active_language_filter = language;
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
            if let Some(lang) = paste
                .language
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                langs.insert(lang.to_string());
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
            limit: SEARCH_PASTES_LIMIT,
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
        let _sent = self.send_update_paste_or_mark_failed(id, content, "Autosave");
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
        let _sent = self.send_update_paste_or_mark_failed(id, content, "Save");
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
        let today = now.date_naive();
        let week_cutoff = now - ChronoDuration::days(7);
        let recent_cutoff = now - ChronoDuration::days(30);
        items
            .iter()
            .filter(|item| {
                let collection_match = match &self.active_collection {
                    SidebarCollection::All => true,
                    SidebarCollection::Today => item.updated_at.date_naive() == today,
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

fn language_in_set(language: Option<&str>, values: &[&str]) -> bool {
    let Some(language) = language.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };
    values
        .iter()
        .any(|value| language.eq_ignore_ascii_case(value))
}

fn contains_any_ci(value: &str, needles: &[&str]) -> bool {
    let value_lower = value.to_ascii_lowercase();
    needles.iter().any(|needle| value_lower.contains(needle))
}

fn tags_contain_any(tags: &[String], needles: &[&str]) -> bool {
    tags.iter().any(|tag| contains_any_ci(tag, needles))
}

fn is_code_summary(item: &PasteSummary) -> bool {
    language_in_set(
        item.language.as_deref(),
        &[
            "rust",
            "python",
            "javascript",
            "typescript",
            "go",
            "java",
            "kotlin",
            "swift",
            "ruby",
            "php",
            "c",
            "cpp",
            "c++",
            "csharp",
            "cs",
            "shell",
            "bash",
            "zsh",
            "sql",
            "html",
            "css",
            "markdown",
        ],
    ) || contains_any_ci(
        item.name.as_str(),
        &[
            ".rs", ".py", ".js", ".ts", ".go", ".java", ".cs", ".sql", ".sh", "snippet", "script",
            "class", "function",
        ],
    ) || tags_contain_any(item.tags.as_slice(), &["code", "snippet", "script"])
}

fn is_config_summary(item: &PasteSummary) -> bool {
    language_in_set(
        item.language.as_deref(),
        &[
            "json",
            "yaml",
            "yml",
            "toml",
            "ini",
            "env",
            "xml",
            "hcl",
            "properties",
        ],
    ) || contains_any_ci(
        item.name.as_str(),
        &[
            "config",
            "settings",
            ".env",
            "dockerfile",
            "compose",
            "k8s",
            "kubernetes",
            "helm",
        ],
    ) || tags_contain_any(
        item.tags.as_slice(),
        &[
            "config",
            "settings",
            "env",
            "docker",
            "k8s",
            "kubernetes",
            "helm",
        ],
    )
}

fn is_log_summary(item: &PasteSummary) -> bool {
    language_in_set(item.language.as_deref(), &["log"])
        || contains_any_ci(
            item.name.as_str(),
            &["log", "logs", "trace", "stderr", "stdout", "error"],
        )
        || tags_contain_any(
            item.tags.as_slice(),
            &["log", "logs", "trace", "stderr", "stdout", "error"],
        )
}

fn is_link_summary(item: &PasteSummary) -> bool {
    contains_any_ci(
        item.name.as_str(),
        &["http://", "https://", "www.", "url", "link", "links"],
    ) || tags_contain_any(item.tags.as_slice(), &["url", "link", "links", "bookmark"])
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tags_csv_trims_and_dedupes_case_insensitively() {
        let parsed = parse_tags_csv(" rust,CLI, rust , cli ,");
        assert_eq!(parsed, vec!["rust".to_string(), "CLI".to_string()]);
    }

    #[test]
    fn language_extension_maps_known_and_unknown_languages() {
        assert_eq!(language_extension(Some("rust")), "rs");
        assert_eq!(language_extension(Some(" Python ")), "py");
        assert_eq!(language_extension(Some("unknown")), "txt");
        assert_eq!(language_extension(None), "txt");
    }

    #[test]
    fn sanitize_filename_replaces_reserved_chars_and_falls_back() {
        assert_eq!(sanitize_filename("bad<>:\"/\\|?*name"), "bad_________name");
        assert_eq!(sanitize_filename("   "), "localpaste-export");
    }

    #[test]
    fn format_fenced_block_uses_language_or_text_default() {
        assert_eq!(
            format_fenced_block("let x = 1;", Some("rust")),
            "```rust\nlet x = 1;\n```"
        );
        assert_eq!(format_fenced_block("hello", None), "```text\nhello\n```");
    }
}
