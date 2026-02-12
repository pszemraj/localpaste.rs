//! Top bar and sidebar rendering for paste navigation and quick actions.

use super::super::*;
use eframe::egui::{self, RichText};

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
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Shortcuts (F1)").clicked() {
                            self.shortcut_help_open = true;
                        }
                    });
                });
            });
    }

    pub(crate) fn render_sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("sidebar")
            .default_width(300.0)
            .show(ctx, |ui| {
                ui.heading(
                    RichText::new(format!(
                        "Pastes ({}/{})",
                        self.pastes.len(),
                        self.all_pastes.len()
                    ))
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
                self.render_collection_filters(ui);

                ui.separator();
                ui.add_space(4.0);
                let mut pending_select: Option<String> = None;
                let row_height = ui.spacing().interact_size.y;
                egui::ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .show_rows(ui, row_height, self.pastes.len(), |ui, range| {
                        for idx in range {
                            if let Some(paste) = self.pastes.get(idx) {
                                let selected =
                                    self.selected_id.as_deref() == Some(paste.id.as_str());
                                let lang_label = display_language_label(
                                    paste.language.as_deref(),
                                    paste.content_len >= HIGHLIGHT_PLAIN_THRESHOLD,
                                );
                                let label = format!("{}  ({})", paste.name, lang_label);
                                let response = ui.selectable_label(selected, RichText::new(label));
                                if response.clicked() {
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

    fn render_collection_filters(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new("Smart filters")
                .small()
                .color(COLOR_TEXT_MUTED),
        );
        let options = [
            (SidebarCollection::All, "All"),
            (SidebarCollection::Today, "Today"),
            (SidebarCollection::Week, "This Week"),
            (SidebarCollection::Recent, "Recent (30d)"),
            (SidebarCollection::Unfiled, "Unfiled"),
            (SidebarCollection::Code, "Code"),
            (SidebarCollection::Config, "Config"),
            (SidebarCollection::Logs, "Logs"),
            (SidebarCollection::Links, "Links"),
        ];
        let mut pending_collection: Option<SidebarCollection> = None;
        ui.horizontal_wrapped(|ui| {
            for (collection, label) in options {
                let selected = self.active_collection == collection;
                if ui.selectable_label(selected, label).clicked() {
                    pending_collection = Some(collection.clone());
                }
            }
        });
        if let Some(collection) = pending_collection {
            self.set_active_collection(collection);
        }
    }
}
