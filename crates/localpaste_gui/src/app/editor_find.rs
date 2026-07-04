//! Current-paste find state and navigation for the virtual editor.

use super::*;

impl LocalPasteApp {
    /// Opens the current-paste find bar and focuses its query input.
    pub(super) fn open_editor_find(&mut self) {
        self.editor_find.open = true;
        self.editor_find.focus_requested = true;
        self.ensure_editor_find_matches_current();
        if let Some(active) = self.editor_find.active_match {
            self.select_editor_find_match(active);
        }
    }

    /// Closes the current-paste find bar without clearing the saved query.
    pub(super) fn close_editor_find(&mut self) {
        self.editor_find.open = false;
        self.editor_find.focus_requested = false;
    }

    /// Marks cached current-paste match ranges stale after a buffer replacement.
    pub(super) fn invalidate_editor_find_matches(&mut self) {
        self.editor_find.matches.clear();
        self.editor_find.active_match = None;
        self.editor_find.last_buffer_epoch = None;
        self.editor_find.last_buffer_revision = None;
    }

    /// Reuses the sidebar search query as an in-paste finder when it matches the loaded body.
    pub(super) fn prime_editor_find_from_sidebar_query(&mut self) {
        if self.editor_find.open && !self.editor_find.query.is_empty() {
            return;
        }
        let query = self.search_query.trim().to_owned();
        if query.is_empty() {
            return;
        }

        self.editor_find.query = query;
        self.rebuild_editor_find_matches(false);
        if self.editor_find.matches.is_empty() {
            self.editor_find.query.clear();
            self.editor_find.open = false;
            return;
        }
        self.editor_find.open = true;
        self.editor_find.focus_requested = false;
        self.select_editor_find_match(0);
    }

    /// Replaces the current-paste find query and selects the first matching range.
    pub(super) fn set_editor_find_query(&mut self, query: String) {
        self.update_editor_find(EditorFindUpdate::Query(query));
    }

    /// Toggles case sensitivity for current-paste find and rebuilds matches.
    pub(super) fn set_editor_find_case_sensitive(&mut self, case_sensitive: bool) {
        self.update_editor_find(EditorFindUpdate::CaseSensitive(case_sensitive));
    }

    /// Advances to the next current-paste find match, wrapping at the end.
    ///
    /// # Panics
    /// Does not intentionally panic.
    pub(super) fn editor_find_next(&mut self) {
        self.ensure_editor_find_matches_current();
        let count = self.editor_find.matches.len();
        if count == 0 {
            return;
        }
        let next = self
            .editor_find
            .active_match
            .map(|active| (active + 1) % count)
            .unwrap_or(0);
        self.select_editor_find_match(next);
    }

    /// Moves to the previous current-paste find match, wrapping at the start.
    pub(super) fn editor_find_previous(&mut self) {
        self.ensure_editor_find_matches_current();
        let count = self.editor_find.matches.len();
        if count == 0 {
            return;
        }
        let previous = self
            .editor_find
            .active_match
            .map(|active| if active == 0 { count - 1 } else { active - 1 })
            .unwrap_or(0);
        self.select_editor_find_match(previous);
    }

