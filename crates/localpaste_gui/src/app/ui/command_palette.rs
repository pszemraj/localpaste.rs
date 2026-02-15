//! Command palette rendering and quick actions.

use super::super::*;
use crate::backend::CoreCmd;
use eframe::egui::{self, RichText};

impl LocalPasteApp {
    pub(crate) fn render_command_palette(&mut self, ctx: &egui::Context) {
        if !self.command_palette_open {
            return;
        }

        let mut pending_open: Option<String> = None;
        let mut pending_delete: Option<String> = None;
        let mut pending_copy_raw: Option<String> = None;
        let mut pending_copy_fenced: Option<String> = None;

        egui::Window::new("Command Palette")
            .id(egui::Id::new("command_palette_modal"))
            .collapsible(false)
            .resizable(false)
            .default_width(680.0)
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 60.0))
            .show(ctx, |ui| {
                let mut query_buf = self.command_palette_query.clone();
                let query_resp = ui.add(
                    egui::TextEdit::singleline(&mut query_buf)
                        .hint_text("Type to search... (Enter=open, Esc=close)"),
                );
                query_resp.request_focus();
                if query_resp.changed() {
                    self.set_command_palette_query(query_buf);
                }

                if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                    self.command_palette_open = false;
                    return;
                }

                let results = self.palette_results();
                if results.is_empty() {
                    ui.add_space(8.0);
                    let query = self.command_palette_query.trim();
                    if !query.is_empty() && self.palette_search_last_sent != query {
                        ui.label(RichText::new("Searching...").color(COLOR_TEXT_MUTED));
                    } else {
                        ui.label(RichText::new("No results").color(COLOR_TEXT_MUTED));
                    }
                    return;
                }
                if self.command_palette_selected >= results.len() {
                    self.command_palette_selected = results.len().saturating_sub(1);
                }

                if ui.input(|input| input.key_pressed(egui::Key::ArrowDown)) {
                    self.command_palette_selected =
                        (self.command_palette_selected + 1).min(results.len().saturating_sub(1));
                }
                if ui.input(|input| input.key_pressed(egui::Key::ArrowUp)) {
                    self.command_palette_selected = self.command_palette_selected.saturating_sub(1);
                }
                if ui.input(|input| input.key_pressed(egui::Key::Enter))
                    && self.command_palette_selected < results.len()
                {
                    pending_open = Some(results[self.command_palette_selected].id.clone());
                }

                ui.add_space(8.0);
                let row_height = ui.spacing().interact_size.y + 6.0;
                egui::ScrollArea::vertical()
                    .max_height(360.0)
                    .auto_shrink([false; 2])
                    .show_rows(ui, row_height, results.len(), |ui, range| {
                        for idx in range {
                            if let Some(item) = results.get(idx) {
                                let selected = idx == self.command_palette_selected;
                                ui.horizontal(|ui| {
                                    let lang = display_language_label(
                                        item.language.as_deref(),
                                        false,
                                        item.content_len >= HIGHLIGHT_PLAIN_THRESHOLD,
                                    );
                                    let label = format!("{}  [{}]", item.name, lang);
                                    if ui
                                        .selectable_label(selected, RichText::new(label))
                                        .clicked()
                                    {
                                        self.command_palette_selected = idx;
                                        pending_open = Some(item.id.clone());
                                    }
                                    if ui.small_button("Delete").clicked() {
                                        pending_delete = Some(item.id.clone());
                                    }
                                    if ui.small_button("Copy").clicked() {
                                        pending_copy_raw = Some(item.id.clone());
                                    }
                                    if ui.small_button("Copy Fenced").clicked() {
                                        pending_copy_fenced = Some(item.id.clone());
                                    }
                                });
                            }
                        }
                    });
            });

        if let Some(id) = pending_open {
            self.open_palette_selection(id);
        }
        if let Some(id) = pending_delete {
            self.send_palette_delete(id);
        }
        if let Some(id) = pending_copy_raw {
            self.queue_palette_copy(id, false);
        }
        if let Some(id) = pending_copy_fenced {
            self.queue_palette_copy(id, true);
        }
    }

    pub(crate) fn queue_palette_copy(&mut self, id: String, fenced: bool) {
        let action = if fenced {
            PaletteCopyAction::Fenced(id.clone())
        } else {
            PaletteCopyAction::Raw(id.clone())
        };
        self.pending_copy_action = Some(action);

        if self.selected_id.as_deref() != Some(id.as_str()) {
            if !self.select_paste(id.clone()) {
                self.pending_copy_action = None;
                return;
            }
            self.set_status("Loading paste for copy...");
            return;
        }

        if self.selected_paste.is_some() {
            self.try_complete_pending_copy();
            return;
        }

        if self.backend.cmd_tx.send(CoreCmd::GetPaste { id }).is_err() {
            self.pending_copy_action = None;
            self.set_status("Load paste for copy failed: backend unavailable.");
            return;
        }
        self.set_status("Loading paste for copy...");
    }

    fn palette_results(&self) -> Vec<PasteSummary> {
        if self.command_palette_query.trim().is_empty() {
            return self.all_pastes.iter().take(30).cloned().collect();
        }
        self.palette_search_results.clone()
    }

    pub(crate) fn send_palette_delete(&mut self, id: String) {
        if self
            .backend
            .cmd_tx
            .send(CoreCmd::DeletePaste { id })
            .is_err()
        {
            self.set_status("Delete failed: backend unavailable.");
            return;
        }
        self.command_palette_open = false;
    }

    pub(crate) fn open_palette_selection(&mut self, id: String) {
        if self.select_paste(id) {
            self.command_palette_open = false;
        }
    }
}
