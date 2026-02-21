//! Command palette rendering and quick actions.

use super::super::*;
use crate::backend::CoreCmd;
use eframe::egui::{self, RichText};

/// Executable actions exposed by the command palette.
#[derive(Clone, Debug)]
pub(crate) enum CommandPaletteAction {
    NewPaste,
    DeleteSelected,
    SaveNow,
    SaveMetadata,
    FocusSearch,
    ToggleProperties,
    RefreshList,
    OpenPaste(String),
    DeletePaste(String),
    CopyPasteRaw(String),
    CopyPasteFenced(String),
}

/// Display row for command actions in the palette command section.
#[derive(Clone, Debug)]
pub(crate) struct CommandPaletteItem {
    pub(crate) label: String,
    pub(crate) hint: String,
    pub(crate) action: CommandPaletteAction,
}

impl LocalPasteApp {
    /// Renders the command palette modal and handles quick-action input.
    ///
    /// # Panics
    /// Panics if egui text layout internals fail while shaping palette rows.
    pub(crate) fn render_command_palette(&mut self, ctx: &egui::Context) {
        if !self.command_palette_open {
            return;
        }

        let mut pending_action: Option<CommandPaletteAction> = None;

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
                        .hint_text("Run a command or search pastes..."),
                );
                query_resp.request_focus();
                if query_resp.changed() {
                    self.set_command_palette_query(query_buf);
                }

