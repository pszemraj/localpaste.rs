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
                                    false,
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
        const FILTERS_PER_ROW: usize = 4;
        const MAX_FILTER_ROWS: usize = 2;
        let max_visible = FILTERS_PER_ROW * MAX_FILTER_ROWS;
        let split_at = options.len().min(max_visible);
        let (visible, hidden) = options.split_at(split_at);
        let mut pending_collection: Option<SidebarCollection> = None;
        ui.scope(|ui| {
            let mut compact_style = (**ui.style()).clone();
            if let Some(button_font) = compact_style
                .text_styles
                .get(&egui::TextStyle::Button)
                .cloned()
            {
                compact_style.text_styles.insert(
                    egui::TextStyle::Button,
                    egui::FontId::new((button_font.size - 1.5).max(10.0), button_font.family),
                );
            }
            if let Some(body_font) = compact_style
                .text_styles
                .get(&egui::TextStyle::Body)
                .cloned()
            {
                compact_style.text_styles.insert(
                    egui::TextStyle::Body,
                    egui::FontId::new((body_font.size - 1.5).max(10.0), body_font.family),
                );
            }
            compact_style.spacing.button_padding = egui::vec2(8.0, 4.0);
            compact_style.spacing.item_spacing = egui::vec2(8.0, 6.0);
            compact_style.spacing.interact_size.y = 26.0;
            ui.set_style(compact_style);

            for row in visible.chunks(FILTERS_PER_ROW) {
                ui.horizontal(|ui| {
                    for (collection, label) in row {
                        let selected = self.active_collection == *collection;
                        if ui
                            .selectable_label(selected, RichText::new(*label).small())
                            .clicked()
                        {
                            pending_collection = Some(collection.clone());
                        }
                    }
                });
            }

            if !hidden.is_empty() {
                let hidden_active = hidden
                    .iter()
                    .find(|(collection, _)| self.active_collection == *collection);
                ui.horizontal(|ui| {
                    ui.menu_button(RichText::new("...").small(), |ui| {
                        for (collection, label) in hidden {
                            let selected = self.active_collection == *collection;
                            if ui.selectable_label(selected, *label).clicked() {
                                pending_collection = Some(collection.clone());
                                ui.close();
                            }
                        }
                    });
                    if let Some((_, label)) = hidden_active {
                        ui.label(RichText::new(*label).small().color(COLOR_TEXT_SECONDARY));
                    }
                });
                if hidden_active.is_some() {
                    ui.label(
                        RichText::new("Active filter is in ...")
                            .small()
                            .color(COLOR_TEXT_MUTED),
                    );
                }
            }
        });
        if let Some(collection) = pending_collection {
            self.set_active_collection(collection);
        }
    }
}
