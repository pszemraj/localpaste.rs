//! Paste discovery with its own query, scope, and result selection.

use super::super::*;
use super::command_palette::CommandPaletteAction;

/// Stable salt used to render and reset the picker results' retained scroll state.
const RESULTS_SCROLL_ID: &str = "paste_picker_results";

impl LocalPasteApp {
    /// Open paste discovery and refresh its retained query after any closed-session results.
    pub(in crate::app) fn open_paste_picker(&mut self) {
        if self.version_overlay_open() {
            return;
        }
        self.command_palette_open = false;
        self.shortcut_help_open = false;
        self.paste_picker_open = true;
        self.paste_picker_select_query = true;
        self.reset_paste_picker_results();
        self.palette_search_last_input_at = Some(Instant::now() - SEARCH_DEBOUNCE);
    }

    /// Close paste discovery and discard its stale result projection.
    ///
    /// # Returns
    /// `false` while a selected-paste delete must retain picker ownership.
    pub(in crate::app) fn close_paste_picker(&mut self) -> bool {
        if self.picker_delete_transition_active() {
            self.set_picker_delete_transition_blocked_status();
            return false;
        }
        self.paste_picker_open = false;
        self.reset_paste_picker_results();
        self.palette_search_last_input_at = None;
        true
    }

    /// Reset selection and defer the viewport reset until results can render.
    pub(in crate::app) fn reset_paste_picker_selection(&mut self) {
        self.paste_picker_selected = 0;
        self.paste_picker_scroll_reset_pending = true;
    }

    /// Keep the selected result within the current paste-only result list.
    pub(in crate::app) fn clamp_paste_picker_selection(&mut self, results_len: usize) {
        self.paste_picker_selected = self
            .paste_picker_selected
            .min(results_len.saturating_sub(1));
    }

    /// Render the paste picker independently of the commands-only palette.
    ///
    /// # Panics
    /// Panics if egui text layout fails while shaping rows.
    pub(in crate::app) fn render_paste_picker(&mut self, ctx: &egui::Context) {
        if !self.paste_picker_open {
            return;
        }
        let mut pending = None;
        egui::Window::new("Paste Picker")
            .id(egui::Id::new("paste_picker_modal"))
            .collapsible(false)
            .resizable(false)
            .default_width(720.0)
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 60.0))
            .show(ctx, |ui| {
                let mut query = self.paste_picker_query.clone();
                let query_id = egui::Id::new(PASTE_PICKER_INPUT_ID);
                let query_visible = super::focus_visible_query(ui, query_id);
                if query_visible && std::mem::take(&mut self.paste_picker_select_query) {
                    let mut state =
                        egui::text_edit::TextEditState::load(ctx, query_id).unwrap_or_default();
                    state
                        .cursor
                        .set_char_range(Some(egui::text::CCursorRange::two(
                            egui::text::CCursor::new(0),
                            egui::text::CCursor::new(query.chars().count()),
                        )));
                    state.store(ctx, query_id);
                }
                let response = egui::TextEdit::singleline(&mut query)
                    .id(query_id)
                    .return_key(None)
                    .hint_text("Search pastes...")
                    .show(ui)
                    .response;
                if response.changed() {
                    self.set_paste_picker_query(query);
                }
                let scope = super::search_scope::scope_selector(
                    ui,
                    "picker_scope",
                    self.paste_picker_scope,
                );
                self.set_paste_picker_scope(scope);
                if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                    if self.close_paste_picker() {
                        self.restore_discovery_focus(ctx);
                    }
                    return;
                }
                let results: Vec<_> = if self.paste_picker_query.trim().is_empty() {
                    self.all_pastes.iter().take(30).cloned().collect()
                } else {
                    self.palette_search_results.clone()
                };
                if results.is_empty() {
                    if let Some(error) = &self.palette_search_error {
                        ui.label(error);
                        if ui
                            .add_enabled(!self.palette_search_pending, egui::Button::new("Retry"))
                            .clicked()
                        {
                            self.palette_search_last_input_at =
                                Some(Instant::now() - SEARCH_DEBOUNCE);
                        }
                        return;
                    }
                    ui.label(
                        if self.palette_search_pending
                            || self.palette_search_last_sent != self.paste_picker_query.trim()
                        {
                            "Searching..."
                        } else {
                            "No matching pastes"
                        },
                    );
                    return;
                }
                let down =
                    ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown));
                let up = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp));
                if down {
                    self.paste_picker_selected += 1;
                }
                if up {
                    self.paste_picker_selected = self.paste_picker_selected.saturating_sub(1);
                }
                self.clamp_paste_picker_selection(results.len());
                if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
                    pending = Some(CommandPaletteAction::OpenPaste(
                        results[self.paste_picker_selected].id.clone(),
                    ));
                }
                // Loading frames return above without consuming this one-shot reset.
                if std::mem::take(&mut self.paste_picker_scroll_reset_pending) {
                    // Clear animation targets and momentum along with the old offset.
                    let id = ui.make_persistent_id(egui::Id::new(RESULTS_SCROLL_ID));
                    egui::scroll_area::State::default().store(ctx, id);
                }
                egui::ScrollArea::vertical()
                    .id_salt(RESULTS_SCROLL_ID)
                    .max_height(360.0)
                    .show(ui, |ui| {
                        for (index, item) in results.iter().enumerate() {
                            let selected = self.paste_picker_selected == index;
                            let row_response = ui
                                .vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        let response = ui.selectable_label(selected, &item.name);
                                        if response.clicked() {
                                            pending = Some(CommandPaletteAction::OpenPaste(
                                                item.id.clone(),
                                            ));
                                        }
                                        ui.label(
                                            RichText::new(display_language_label(
                                                item.language.as_deref(),
                                                false,
                                                item.content_len >= HIGHLIGHT_PLAIN_THRESHOLD,
                                            ))
                                            .small()
                                            .color(COLOR_TEXT_MUTED),
                                        );
                                        for (label, action) in [
                                            (
                                                "Copy",
                                                CommandPaletteAction::CopyPasteRaw(item.id.clone()),
                                            ),
                                            (
                                                "Copy Fenced",
                                                CommandPaletteAction::CopyPasteFenced(
                                                    item.id.clone(),
                                                ),
                                            ),
                                            (
                                                "Delete",
                                                CommandPaletteAction::DeletePaste(item.id.clone()),
                                            ),
                                        ] {
                                            if ui.small_button(label).clicked() {
                                                pending = Some(action);
                                            }
                                        }
                                    });
                                    if let Some(excerpt) = item.match_excerpt.as_deref() {
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(excerpt)
                                                    .small()
                                                    .color(COLOR_TEXT_MUTED),
                                            )
                                            .wrap(),
                                        );
                                    }
                                })
                                .response;
                            if selected && (down || up) {
                                row_response.scroll_to_me(None);
                            }
                        }
                    });
            });
        if let Some(action) = pending {
            self.execute_command_palette_action(ctx, action);
        }
    }
}
