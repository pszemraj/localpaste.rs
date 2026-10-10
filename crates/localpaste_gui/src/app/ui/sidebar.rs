//! Top bar and sidebar rendering for paste navigation and quick actions.

use super::super::shortcuts::runtime_shortcut_label;
use super::super::*;
use chrono::Local;
use eframe::egui::{self, RichText};
use list::{SidebarList, SidebarListItem, SidebarListMetrics};

mod clusters;
mod list;

const APP_VERSION_LABEL: &str = concat!("- v", env!("CARGO_PKG_VERSION"));
const SIDEBAR_LANGUAGE_COLUMN_WIDTH: f32 = 84.0;
const SIDEBAR_ROWS_SCROLL_SALT: &str = "sidebar_rows";

/// Returns the id under which the sidebar rows scroll area stores its state.
///
/// `ScrollArea::id_salt` wraps its argument in `Id::new` before deriving the
/// persistent id, so a lookup must wrap the salt the same way. Hashing the raw
/// salt yields a different id, and the stored scroll offset is never found.
///
/// # Arguments
/// - `ui`: Sidebar UI the rows scroll area is added to.
fn sidebar_rows_scroll_id(ui: &egui::Ui) -> egui::Id {
    ui.make_persistent_id(egui::Id::new(SIDEBAR_ROWS_SCROLL_SALT))
}

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

/// Splits a sidebar row into its title and language-label areas.
///
/// The language label is right-aligned and only as wide as its text, up to
/// [`SIDEBAR_LANGUAGE_COLUMN_WIDTH`], so the title gets the rest of the row
/// up to `spacing_x` before the label.
///
/// # Arguments
/// - `row_rect`: The whole row.
/// - `padding_x`: Horizontal inset from each row edge.
/// - `spacing_x`: Gap kept between the title and the language label.
/// - `lang_width`: Width of the laid-out language label.
///
/// # Returns
/// The title area and the language-label area.
fn sidebar_row_text_rects(
    row_rect: egui::Rect,
    padding_x: f32,
    spacing_x: f32,
    lang_width: f32,
) -> (egui::Rect, egui::Rect) {
    let content_rect = row_rect.shrink2(egui::vec2(padding_x, 0.0));
    let lang_width = lang_width.clamp(0.0, SIDEBAR_LANGUAGE_COLUMN_WIDTH);
    let lang_left = (content_rect.right() - lang_width).max(content_rect.left());
    let title_right = (lang_left - spacing_x).max(content_rect.left());
    let title_rect = egui::Rect::from_min_max(
        content_rect.min,
        egui::pos2(title_right, content_rect.max.y),
    );
    let lang_rect =
        egui::Rect::from_min_max(egui::pos2(lang_left, content_rect.min.y), content_rect.max);
    (title_rect, lang_rect)
}

