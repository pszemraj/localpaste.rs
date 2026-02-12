//! Top bar and sidebar rendering for paste navigation and quick actions.

use super::super::*;
use crate::backend::CoreCmd;
use eframe::egui::{self, RichText};
use localpaste_core::folder_ops::introduces_cycle;
use std::collections::BTreeSet;

impl LocalPasteApp {
    pub(crate) fn render_top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top")
            .resizable(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(RichText::new("LocalPaste.rs").color(COLOR_ACCENT));
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new(&self.db_path)
                            .monospace()
                            .color(COLOR_TEXT_SECONDARY),
                    );
                });
            });
    }

    pub(crate) fn render_sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("sidebar")
            .default_width(300.0)
            .show(ctx, |ui| {
                ui.heading(
                    RichText::new(format!("Pastes ({}/{})", self.pastes.len(), self.all_pastes.len()))
                        .color(COLOR_TEXT_PRIMARY),
                );
                ui.add_space(8.0);

                let mut search_buf = self.search_query.clone();
                let search_resp = ui.add(
                    egui::TextEdit::singleline(&mut search_buf)
                        .id_salt(SEARCH_INPUT_ID)
                        .hint_text("Search pastes... (Ctrl/Cmd+F)"),
                );
                if self.search_focus_requested {
                    search_resp.request_focus();
                    self.search_focus_requested = false;
                }
                if search_resp.changed() {
                    self.set_search_query(search_buf);
                }

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("+ New Paste").clicked() {
                        self.create_new_paste();
                    }
                    if ui
                        .add_enabled(self.selected_id.is_some(), egui::Button::new("Delete"))
                        .clicked()
                    {
                        self.delete_selected();
                    }
                });

                ui.add_space(10.0);
                ui.label(RichText::new("Collections").small().color(COLOR_TEXT_MUTED));
                let mut pending_collection: Option<SidebarCollection> = None;
                self.render_collection_button(
                    ui,
                    "All",
                    matches!(self.active_collection, SidebarCollection::All),
                    SidebarCollection::All,
                    &mut pending_collection,
                );
                self.render_collection_button(
                    ui,
                    "Recent (7d)",
                    matches!(self.active_collection, SidebarCollection::Recent),
                    SidebarCollection::Recent,
                    &mut pending_collection,
                );
                self.render_collection_button(
                    ui,
                    "Unfiled",
                    matches!(self.active_collection, SidebarCollection::Unfiled),
                    SidebarCollection::Unfiled,
                    &mut pending_collection,
                );

                let mut langs: BTreeSet<String> = BTreeSet::new();
                for paste in &self.all_pastes {
                    if let Some(lang) = paste
                        .language
                        .as_ref()
                        .map(|v| v.trim())
                        .filter(|v| !v.is_empty())
                    {
                        langs.insert(lang.to_string());
                    }
                }
                if !langs.is_empty() {
                    ui.add_space(8.0);
                    ui.label(RichText::new("By Language").small().color(COLOR_TEXT_MUTED));
                    for lang in langs {
                        let selected = matches!(&self.active_collection, SidebarCollection::Language(value) if value.eq_ignore_ascii_case(&lang));
                        self.render_collection_button(
                            ui,
                            lang.as_str(),
                            selected,
                            SidebarCollection::Language(lang.clone()),
                            &mut pending_collection,
                        );
                    }
                }

                ui.add_space(8.0);
                ui.label(RichText::new("Folders").small().color(COLOR_TEXT_MUTED));
                ui.horizontal(|ui| {
                    if ui.small_button("+ Folder").clicked() {
                        self.folder_dialog = Some(FolderDialog::Create {
                            name: String::new(),
                            parent_id: self.active_folder_id(),
                        });
                    }
                    let active_folder = self.active_folder();
                    if ui
                        .add_enabled(active_folder.is_some(), egui::Button::new("Rename"))
                        .clicked()
                    {
                        if let Some(folder) = active_folder.clone() {
                            self.folder_dialog = Some(FolderDialog::Edit {
                                id: folder.id,
                                name: folder.name,
                                parent_id: folder.parent_id,
                            });
                        }
                    }
                    if ui
                        .add_enabled(active_folder.is_some(), egui::Button::new("Delete"))
                        .clicked()
                    {
                        if let Some(folder) = active_folder {
                            self.folder_dialog = Some(FolderDialog::Delete {
                                id: folder.id,
                                name: folder.name,
                            });
                        }
                    }
                });
                self.render_folder_collection_nodes(ui, None, &mut pending_collection);

                if let Some(collection) = pending_collection {
                    self.set_active_collection(collection);
                }

                ui.separator();
                ui.add_space(4.0);
                let mut pending_select: Option<String> = None;
                let row_height = ui.spacing().interact_size.y;
                egui::ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .show_rows(ui, row_height, self.pastes.len(), |ui, range| {
                        for idx in range {
                            if let Some(paste) = self.pastes.get(idx) {
                                let selected = self.selected_id.as_deref() == Some(paste.id.as_str());
                                let lang_label = display_language_label(
                                    paste.language.as_deref(),
                                    paste.content_len >= HIGHLIGHT_PLAIN_THRESHOLD,
                                );
                                let label = format!("{}  ({})", paste.name, lang_label);
                                if ui
                                    .selectable_label(selected, RichText::new(label))
                                    .clicked()
                                {
                                    pending_select = Some(paste.id.clone());
                                }
                            }
                        }
                    });
                if let Some(id) = pending_select {
                    self.select_paste(id);
                }
            });

        self.render_folder_dialog(ctx);
    }

    fn render_collection_button(
        &self,
        ui: &mut egui::Ui,
        label: &str,
        selected: bool,
        collection: SidebarCollection,
        pending_collection: &mut Option<SidebarCollection>,
    ) {
        if ui.selectable_label(selected, label).clicked() {
            *pending_collection = Some(collection);
        }
    }

    fn render_folder_collection_nodes(
        &self,
        ui: &mut egui::Ui,
        parent_id: Option<&str>,
        pending_collection: &mut Option<SidebarCollection>,
    ) {
        let mut children: Vec<Folder> = self
            .folders
            .iter()
            .filter(|folder| folder.parent_id.as_deref() == parent_id)
            .cloned()
            .collect();
        children.sort_by(|a, b| {
            a.name
                .to_ascii_lowercase()
                .cmp(&b.name.to_ascii_lowercase())
        });

        for folder in children {
            let count = self.folder_paste_count(Some(folder.id.as_str()));
            let label = format!("{} ({})", folder.name, count);
            let selected = matches!(&self.active_collection, SidebarCollection::Folder(id) if id == &folder.id);
            if ui.selectable_label(selected, label).clicked() {
                *pending_collection = Some(SidebarCollection::Folder(folder.id.clone()));
            }
            ui.indent(format!("folder-indent-{}", folder.id), |ui| {
                self.render_folder_collection_nodes(
                    ui,
                    Some(folder.id.as_str()),
                    pending_collection,
                );
            });
        }
    }

    fn active_folder_id(&self) -> Option<String> {
        match &self.active_collection {
            SidebarCollection::Folder(id) => Some(id.clone()),
            _ => None,
        }
    }

    fn active_folder(&self) -> Option<Folder> {
        let id = self.active_folder_id()?;
        self.folders.iter().find(|folder| folder.id == id).cloned()
    }

    fn folder_parent_choices(&self, editing_id: Option<&str>) -> Vec<(Option<String>, String)> {
        let mut choices: Vec<(Option<String>, String)> = vec![(None, "Top level".to_string())];
        let mut folders: Vec<Folder> = self.folders.clone();
        folders.sort_by(|a, b| {
            a.name
                .to_ascii_lowercase()
                .cmp(&b.name.to_ascii_lowercase())
        });

        for folder in folders {
            if let Some(editing_id) = editing_id {
                if folder.id == editing_id {
                    continue;
                }
                if introduces_cycle(&self.folders, editing_id, folder.id.as_str()) {
                    continue;
                }
            }
            choices.push((Some(folder.id.clone()), folder.name.clone()));
        }

        choices
    }

    fn render_folder_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.folder_dialog.take() else {
            return;
        };
        let mut keep_open = true;

        match &mut dialog {
            FolderDialog::Create { name, parent_id } => {
                egui::Window::new("Create Folder")
                    .collapsible(false)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.label("Folder name");
                        ui.add(egui::TextEdit::singleline(name).desired_width(260.0));

                        let choices = self.folder_parent_choices(None);
                        let selected_label = choices
                            .iter()
                            .find(|(id, _)| id == parent_id)
                            .map(|(_, label)| label.clone())
                            .unwrap_or_else(|| "Top level".to_string());
                        egui::ComboBox::from_id_salt("create_folder_parent")
                            .selected_text(selected_label)
                            .show_ui(ui, |ui| {
                                for (id, label) in &choices {
                                    ui.selectable_value(parent_id, id.clone(), label.clone());
                                }
                            });

                        ui.horizontal(|ui| {
                            if ui.button("Create").clicked() {
                                let trimmed = name.trim();
                                if !trimmed.is_empty() {
                                    let _ = self.backend.cmd_tx.send(CoreCmd::CreateFolder {
                                        name: trimmed.to_string(),
                                        parent_id: parent_id.clone(),
                                    });
                                    keep_open = false;
                                }
                            }
                            if ui.button("Cancel").clicked() {
                                keep_open = false;
                            }
                        });
                    });
            }
            FolderDialog::Edit {
                id,
                name,
                parent_id,
            } => {
                let editing_id = id.clone();
                egui::Window::new("Edit Folder")
                    .collapsible(false)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.label("Folder name");
                        ui.add(egui::TextEdit::singleline(name).desired_width(260.0));

                        let choices = self.folder_parent_choices(Some(editing_id.as_str()));
                        let selected_label = choices
                            .iter()
                            .find(|(pid, _)| pid == parent_id)
                            .map(|(_, label)| label.clone())
                            .unwrap_or_else(|| "Top level".to_string());
                        egui::ComboBox::from_id_salt("edit_folder_parent")
                            .selected_text(selected_label)
                            .show_ui(ui, |ui| {
                                for (pid, label) in &choices {
                                    ui.selectable_value(parent_id, pid.clone(), label.clone());
                                }
                            });

                        ui.horizontal(|ui| {
                            if ui.button("Save").clicked() {
                                let trimmed = name.trim();
                                if !trimmed.is_empty() {
                                    let _ = self.backend.cmd_tx.send(CoreCmd::UpdateFolder {
                                        id: editing_id.clone(),
                                        name: trimmed.to_string(),
                                        parent_id: parent_id.clone(),
                                    });
                                    keep_open = false;
                                }
                            }
                            if ui.button("Cancel").clicked() {
                                keep_open = false;
                            }
                        });
                    });
            }
            FolderDialog::Delete { id, name } => {
                let delete_id = id.clone();
                let delete_name = name.clone();
                egui::Window::new("Delete Folder")
                    .collapsible(false)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.label(format!(
                            "Delete folder '{}' and move its pastes to Unfiled?",
                            delete_name
                        ));
                        ui.label(
                            RichText::new("Nested folders will also be deleted.")
                                .small()
                                .color(COLOR_TEXT_MUTED),
                        );
                        ui.horizontal(|ui| {
                            if ui.button("Delete").clicked() {
                                let _ = self.backend.cmd_tx.send(CoreCmd::DeleteFolder {
                                    id: delete_id.clone(),
                                });
                                if self.active_folder_id().as_deref() == Some(delete_id.as_str()) {
                                    self.set_active_collection(SidebarCollection::All);
                                }
                                keep_open = false;
                            }
                            if ui.button("Cancel").clicked() {
                                keep_open = false;
                            }
                        });
                    });
            }
        }

        if keep_open {
            self.folder_dialog = Some(dialog);
        }
    }
}