                if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                    self.command_palette_open = false;
                    return;
                }

                let actions = self.command_palette_actions();
                let results = self.palette_results();
                let total_items = actions.len().saturating_add(results.len());
                if total_items == 0 {
                    ui.add_space(8.0);
                    let query = self.command_palette_query.trim();
                    if !query.is_empty() && self.palette_search_last_sent != query {
                        ui.label(RichText::new("Searching...").color(COLOR_TEXT_MUTED));
                    } else {
                        ui.label(RichText::new("No commands or results").color(COLOR_TEXT_MUTED));
                    }
                    return;
                }
                self.clamp_command_palette_selection();

                if ui.input(|input| input.key_pressed(egui::Key::ArrowDown)) {
                    self.command_palette_selected =
                        (self.command_palette_selected + 1).min(total_items.saturating_sub(1));
                }
                if ui.input(|input| input.key_pressed(egui::Key::ArrowUp)) {
                    self.command_palette_selected = self.command_palette_selected.saturating_sub(1);
                }
                if ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                    if self.command_palette_selected < actions.len() {
                        pending_action =
                            Some(actions[self.command_palette_selected].action.clone());
                    } else {
                        let idx = self.command_palette_selected.saturating_sub(actions.len());
                        if idx < results.len() {
                            pending_action =
                                Some(CommandPaletteAction::OpenPaste(results[idx].id.clone()));
                        }
                    }
                }

                ui.add_space(8.0);
                ui.label(RichText::new("Commands").small().color(COLOR_TEXT_MUTED));
                for (idx, item) in actions.iter().enumerate() {
                    let selected = idx == self.command_palette_selected;
                    let response = ui.selectable_label(
                        selected,
                        RichText::new(format!("{}  {}", item.label, item.hint)),
                    );
                    if response.clicked() {
                        self.command_palette_selected = idx;
                        pending_action = Some(item.action.clone());
                    }
                }

                ui.add_space(6.0);
                ui.label(RichText::new("Pastes").small().color(COLOR_TEXT_MUTED));
                let row_height = ui.spacing().interact_size.y + 6.0;
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .auto_shrink([false; 2])
                    .show_rows(ui, row_height, results.len(), |ui, range| {
                        for idx in range {
                            if let Some(item) = results.get(idx) {
                                let absolute_idx = actions.len().saturating_add(idx);
                                let selected = absolute_idx == self.command_palette_selected;
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
                                        self.command_palette_selected = absolute_idx;
                                        pending_action =
                                            Some(CommandPaletteAction::OpenPaste(item.id.clone()));
                                    }
                                    if ui.small_button("Delete").clicked() {
                                        pending_action = Some(CommandPaletteAction::DeletePaste(
                                            item.id.clone(),
                                        ));
                                    }
                                    if ui.small_button("Copy").clicked() {
                                        pending_action = Some(CommandPaletteAction::CopyPasteRaw(
                                            item.id.clone(),
                                        ));
                                    }
                                    if ui.small_button("Copy Fenced").clicked() {
                                        pending_action = Some(
                                            CommandPaletteAction::CopyPasteFenced(item.id.clone()),
                                        );
                                    }
                                });
                            }
                        }
                    });
            });

        if let Some(action) = pending_action {
            self.execute_command_palette_action(action);
        }
    }

    fn execute_command_palette_action(&mut self, action: CommandPaletteAction) {
        match action {
            CommandPaletteAction::NewPaste => {
                self.create_new_paste();
                self.command_palette_open = false;
            }
            CommandPaletteAction::DeleteSelected => {
                self.delete_selected();
                self.command_palette_open = false;
            }
            CommandPaletteAction::SaveNow => {
                self.save_now();
                self.save_metadata_now();
                self.command_palette_open = false;
            }
            CommandPaletteAction::SaveMetadata => {
                self.save_metadata_now();
                self.command_palette_open = false;
            }
            CommandPaletteAction::FocusSearch => {
                self.search_focus_requested = true;
                self.command_palette_open = false;
            }
            CommandPaletteAction::ToggleProperties => {
                self.properties_drawer_open = !self.properties_drawer_open;
                self.command_palette_open = false;
            }
            CommandPaletteAction::RefreshList => {
                self.request_refresh();
                self.command_palette_open = false;
            }
            CommandPaletteAction::OpenPaste(id) => {
                self.open_palette_selection(id);
            }
            CommandPaletteAction::DeletePaste(id) => {
                self.send_palette_delete(id);
            }
            CommandPaletteAction::CopyPasteRaw(id) => {
                self.queue_palette_copy(id, false);
            }
            CommandPaletteAction::CopyPasteFenced(id) => {
                self.queue_palette_copy(id, true);
            }
        }
    }

    /// Returns the current count of executable command rows visible in the palette.
    ///
    /// # Returns
    /// Number of command rows after query filtering.
    pub(crate) fn command_palette_action_count(&self) -> usize {
        self.command_palette_actions().len()
    }

    /// Clamps the absolute palette selection index using command rows + result rows.
    ///
    /// # Arguments
    /// - `results_len`: Number of paste-result rows currently available.
    pub(crate) fn clamp_command_palette_selection_with_results_len(&mut self, results_len: usize) {
        let total_items = self
            .command_palette_action_count()
            .saturating_add(results_len);
        if total_items == 0 {
            self.command_palette_selected = 0;
            return;
        }
        if self.command_palette_selected >= total_items {
            self.command_palette_selected = total_items.saturating_sub(1);
        }
    }

    fn clamp_command_palette_selection(&mut self) {
        self.clamp_command_palette_selection_with_results_len(self.palette_results().len());
    }

    fn command_palette_actions(&self) -> Vec<CommandPaletteItem> {
        let query = self.command_palette_query.trim().to_ascii_lowercase();
        let mut items = Vec::new();

        items.push(CommandPaletteItem {
            label: "New paste".to_string(),
            hint: "(Ctrl/Cmd+N)".to_string(),
            action: CommandPaletteAction::NewPaste,
        });
        if self.selected_id.is_some() {
            items.push(CommandPaletteItem {
                label: "Delete selected".to_string(),
                hint: "(Ctrl/Cmd+Delete)".to_string(),
                action: CommandPaletteAction::DeleteSelected,
            });
            items.push(CommandPaletteItem {
                label: "Save now".to_string(),
                hint: "(Ctrl/Cmd+S)".to_string(),
                action: CommandPaletteAction::SaveNow,
            });
            items.push(CommandPaletteItem {
                label: "Save metadata".to_string(),
                hint: "persist title/type/tags".to_string(),
                action: CommandPaletteAction::SaveMetadata,
            });
        }
        items.push(CommandPaletteItem {
            label: "Focus sidebar search".to_string(),
            hint: "(Ctrl/Cmd+F)".to_string(),
            action: CommandPaletteAction::FocusSearch,
        });
        items.push(CommandPaletteItem {
            label: "Toggle properties".to_string(),
            hint: "(Ctrl/Cmd+I)".to_string(),
            action: CommandPaletteAction::ToggleProperties,
        });
        items.push(CommandPaletteItem {
            label: "Refresh list".to_string(),
            hint: "reload from backend".to_string(),
            action: CommandPaletteAction::RefreshList,
        });

        if query.is_empty() {
            return items;
        }
        items
            .into_iter()
            .filter(|item| {
                let haystack = format!(
                    "{} {}",
                    item.label.to_ascii_lowercase(),
                    item.hint.to_ascii_lowercase()
                );
                haystack.contains(query.as_str())
            })
            .collect()
    }

    /// Queues a copy action for a palette result, loading selection if needed.
    ///
    /// # Arguments
    /// - `id`: Paste id targeted by the copy action.
    /// - `fenced`: When `true`, copy as fenced Markdown code block.
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

    /// Sends a delete command for a palette-selected paste and closes palette.
    pub(crate) fn send_palette_delete(&mut self, id: String) {
        if self.send_delete_paste(id) {
            self.command_palette_open = false;
        }
    }

    /// Opens the selected palette result in the main editor view.
    pub(crate) fn open_palette_selection(&mut self, id: String) {
        if self.select_paste(id) {
            self.command_palette_open = false;
        }
    }
}
