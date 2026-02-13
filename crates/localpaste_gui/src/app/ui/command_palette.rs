//! Command palette rendering and quick actions.

use super::super::*;
use crate::backend::CoreCmd;
use chrono::Utc;
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
                let query_resp = ui.add(
                    egui::TextEdit::singleline(&mut self.command_palette_query)
                        .hint_text("Type to search... (Enter=open, Esc=close)"),
                );
                query_resp.request_focus();

                if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                    self.command_palette_open = false;
                    return;
                }

                let results = self.rank_palette_results();
                if results.is_empty() {
                    ui.add_space(8.0);
                    ui.label(RichText::new("No results").color(COLOR_TEXT_MUTED));
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
                                    let lang = item.language.as_deref().unwrap_or("auto");
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
            self.select_paste(id);
            self.command_palette_open = false;
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
            self.try_copy_selected_immediately();
            return;
        }

        if self.backend.cmd_tx.send(CoreCmd::GetPaste { id }).is_err() {
            self.pending_copy_action = None;
            self.set_status("Load paste for copy failed: backend unavailable.");
            return;
        }
        self.set_status("Loading paste for copy...");
    }

    fn try_copy_selected_immediately(&mut self) {
        let Some(action) = self.pending_copy_action.clone() else {
            return;
        };
        let Some(paste) = self.selected_paste.as_ref() else {
            return;
        };
        match action {
            PaletteCopyAction::Raw(id) if id == paste.id => {
                self.clipboard_outgoing = Some(paste.content.clone());
                self.pending_copy_action = None;
                self.set_status("Copied paste content.");
            }
            PaletteCopyAction::Fenced(id) if id == paste.id => {
                let lang = paste.language.as_deref().unwrap_or("text");
                self.clipboard_outgoing = Some(format!("```{}\n{}\n```", lang, paste.content));
                self.pending_copy_action = None;
                self.set_status("Copied fenced code block.");
            }
            _ => {}
        }
    }

    pub(crate) fn rank_palette_results(&self) -> Vec<PasteSummary> {
        let query = self.command_palette_query.trim().to_ascii_lowercase();
        if query.is_empty() {
            let mut items = self.all_pastes.clone();
            items.sort_by(|a, b| {
                b.updated_at.cmp(&a.updated_at).then_with(|| {
                    a.name
                        .to_ascii_lowercase()
                        .cmp(&b.name.to_ascii_lowercase())
                })
            });
            items.truncate(30);
            return items;
        }

        let now = Utc::now();
        let mut scored: Vec<(i64, PasteSummary)> = self
            .all_pastes
            .iter()
            .filter_map(|item| {
                let name = item.name.to_ascii_lowercase();
                let mut score: i64 = 0;

                if name.starts_with(query.as_str()) {
                    score += 1000;
                } else if name.contains(query.as_str()) {
                    score += 700;
                }
                if item
                    .language
                    .as_ref()
                    .map(|lang| lang.to_ascii_lowercase().contains(query.as_str()))
                    .unwrap_or(false)
                {
                    score += 250;
                }
                if item
                    .tags
                    .iter()
                    .any(|tag| tag.to_ascii_lowercase().contains(query.as_str()))
                {
                    score += 250;
                }
                if score == 0 {
                    return None;
                }

                let age_days = (now - item.updated_at).num_days().max(0);
                score += (30 - age_days.min(30)) * 5;
                Some((score, item.clone()))
            })
            .collect();

        scored.sort_by(|(score_a, item_a), (score_b, item_b)| {
            score_b
                .cmp(score_a)
                .then_with(|| item_b.updated_at.cmp(&item_a.updated_at))
                .then_with(|| {
                    item_a
                        .name
                        .to_ascii_lowercase()
                        .cmp(&item_b.name.to_ascii_lowercase())
                })
        });
        scored.truncate(40);
        scored.into_iter().map(|(_, item)| item).collect()
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
}