    /// Renders the compact current-paste find row below the editor toolbar.
    pub(super) fn render_editor_find_bar(&mut self, ui: &mut egui::Ui) {
        if !self.editor_find.open {
            return;
        }
        self.ensure_editor_find_matches_current();

        let mut query = self.editor_find.query.clone();
        let mut case_sensitive = self.editor_find.case_sensitive;
        let mut query_changed = false;
        let mut case_changed = false;
        let mut previous_requested = false;
        let mut next_requested = false;
        let mut close_requested = false;
        let mut clear_focus_request = false;

        ui.scope(|ui| {
            apply_editor_find_row_style(ui);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Find").small().color(COLOR_TEXT_MUTED));
                let response = ui.add(
                    egui::TextEdit::singleline(&mut query)
                        .id(egui::Id::new(EDITOR_FIND_INPUT_ID))
                        .desired_width((ui.available_width() * 0.38).clamp(180.0, 420.0))
                        .hint_text("Search current paste"),
                );
                if self.editor_find.focus_requested {
                    response.request_focus();
                    clear_focus_request = true;
                }
                query_changed |= response.changed();
                if response.has_focus() {
                    ui.input(|input| {
                        if input.key_pressed(egui::Key::Enter) {
                            if input.modifiers.shift {
                                previous_requested = true;
                            } else {
                                next_requested = true;
                            }
                        }
                        if input.key_pressed(egui::Key::Escape) {
                            close_requested = true;
                        }
                    });
                }

                ui.label(
                    RichText::new(self.editor_find_status_text())
                        .small()
                        .color(COLOR_TEXT_MUTED),
                );

                if ui.add(editor_find_button("Prev")).clicked() {
                    previous_requested = true;
                }
                if ui.add(editor_find_button("Next")).clicked() {
                    next_requested = true;
                }
                if ui.checkbox(&mut case_sensitive, "Case").changed() {
                    case_changed = true;
                }
                if ui.add(editor_find_button("Close")).clicked() {
                    close_requested = true;
                }
            });
        });

        if clear_focus_request {
            self.editor_find.focus_requested = false;
        }
        if query_changed {
            self.set_editor_find_query(query);
        }
        if case_changed {
            self.set_editor_find_case_sensitive(case_sensitive);
        }
        if previous_requested {
            self.editor_find_previous();
        }
        if next_requested {
            self.editor_find_next();
        }
        if close_requested {
            self.close_editor_find();
        }
    }

    fn editor_find_status_text(&self) -> String {
        if self.editor_find.query.is_empty() {
            return "Enter query".to_owned();
        }
        let count = self.editor_find.matches.len();
        if count == 0 {
            return "No matches".to_owned();
        }
        let active = self.editor_find.active_match.unwrap_or(0).min(count - 1);
        format!("{} / {}", active + 1, count)
    }

    fn update_editor_find(&mut self, update: EditorFindUpdate) {
        let changed = match update {
            EditorFindUpdate::Query(query) if self.editor_find.query != query => {
                self.editor_find.query = query;
                true
            }
            EditorFindUpdate::CaseSensitive(case_sensitive)
                if self.editor_find.case_sensitive != case_sensitive =>
            {
                self.editor_find.case_sensitive = case_sensitive;
                true
            }
            _ => false,
        };
        if changed {
            self.rebuild_editor_find_after_option_change();
        }
    }

    fn rebuild_editor_find_after_option_change(&mut self) {
        self.editor_find.active_match = None;
        self.rebuild_editor_find_matches(true);
    }

    fn ensure_editor_find_matches_current(&mut self) {
        if !self.editor_find.open || self.editor_find.query.is_empty() {
            return;
        }
        if self.editor_find.last_buffer_epoch == Some(self.active_buffer_epoch)
            && self.editor_find.last_buffer_revision == Some(self.active_revision())
        {
            return;
        }
        self.rebuild_editor_find_matches(false);
    }

    fn rebuild_editor_find_matches(&mut self, select_match: bool) {
        let previous_active = self.editor_find.active_match.take();
        self.editor_find.matches.clear();
        self.editor_find.last_buffer_epoch = Some(self.active_buffer_epoch);
        self.editor_find.last_buffer_revision = Some(self.active_revision());

        if self.editor_find.query.is_empty() {
            return;
        }

        let text = self.active_snapshot();
        self.editor_find.matches = find_text_ranges(
            text.as_str(),
            self.editor_find.query.as_str(),
            self.editor_find.case_sensitive,
        );
        if self.editor_find.matches.is_empty() {
            return;
        }

        let active = previous_active
            .filter(|index| *index < self.editor_find.matches.len())
            .unwrap_or_else(|| {
                let cursor = self.virtual_editor_state.cursor();
                self.editor_find
                    .matches
                    .iter()
                    .position(|range| range.start >= cursor)
                    .unwrap_or(0)
            });
        self.editor_find.active_match = Some(active);
        if select_match {
            self.select_editor_find_match(active);
        }
    }

    fn select_editor_find_match(&mut self, index: usize) {
        let Some(range) = self.editor_find.matches.get(index).cloned() else {
            return;
        };
        let len = self.virtual_editor_buffer.len_chars();
        let start = range.start.min(len);
        let end = range.end.min(len);
        if start >= end {
            return;
        }
        self.editor_find.active_match = Some(index);
        self.virtual_editor_state.set_cursor(start, len);
        self.virtual_editor_state.move_cursor(end, len, true);
        self.virtual_editor_state.clear_preferred_column();
        self.virtual_follow_cursor_next_frame = true;
        self.reset_virtual_caret_blink();
    }
}

enum EditorFindUpdate {
    Query(String),
    CaseSensitive(bool),
}

fn find_text_ranges(text: &str, query: &str, case_sensitive: bool) -> Vec<Range<usize>> {
    let needle: Vec<char> = query.chars().collect();
    if needle.is_empty() {
        return Vec::new();
    }
    let haystack: Vec<char> = text.chars().collect();
    if needle.len() > haystack.len() {
        return Vec::new();
    }

    let mut matches = Vec::new();
    let mut start = 0;
    while start + needle.len() <= haystack.len() {
        if chars_match_at(&haystack, &needle, start, case_sensitive) {
            matches.push(start..start + needle.len());
            start += needle.len();
        } else {
            start += 1;
        }
    }
    matches
}

fn chars_match_at(haystack: &[char], needle: &[char], start: usize, case_sensitive: bool) -> bool {
    needle.iter().enumerate().all(|(offset, needle_ch)| {
        chars_equal(haystack[start + offset], *needle_ch, case_sensitive)
    })
}

fn chars_equal(left: char, right: char, case_sensitive: bool) -> bool {
    if case_sensitive || left == right {
        return left == right;
    }
    lower_chars_equal(left, right)
}

fn lower_chars_equal(left: char, right: char) -> bool {
    let mut left_lower = left.to_lowercase();
    let mut right_lower = right.to_lowercase();
    loop {
        match (left_lower.next(), right_lower.next()) {
            (Some(left), Some(right)) if left == right => continue,
            (None, None) => return true,
            _ => return false,
        }
    }
}

fn editor_find_button(label: impl Into<egui::WidgetText>) -> egui::Button<'static> {
    egui::Button::new(label)
        .small()
        .sense(non_focusable_click_sense())
}

fn apply_editor_find_row_style(ui: &mut egui::Ui) {
    let mut compact_style = (**ui.style()).clone();
    compact_style.spacing.button_padding = egui::vec2(8.0, 4.0);
    compact_style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    compact_style.spacing.interact_size.y = 26.0;
    ui.set_style(compact_style);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_text_ranges_returns_char_offsets_for_unicode_text() {
        let matches = find_text_ranges("aé needle\né needle", "needle", false);
        assert_eq!(matches, vec![3..9, 12..18]);
    }

    #[test]
    fn find_text_ranges_honors_case_sensitive_mode() {
        assert_eq!(
            find_text_ranges("Needle needle", "needle", false),
            vec![0..6, 7..13]
        );
        assert_eq!(
            find_text_ranges("Needle needle", "needle", true),
            vec![7..13]
        );
    }
}