/// Lays out one line of sidebar row text, ending in an ellipsis when it is wider than `max_width`.
///
/// # Arguments
/// - `ui`: Row UI whose style resolves `style`.
/// - `text`: Text to lay out.
/// - `style`: Text style of the line.
/// - `color`: Text color.
/// - `max_width`: Widest the line may be.
///
/// # Returns
/// The single-row galley, elided when the text did not fit.
fn sidebar_row_galley(
    ui: &egui::Ui,
    text: &str,
    style: egui::TextStyle,
    color: egui::Color32,
    max_width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::single_section(
        text.to_owned(),
        egui::TextFormat::simple(style.resolve(ui.style()), color),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_width.max(0.0));
    ui.fonts_mut(|fonts| fonts.layout_job(job))
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
                        // Background chrome should not enter egui's keyboard focus ring while the
                        // editor owns arrow/home/end navigation on muscle memory.
                        let shortcuts = ui.add(
                            egui::Button::new("Shortcuts (F1)")
                                .small()
                                .sense(non_focusable_click_sense()),
                        );
                        // Capture the opener before the editor blurs on this press.
                        if shortcuts.is_pointer_button_down_on() {
                            self.remember_discovery_focus(ctx);
                        }
                        if shortcuts.clicked() {
                            self.open_shortcut_help(ctx);
                        } else if ui.input(|input| input.pointer.primary_released())
                            && !self.command_palette_open
                            && !self.paste_picker_open
                            && !self.shortcut_help_open
                        {
                            self.discovery_return_focus = None;
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
                        .id(egui::Id::new(SEARCH_INPUT_ID))
                        .muted_hint(format!(
                            "Search pastes... ({})",
                            runtime_shortcut_label(RuntimeShortcutAction::FocusSearch)
                        )),
                );
                if self.search_focus_requested {
                    search_resp.request_focus();
                    self.search_focus_requested = false;
                }
                if search_resp.changed() {
                    self.set_search_query(search_buf);
                }
                ui.horizontal(|ui| {
                    // Scope and language share one row, each half the sidebar width.
                    let combo_width =
                        ((ui.available_width() - ui.spacing().item_spacing.x) * 0.5).max(0.0);
                    let scope = super::search_scope::scope_selector(
                        ui,
                        "sidebar_scope",
                        self.search_scope,
                        combo_width,
                    );
                    self.set_search_scope(scope);
                    self.render_language_filter(ui, combo_width);
                });
                if let Some(error) = &self.search_error {
                    ui.label(RichText::new(error).small().color(COLOR_TEXT_MUTED));
                    if ui.button("Retry search").clicked() {
                        self.search_error = None;
                        self.search_last_sent.clear();
                        self.search_last_input_at = Some(Instant::now() - SEARCH_DEBOUNCE);
                    }
                }

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    // The block reason is hover text, so it cannot push the list down.
                    let explain_block = |response: egui::Response| match mutation_block_reason {
                        Some(reason) => response.on_disabled_hover_text(reason),
                        None => response,
                    };
                    if explain_block(ui.add_enabled(
                        !background_mutation_blocked,
                        egui::Button::new("+ New Paste").sense(non_focusable_click_sense()),
                    ))
                    .clicked()
                    {
                        self.create_new_paste();
                    }
                    if explain_block(ui.add_enabled(
                        self.selected_id.is_some() && !background_mutation_blocked,
                        egui::Button::new("Delete").sense(non_focusable_click_sense()),
                    ))
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
                let selection_blocked = self.selection_transition_block_reason().is_some();
                let metrics = SidebarListMetrics::from_spacing(ui.spacing());
                let sidebar_list = SidebarList::build(
                    &self.pastes,
                    self.search_query.as_str(),
                    Local::now(),
                    self.sidebar_rows_built_at,
                );
                let mut rows_scroll = egui::ScrollArea::vertical()
                    .id_salt(SIDEBAR_ROWS_SCROLL_SALT)
                    .auto_shrink([false; 2]);
                if let Some(offset) = self.sidebar_reveal_offset(ui, &sidebar_list, metrics) {
                    rows_scroll = rows_scroll.vertical_scroll_offset(offset);
                }
                rows_scroll.show_viewport(ui, |ui, viewport| {
                    ui.set_height(sidebar_list.content_height(metrics));
                    // Rows rest on the tertiary surface, so hover lifts them to the border tone.
                    super::soften_list_row_visuals(ui.visuals_mut(), COLOR_BORDER);
                    let range = sidebar_list.visible_range(viewport.y_range(), metrics);
                    // Item `i` keeps the same widget id however far the list is scrolled.
                    ui.skip_ahead_auto_ids(range.start);
                    let origin = ui.max_rect().min;
                    let item_size = egui::vec2(ui.max_rect().width().max(1.0), metrics.item_height);
                    for item_idx in range {
                        let item_rect = egui::Rect::from_min_size(
                            origin + egui::vec2(0.0, sidebar_list.item_top(item_idx, metrics)),
                            item_size,
                        );
                        match sidebar_list.items().get(item_idx).copied() {
                            Some(SidebarListItem::Header(bucket)) => {
                                let row_rect =
                                    ui.allocate_rect(item_rect, egui::Sense::hover()).rect;
                                let text_pos = egui::pos2(row_rect.left(), row_rect.center().y);
                                ui.painter().text(
                                    text_pos,
                                    egui::Align2::LEFT_CENTER,
                                    bucket.label(),
                                    egui::TextStyle::Small.resolve(ui.style()),
                                    COLOR_TEXT_MUTED,
                                );
                            }
                            Some(SidebarListItem::Row(idx)) => {
                                let Some(paste) = self.pastes.get(idx) else {
                                    continue;
                                };
                                let selected =
                                    self.selected_id.as_deref() == Some(paste.id.as_str());
                                let lang_label = paste_list_language_label(paste);
                                let row_response =
                                    ui.allocate_rect(item_rect, egui::Sense::click());
                                let row_rect = row_response.rect;
                                let row_visuals =
                                    ui.style().interact_selectable(&row_response, selected);
                                ui.painter().rect(
                                    row_rect.expand(row_visuals.expansion),
                                    row_visuals.corner_radius,
                                    row_visuals.bg_fill,
                                    row_visuals.bg_stroke,
                                    egui::StrokeKind::Middle,
                                );

                                let lang_galley = sidebar_row_galley(
                                    ui,
                                    lang_label.as_str(),
                                    egui::TextStyle::Small,
                                    COLOR_TEXT_MUTED,
                                    SIDEBAR_LANGUAGE_COLUMN_WIDTH,
                                );
                                let (title_rect, lang_rect) = sidebar_row_text_rects(
                                    row_rect,
                                    ui.spacing().button_padding.x,
                                    ui.spacing().item_spacing.x,
                                    lang_galley.size().x,
                                );
                                let title_galley = sidebar_row_galley(
                                    ui,
                                    paste.name.as_str(),
                                    egui::TextStyle::Button,
                                    row_visuals.text_color(),
                                    title_rect.width(),
                                );
                                let title_pos = egui::Align2::LEFT_CENTER
                                    .anchor_size(title_rect.left_center(), title_galley.size())
                                    .min;
                                ui.painter().with_clip_rect(title_rect).galley(
                                    title_pos,
                                    title_galley,
                                    row_visuals.text_color(),
                                );
                                let lang_pos = egui::Align2::RIGHT_CENTER
                                    .anchor_size(lang_rect.right_center(), lang_galley.size())
                                    .min;
                                ui.painter().with_clip_rect(lang_rect).galley(
                                    lang_pos,
                                    lang_galley,
                                    COLOR_TEXT_MUTED,
                                );

                                if row_response
                                    .on_hover_text(sidebar_hover_text(paste))
                                    .clicked()
                                {
                                    if selection_blocked {
                                        self.set_selection_transition_blocked_status();
                                    } else {
                                        pending_select = Some(paste.id.clone());
                                    }
                                }
                            }
                            None => {}
                        }
                    }
                });
                if let Some(id) = pending_select {
                    self.select_paste(id);
                }
            });
    }

    /// Returns the list offset that brings the selected row into view.
    ///
    /// Fires once per selection change, as soon as the selected row is in the
    /// list, and once per change of the search or filters that produced the
    /// rows (`sidebar_rows_filter`), on the first frame that shows the new
    /// rows. Rebuilds under unchanged filters, such as background refreshes,
    /// saves, and renames, do not fire it. It scrolls only when the selected
    /// row is outside the viewport, so manual scrolling is left alone until
    /// the selection or the filters change again. A row directly under a date
    /// header reveals the header with it; a cluster gap above the row stays
    /// out of the revealed span.
    ///
    /// # Arguments
    /// - `ui`: Sidebar UI about to add the rows scroll area.
    /// - `list`: Rendered list, headers and cluster gaps included.
    /// - `metrics`: Sizes the list is laid out with.
    ///
    /// # Returns
    /// The vertical offset to apply this frame, or `None` to keep the current one.
    fn sidebar_reveal_offset(
        &mut self,
        ui: &egui::Ui,
        list: &SidebarList,
        metrics: SidebarListMetrics,
    ) -> Option<f32> {
        if self.sidebar_revealed_filter != self.sidebar_rows_filter {
            self.sidebar_revealed_filter = self.sidebar_rows_filter.clone();
        } else if self.sidebar_revealed_id == self.selected_id {
            return None;
        }
        let Some(selected) = self.selected_id.as_deref() else {
            self.sidebar_revealed_id = None;
            return None;
        };
        let items = list.items();
        let row = items.iter().position(|item| {
            matches!(item, SidebarListItem::Row(idx)
                if self.pastes.get(*idx).is_some_and(|paste| paste.id == selected))
        })?;
        self.sidebar_revealed_id = self.selected_id.clone();

        let first = match row.checked_sub(1).map(|prev| items[prev]) {
            Some(SidebarListItem::Header(_)) => row - 1,
            _ => row,
        };
        let top = list.item_top(first, metrics);
        let bottom = list.item_top(row, metrics) + metrics.item_height;
        let viewport = ui.available_rect_before_wrap().height();
        let offset = egui::scroll_area::State::load(ui.ctx(), sidebar_rows_scroll_id(ui))
            .map_or(0.0, |state| state.offset.y);
        if top < offset {
            Some(top)
        } else if bottom > offset + viewport {
            Some(bottom - viewport)
        } else {
            None
        }
    }

    fn render_collection_filters(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new("Smart filters")
                .small()
                .color(COLOR_TEXT_MUTED),
        );
        // Time windows on the first row, content kinds on the second.
        let rows: [&[(SidebarCollection, &str)]; 2] = [
            &[
                (SidebarCollection::All, "All"),
                (SidebarCollection::Today, "Today"),
                (SidebarCollection::Week, "This Week"),
                (SidebarCollection::Recent, "Recent (30d)"),
            ],
            &[
                (SidebarCollection::Unfiled, "Unfiled"),
                (SidebarCollection::Documents, "Documents"),
                (SidebarCollection::Code, "Code"),
                (SidebarCollection::Config, "Config"),
                (SidebarCollection::Logs, "Logs"),
                (SidebarCollection::Links, "Links"),
            ],
        ];
        let mut pending_collection: Option<SidebarCollection> = None;
        ui.scope(|ui| {
            // Compact chips so each group fits one row at the default sidebar width,
            // with margin left for text that snaps wider at fractional display scales.
            let spacing = ui.spacing_mut();
            spacing.button_padding = egui::vec2(3.0, 3.0);
            spacing.item_spacing = egui::vec2(4.0, 6.0);
            spacing.interact_size.y = 24.0;
            for row in rows {
                ui.horizontal_wrapped(|ui| {
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
        });
        if let Some(collection) = pending_collection {
            self.set_active_collection(collection);
        }
    }

    /// Renders the language filter combo beside the search scope, when any paste has a language.
    ///
    /// # Arguments
    /// - `ui`: Horizontal row shared with the search scope combo.
    /// - `width`: Outer combo width; long language names truncate to the row's remaining width.
    fn render_language_filter(&mut self, ui: &mut egui::Ui, width: f32) {
        let language_options = self.language_filter_options();
        if language_options.is_empty() {
            return;
        }

        let mut selected_language = self.active_language_filter.clone();
        let selected_text = selected_language
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("All languages")
            .to_string();
        egui::ComboBox::from_id_salt("sidebar_language_filter")
            .selected_text(selected_text)
            .width(width)
            .truncate()
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut selected_language, None, "All languages");
                for lang in &language_options {
                    ui.selectable_value(&mut selected_language, Some(lang.clone()), lang.as_str());
                }
            })
            .response
            .on_hover_text("Filter by language");
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
        let wide = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(300.0, 28.0));
        let narrow = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(80.0, 28.0));
        // (row, language label width, title left, title right, label left, label right)
        let cases = [
            // The title runs up to the spacing before a short label.
            (wide, 30.0, 18.0, 266.0, 272.0, 302.0),
            // A label wider than the language column is held to the column.
            (wide, 200.0, 18.0, 212.0, 218.0, 302.0),
            (narrow, 30.0, 8.0, 36.0, 42.0, 72.0),
            (narrow, 200.0, 8.0, 8.0, 8.0, 72.0),
        ];

        for (row_rect, lang_width, title_left, title_right, lang_left, lang_right) in cases {
            let (title_rect, lang_rect) = sidebar_row_text_rects(row_rect, 8.0, 6.0, lang_width);

            assert!((title_rect.left() - title_left).abs() < f32::EPSILON);
            assert!((title_rect.right() - title_right).abs() < f32::EPSILON);
            assert!(title_rect.width() >= 0.0);
            assert!(lang_rect.left() >= title_rect.left());
            assert!(title_rect.right() <= lang_rect.left());
            assert!((lang_rect.left() - lang_left).abs() < f32::EPSILON);
            assert!(lang_rect.width() > 0.0);
            assert!((lang_rect.right() - lang_right).abs() < f32::EPSILON);
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
            match_excerpt: None,
        };
        let tooltip = sidebar_hover_text(&summary);
        assert!(tooltip.contains("untamed-tundra"));
        assert!(tooltip.contains("Kind: Code"));
        assert!(tooltip.contains("Handle: fn handle_request"));
        assert!(tooltip.contains("Terms: fsdp2, cublaslt"));
    }
}
