//! Top bar and sidebar rendering for paste navigation and quick actions.

use super::super::*;
use eframe::egui::{self, RichText};
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
                    if let Some(lang) = paste.language.as_ref().map(|v| v.trim()).filter(|v| !v.is_empty()) {
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
}
