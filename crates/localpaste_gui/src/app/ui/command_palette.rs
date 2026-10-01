//! Command palette rendering and quick actions.

use super::super::shortcuts::{runtime_shortcut_label, RuntimeShortcutAction};
use super::super::*;
use crate::backend::CoreCmd;
use eframe::egui;

/// Executable actions exposed by the command palette.
#[derive(Clone, Debug)]
pub(crate) enum CommandPaletteAction {
    NewPaste,
    PasteAsNew,
    DeleteSelected,
    SaveNow,
    SaveMetadata,
    OpenDiffModal,
    OpenHistoryModal,
    FocusSearch,
    ToggleProperties,
    RefreshList,
    Export,
    Duplicate,
    Copy,
    CopyLink,
    Find,
    PastePicker,
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
    /// Render commands without interleaving paste search results.
    ///
    /// # Panics
    /// Panics if egui text layout fails while shaping rows.
    pub(crate) fn render_command_palette(&mut self, ctx: &egui::Context) {
        if !self.command_palette_open {
            return;
        }
        let mut pending = None;
        egui::Window::new("Command Palette")
            .id(egui::Id::new("command_palette_modal"))
            .collapsible(false)
            .resizable(false)
            .default_width(680.0)
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 60.0))
            .show(ctx, |ui| {
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.command_palette_query)
                        .id(egui::Id::new(COMMAND_PALETTE_INPUT_ID))
                        .return_key(None)
                        .hint_text("Search commands..."),
                );
                response.request_focus();
                if response.changed() {
                    self.command_palette_selected = 0;
                }
                if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
                {
                    self.command_palette_open = false;
                    return;
                }
                let actions = self.command_palette_actions();
                if actions.is_empty() {
                    ui.label("No matching commands");
                    return;
                }
                let down = ui.input_mut(|input| {
                    input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)
                });
                let up = ui.input_mut(|input| {
                    input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)
                });
                if down {
                    self.command_palette_selected += 1;
                }
                if up {
                    self.command_palette_selected = self.command_palette_selected.saturating_sub(1);
                }
                self.command_palette_selected =
                    self.command_palette_selected.min(actions.len() - 1);
                if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
                {
                    pending = Some(actions[self.command_palette_selected].action.clone());
                }
                egui::ScrollArea::vertical()
                    .max_height(400.0)
                    .show(ui, |ui| {
                        for (index, item) in actions.iter().enumerate() {
                            let selected = index == self.command_palette_selected;
                            let row = ui.selectable_label(
                                selected,
                                format!("{}  {}", item.label, item.hint),
                            );
                            if selected
                                && (down || up || response.changed() || response.gained_focus())
                            {
                                row.scroll_to_me(None);
                            }
                            if row.clicked() {
                                pending = Some(item.action.clone());
                            }
                        }
                    });
            });
        if let Some(action) = pending {
            self.execute_command_palette_action(ctx, action);
        }
    }

    /// Execute a command or a paste-picker row action using the normal app workflow.
    ///
    /// # Arguments
    /// - `ctx`: Context for clipboard requests.
    /// - `action`: Selected command or result action.
    pub(super) fn execute_command_palette_action(
        &mut self,
        ctx: &egui::Context,
        action: CommandPaletteAction,
    ) {
        if self.mutation_shortcut_block_reason().is_some()
            && matches!(
                action,
                CommandPaletteAction::NewPaste
                    | CommandPaletteAction::PasteAsNew
                    | CommandPaletteAction::Duplicate
                    | CommandPaletteAction::DeleteSelected
                    | CommandPaletteAction::DeletePaste(_)
            )
        {
            self.set_mutation_shortcut_blocked_status();
            return;
        }
        match action {
            CommandPaletteAction::Export => {
                self.export_selected_paste();
                self.command_palette_open = false;
            }
            CommandPaletteAction::Duplicate => {
                self.create_new_paste_with_content(self.active_snapshot());
                self.command_palette_open = false;
            }
            CommandPaletteAction::Copy => {
                self.clipboard_outgoing = Some(self.active_snapshot());
                self.set_status("Copied paste content.");
                self.command_palette_open = false;
            }
            CommandPaletteAction::CopyLink => {
                if let Some(id) = &self.selected_id {
                    self.clipboard_outgoing =
                        Some(util::api_paste_link_for_copy(self.server_addr, id));
                }
                self.command_palette_open = false;
            }
            CommandPaletteAction::Find => {
                self.open_editor_find();
                self.command_palette_open = false;
            }
            CommandPaletteAction::PastePicker => {
                self.open_paste_picker();
            }
            CommandPaletteAction::NewPaste => {
                self.create_new_paste();
                self.command_palette_open = false;
            }
            CommandPaletteAction::PasteAsNew => {
                self.request_paste_as_new(ctx);
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
            CommandPaletteAction::OpenDiffModal => {
                self.open_diff_modal();
                self.command_palette_open = false;
            }
            CommandPaletteAction::OpenHistoryModal => {
                self.open_history_modal();
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

    /// Build the executable command rows matching the current command query.
    ///
    /// # Returns
    /// Commands available for the current selection, filtered by label and hint.
    pub(in crate::app) fn command_palette_actions(&self) -> Vec<CommandPaletteItem> {
        let query = self.command_palette_query.trim().to_ascii_lowercase();
        let mut items = Vec::new();

        items.push(CommandPaletteItem {
            label: "New paste".to_string(),
            hint: shortcut_hint(RuntimeShortcutAction::NewPaste),
            action: CommandPaletteAction::NewPaste,
        });
        items.push(CommandPaletteItem {
            label: "Paste as new paste".to_string(),
            hint: shortcut_hint(RuntimeShortcutAction::PasteAsNew),
            action: CommandPaletteAction::PasteAsNew,
        });
        items.push(CommandPaletteItem {
            label: "Open paste picker".into(),
            hint: shortcut_hint(RuntimeShortcutAction::TogglePastePicker),
            action: CommandPaletteAction::PastePicker,
        });
        if self.selected_id.is_some() {
            for (label, action) in [
                ("Export paste", CommandPaletteAction::Export),
                ("Duplicate paste", CommandPaletteAction::Duplicate),
                ("Copy paste", CommandPaletteAction::Copy),
                ("Copy link", CommandPaletteAction::CopyLink),
                ("Find in paste", CommandPaletteAction::Find),
            ] {
                items.push(CommandPaletteItem {
                    label: label.into(),
                    hint: String::new(),
                    action,
                });
            }
            items.push(CommandPaletteItem {
                label: "Delete selected".to_string(),
                hint: shortcut_hint(RuntimeShortcutAction::DeleteSelected),
                action: CommandPaletteAction::DeleteSelected,
            });
            items.push(CommandPaletteItem {
                label: "Save now".to_string(),
                hint: shortcut_hint(RuntimeShortcutAction::Save),
                action: CommandPaletteAction::SaveNow,
            });
            items.push(CommandPaletteItem {
                label: "Save metadata".to_string(),
                hint: "persist title/type/tags".to_string(),
                action: CommandPaletteAction::SaveMetadata,
            });
            items.push(CommandPaletteItem {
                label: "Open diff modal".to_string(),
                hint: "compare current paste".to_string(),
                action: CommandPaletteAction::OpenDiffModal,
            });
            items.push(CommandPaletteItem {
                label: "Open history modal".to_string(),
                hint: "browse snapshots".to_string(),
                action: CommandPaletteAction::OpenHistoryModal,
            });
        }
        items.push(CommandPaletteItem {
            label: "Focus sidebar search".to_string(),
            hint: shortcut_hint(RuntimeShortcutAction::FocusSearch),
            action: CommandPaletteAction::FocusSearch,
        });
        items.push(CommandPaletteItem {
            label: "Toggle properties".to_string(),
            hint: shortcut_hint(RuntimeShortcutAction::ToggleProperties),
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

        if !self.dispatch_backend_cmd(CoreCmd::GetPaste { id }) {
            self.pending_copy_action = None;
            self.set_status("Load paste for copy failed: backend unavailable.");
            return;
        }
        self.set_status("Loading paste for copy...");
    }

    /// Sends a delete command for a palette-selected paste and closes palette.
    pub(crate) fn send_palette_delete(&mut self, id: String) {
        if self.send_delete_paste(id) {
            self.close_paste_picker();
        }
    }

    /// Opens the selected palette result in the main editor view.
    pub(crate) fn open_palette_selection(&mut self, id: String) {
        if self.select_paste(id) {
            self.close_paste_picker();
        }
    }
}

fn shortcut_hint(action: RuntimeShortcutAction) -> String {
    format!("({})", runtime_shortcut_label(action))
}
