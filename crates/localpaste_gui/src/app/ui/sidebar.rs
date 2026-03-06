//! Top bar and sidebar rendering for paste navigation and quick actions.

use super::super::*;
use eframe::egui::{self, RichText};

const APP_VERSION_LABEL: &str = concat!("- v", env!("CARGO_PKG_VERSION"));
const SIDEBAR_LANGUAGE_COLUMN_WIDTH: f32 = 84.0;

fn sidebar_hover_text(paste: &PasteSummary) -> String {
    let mut lines = vec![paste.name.clone()];
    let derived = &paste.derived;
    if derived.handle.is_some() || !derived.terms.is_empty() {
        lines.push(format!("Kind: {}", derived.kind.label()));
        if let Some(handle) = &derived.handle {
            lines.push(format!("Handle: {}", handle));
        }
        if !derived.terms.is_empty() {
            lines.push(format!("Terms: {}", derived.terms.join(", ")));
        }
    }
    lines.join("\n")
}

fn sidebar_row_text_rects(
    row_rect: egui::Rect,
    padding_x: f32,
    spacing_x: f32,
) -> (egui::Rect, egui::Rect) {
    let content_rect = row_rect.shrink2(egui::vec2(padding_x, 0.0));
    let lang_left = (content_rect.right() - SIDEBAR_LANGUAGE_COLUMN_WIDTH).max(content_rect.left());
    let title_right = (lang_left - spacing_x).max(content_rect.left());
    let title_rect = egui::Rect::from_min_max(
        content_rect.min,
        egui::pos2(title_right, content_rect.max.y),
    );
    let lang_rect =
        egui::Rect::from_min_max(egui::pos2(lang_left, content_rect.min.y), content_rect.max);
    (title_rect, lang_rect)
}

impl LocalPasteApp {
    /// Renders the top title/status bar.
    pub(crate) fn render_top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top")
            .resizable(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(RichText::new("LocalPaste.rs").color(COLOR_ACCENT_TEXT));
                    ui.label(
                        RichText::new(APP_VERSION_LABEL)
                            .small()
                            .color(COLOR_TEXT_MUTED),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Shortcuts (F1)").clicked() {
                            self.shortcut_help_open = true;
                        }
                    });
                });
            });
    }

    /// Renders the left sidebar with search, filters, and paste list.
    pub(crate) fn render_sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("sidebar")
            .default_width(300.0)
            .show(ctx, |ui| {
                let mutation_block_reason = self.mutation_shortcut_block_reason();
                let background_mutation_blocked = mutation_block_reason.is_some();
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
                    if ui
                        .add_enabled(
                            !background_mutation_blocked,
                            egui::Button::new("+ New Paste"),
                        )
                        .clicked()
                    {
                        self.create_new_paste();
                    }
                    if ui
                        .add_enabled(
                            self.selected_id.is_some() && !background_mutation_blocked,
                            egui::Button::new("Delete"),
                        )
                        .clicked()
                    {
                        self.delete_selected();
                    }
                });
                if let Some(reason) = mutation_block_reason {
                    ui.add_space(4.0);
                    ui.label(RichText::new(reason).small().color(COLOR_TEXT_MUTED));
                }

                ui.add_space(10.0);
                self.render_collection_filters(ui);
                self.render_language_filters(ui);

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
                                let row_width = ui.available_width().max(1.0);
                                let (row_rect, row_response) = ui.allocate_exact_size(
                                    egui::vec2(row_width, row_height),
                                    egui::Sense::click(),
                                );
                                let row_visuals =
                                    ui.style().interact_selectable(&row_response, selected);
                                ui.painter().rect(
                                    row_rect.expand(row_visuals.expansion),
                                    row_visuals.corner_radius,
                                    row_visuals.bg_fill,
                                    row_visuals.bg_stroke,
                                    egui::StrokeKind::Middle,
                                );

                                let (title_rect, lang_rect) = sidebar_row_text_rects(
                                    row_rect,
                                    ui.spacing().button_padding.x,
                                    ui.spacing().item_spacing.x,
                                );
                                ui.painter().with_clip_rect(title_rect).text(
                                    egui::pos2(title_rect.left(), title_rect.center().y),
                                    egui::Align2::LEFT_CENTER,
                                    paste.name.as_str(),
                                    egui::TextStyle::Button.resolve(ui.style()),
                                    row_visuals.text_color(),
                                );
                                ui.painter().with_clip_rect(lang_rect).text(
                                    egui::pos2(lang_rect.right(), lang_rect.center().y),
                                    egui::Align2::RIGHT_CENTER,
                                    lang_label.as_str(),
                                    egui::TextStyle::Small.resolve(ui.style()),
                                    COLOR_TEXT_MUTED,
                                );

                                if row_response
                                    .on_hover_text(sidebar_hover_text(paste))
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

    fn render_language_filters(&mut self, ui: &mut egui::Ui) {
        let language_options = self.language_filter_options();
        if language_options.is_empty() {
            return;
        }
        ui.add_space(8.0);
        ui.label(
            RichText::new("Language filter")
                .small()
                .color(COLOR_TEXT_MUTED),
        );

        let mut selected_language = self.active_language_filter.clone();
        let selected_text = selected_language
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("All languages")
            .to_string();
        egui::ComboBox::from_id_salt("sidebar_language_filter")
            .selected_text(selected_text)
            .width(180.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut selected_language, None, "All languages");
                for lang in &language_options {
                    ui.selectable_value(&mut selected_language, Some(lang.clone()), lang.as_str());
                }
            });
        if selected_language != self.active_language_filter {
            self.set_active_language_filter(selected_language);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{sidebar_hover_text, sidebar_row_text_rects};
    use eframe::egui;

    #[test]
    fn sidebar_row_text_layout_matrix() {
        let cases = [
            (
                egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(300.0, 28.0)),
                18.0,
                Some(302.0),
            ),
            (
                egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(80.0, 28.0)),
                8.0,
                Some(72.0),
            ),
        ];

        for (row_rect, expected_title_left, expected_lang_right) in cases {
            let (title_rect, lang_rect) = sidebar_row_text_rects(row_rect, 8.0, 6.0);

            assert!((title_rect.left() - expected_title_left).abs() < f32::EPSILON);
            assert!(title_rect.width() >= 0.0);
            assert!(lang_rect.left() >= title_rect.left());
            assert!(title_rect.right() <= lang_rect.left());
            assert!(lang_rect.width() > 0.0);
            if let Some(expected_lang_right) = expected_lang_right {
                assert!((lang_rect.right() - expected_lang_right).abs() < f32::EPSILON);
            }
        }
    }

    #[test]
    fn sidebar_hover_text_includes_derived_retrieval_hints_when_present() {
        let summary = crate::backend::PasteSummary {
            id: "id".to_string(),
            name: "untamed-tundra".to_string(),
            language: Some("rust".to_string()),
            content_len: 10,
            updated_at: chrono::Utc::now(),
            folder_id: None,
            tags: Vec::new(),
            derived: localpaste_core::semantic::DerivedMeta {
                kind: localpaste_core::semantic::PasteKind::Code,
                handle: Some("fn handle_request".to_string()),
                terms: vec!["fsdp2".to_string(), "cublaslt".to_string()],
            },
        };
        let tooltip = sidebar_hover_text(&summary);
        assert!(tooltip.contains("untamed-tundra"));
        assert!(tooltip.contains("Kind: Code"));
        assert!(tooltip.contains("Handle: fn handle_request"));
        assert!(tooltip.contains("Terms: fsdp2, cublaslt"));
    }
}
