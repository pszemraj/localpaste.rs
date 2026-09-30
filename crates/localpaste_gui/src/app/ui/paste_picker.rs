//! Paste discovery with its own query, scope, and result selection.

use super::super::*;
use super::command_palette::CommandPaletteAction;

impl LocalPasteApp {
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
                let response = ui.add(
                    egui::TextEdit::singleline(&mut query)
                        .id(egui::Id::new(PASTE_PICKER_INPUT_ID))
                        .return_key(None)
                        .hint_text("Search pastes..."),
                );
                response.request_focus();
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
                    self.paste_picker_open = false;
                    return;
                }
                let results: Vec<_> = if self.paste_picker_query.trim().is_empty() {
                    self.all_pastes.iter().take(30).cloned().collect()
                } else {
                    self.palette_search_results.clone()
                };
                if results.is_empty() {
                    ui.label(
                        if self.palette_search_last_sent != self.paste_picker_query.trim() {
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
                egui::ScrollArea::vertical()
                    .max_height(360.0)
                    .show(ui, |ui| {
                        for (index, item) in results.iter().enumerate() {
                            ui.horizontal(|ui| {
                                let selected = self.paste_picker_selected == index;
                                let response = ui.selectable_label(selected, &item.name);
                                if selected && (down || up) {
                                    response.scroll_to_me(None);
                                }
                                if response.clicked() {
                                    pending =
                                        Some(CommandPaletteAction::OpenPaste(item.id.clone()));
                                }
                                ui.label(
                                    RichText::new(display_language_label(
                                        item.language.as_deref(),
                                        false,
                                        false,
                                    ))
                                    .small()
                                    .color(COLOR_TEXT_MUTED),
                                );
                                for (label, action) in [
                                    ("Copy", CommandPaletteAction::CopyPasteRaw(item.id.clone())),
                                    (
                                        "Copy Fenced",
                                        CommandPaletteAction::CopyPasteFenced(item.id.clone()),
                                    ),
                                    ("Delete", CommandPaletteAction::DeletePaste(item.id.clone())),
                                ] {
                                    if ui.small_button(label).clicked() {
                                        pending = Some(action);
                                    }
                                }
                            });
                        }
                    });
            });
        if let Some(action) = pending {
            self.execute_command_palette_action(ctx, action);
        }
    }
}
