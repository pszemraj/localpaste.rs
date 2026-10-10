//! Top bar and sidebar rendering for paste navigation and quick actions.

use super::super::shortcuts::runtime_shortcut_label;
use super::super::*;
use chrono::{DateTime, Duration as ChronoDuration, Local, TimeZone, Utc};
use eframe::egui::{self, RichText};

const APP_VERSION_LABEL: &str = concat!("- v", env!("CARGO_PKG_VERSION"));
const SIDEBAR_LANGUAGE_COLUMN_WIDTH: f32 = 84.0;
const SIDEBAR_ROWS_SCROLL_SALT: &str = "sidebar_rows";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SidebarTimeBucket {
    Today,
    Yesterday,
    ThisWeek,
    Earlier,
}

impl SidebarTimeBucket {
    fn label(self) -> &'static str {
        match self {
            Self::Today => "Today",
            Self::Yesterday => "Yesterday",
            Self::ThisWeek => "This Week",
            Self::Earlier => "Earlier",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SidebarListItem {
    Header(SidebarTimeBucket),
    Row(usize),
}

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

fn sidebar_time_bucket<Tz: TimeZone>(
    updated_at: DateTime<Utc>,
    now: DateTime<Tz>,
) -> SidebarTimeBucket {
    let timezone = now.timezone();
    let updated_day = updated_at.with_timezone(&timezone).date_naive();
    let today = now.date_naive();
    if updated_day == today {
        return SidebarTimeBucket::Today;
    }
    if updated_day == today - ChronoDuration::days(1) {
        return SidebarTimeBucket::Yesterday;
    }
    if updated_day >= today - ChronoDuration::days(7) {
        SidebarTimeBucket::ThisWeek
    } else {
        SidebarTimeBucket::Earlier
    }
}

fn build_sidebar_list_items(
    pastes: &[PasteSummary],
    search_query: &str,
    now: DateTime<Local>,
) -> Vec<SidebarListItem> {
    if !search_query.trim().is_empty() {
        return (0..pastes.len()).map(SidebarListItem::Row).collect();
    }

    let mut items = Vec::with_capacity(pastes.len().saturating_add(4));
    let mut current_bucket = None;
    for (idx, paste) in pastes.iter().enumerate() {
        let bucket = sidebar_time_bucket(paste.updated_at, now);
        if current_bucket != Some(bucket) {
            items.push(SidebarListItem::Header(bucket));
            current_bucket = Some(bucket);
        }
        items.push(SidebarListItem::Row(idx));
    }
    items
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
                    if ui
                        .add_enabled(
                            !background_mutation_blocked,
                            egui::Button::new("+ New Paste").sense(non_focusable_click_sense()),
                        )
                        .clicked()
                    {
                        self.create_new_paste();
                    }
                    if ui
                        .add_enabled(
                            self.selected_id.is_some() && !background_mutation_blocked,
                            egui::Button::new("Delete").sense(non_focusable_click_sense()),
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

                ui.separator();
                ui.add_space(4.0);
                let mut pending_select: Option<String> = None;
                let selection_blocked = self.selection_transition_block_reason().is_some();
                let row_height = ui.spacing().interact_size.y;
                let sidebar_items = build_sidebar_list_items(
                    &self.pastes,
                    self.search_query.as_str(),
                    Local::now(),
                );
                let mut rows_scroll = egui::ScrollArea::vertical()
                    .id_salt(SIDEBAR_ROWS_SCROLL_SALT)
                    .auto_shrink([false; 2]);
                if let Some(offset) = self.sidebar_reveal_offset(ui, &sidebar_items, row_height) {
                    rows_scroll = rows_scroll.vertical_scroll_offset(offset);
                }
                rows_scroll.show_rows(ui, row_height, sidebar_items.len(), |ui, range| {
                    for item_idx in range {
                        match sidebar_items.get(item_idx).copied() {
                            Some(SidebarListItem::Header(bucket)) => {
                                let row_width = ui.available_width().max(1.0);
                                let (row_rect, _) = ui.allocate_exact_size(
                                    egui::vec2(row_width, row_height),
                                    egui::Sense::hover(),
                                );
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
                                if let Some(paste) = self.pastes.get(idx) {
                                    let selected =
                                        self.selected_id.as_deref() == Some(paste.id.as_str());
                                    let lang_label = paste_list_language_label(paste);
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
                                        if selection_blocked {
                                            self.set_selection_transition_blocked_status();
                                        } else {
                                            pending_select = Some(paste.id.clone());
                                        }
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

    /// Returns the list offset that brings a newly selected row into view.
    ///
    /// Fires once per selection change, as soon as the selected row is in the
    /// list, and only when that row is outside the viewport, so manual
    /// scrolling is left alone until the selection changes again. A row
    /// directly under a date header reveals the header with it.
    ///
    /// # Arguments
    /// - `ui`: Sidebar UI about to add the rows scroll area.
    /// - `items`: Rendered list items, headers included.
    /// - `row_height`: Height of one list item, without spacing.
    ///
    /// # Returns
    /// The vertical offset to apply this frame, or `None` to keep the current one.
    fn sidebar_reveal_offset(
        &mut self,
        ui: &egui::Ui,
        items: &[SidebarListItem],
        row_height: f32,
    ) -> Option<f32> {
        if self.sidebar_revealed_id == self.selected_id {
            return None;
        }
        let Some(selected) = self.selected_id.as_deref() else {
            self.sidebar_revealed_id = None;
            return None;
        };
        let row = items.iter().position(|item| {
            matches!(item, SidebarListItem::Row(idx)
                if self.pastes.get(*idx).is_some_and(|paste| paste.id == selected))
        })?;
        self.sidebar_revealed_id = self.selected_id.clone();

        let first = match row.checked_sub(1).map(|prev| items[prev]) {
            Some(SidebarListItem::Header(_)) => row - 1,
            _ => row,
        };
        let stride = row_height + ui.spacing().item_spacing.y;
        let top = first as f32 * stride;
        let bottom = row as f32 * stride + row_height;
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
            // Compact chips so each group fits one row at the default sidebar width.
            let spacing = ui.spacing_mut();
            spacing.button_padding = egui::vec2(4.0, 3.0);
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
    use super::{
        build_sidebar_list_items, sidebar_hover_text, sidebar_row_text_rects, sidebar_time_bucket,
        SidebarListItem, SidebarTimeBucket,
    };
    use chrono::{Duration, FixedOffset, Local, TimeZone, Utc};
    use eframe::egui;

    fn summary(id: &str, updated_at: chrono::DateTime<Utc>) -> crate::backend::PasteSummary {
        crate::backend::PasteSummary {
            id: id.to_string(),
            name: id.to_string(),
            language: None,
            content_len: 0,
            updated_at,
            folder_id: None,
            tags: Vec::new(),
            derived: localpaste_core::semantic::DerivedMeta::default(),
            match_excerpt: None,
        }
    }

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
    fn sidebar_time_bucket_classifies_recency_boundaries() {
        let now = Local.with_ymd_and_hms(2026, 6, 15, 12, 0, 0).unwrap();
        assert_eq!(
            sidebar_time_bucket((now - Duration::hours(2)).with_timezone(&Utc), now),
            SidebarTimeBucket::Today
        );
        assert_eq!(
            sidebar_time_bucket((now - Duration::days(1)).with_timezone(&Utc), now),
            SidebarTimeBucket::Yesterday
        );
        assert_eq!(
            sidebar_time_bucket((now - Duration::days(3)).with_timezone(&Utc), now),
            SidebarTimeBucket::ThisWeek
        );
        assert_eq!(
            sidebar_time_bucket((now - Duration::days(9)).with_timezone(&Utc), now),
            SidebarTimeBucket::Earlier
        );
    }

    #[test]
    fn sidebar_time_bucket_uses_local_calendar_boundaries() {
        let east = FixedOffset::east_opt(14 * 60 * 60).unwrap();
        let west = FixedOffset::west_opt(10 * 60 * 60).unwrap();
        let this_week_now = east.with_ymd_and_hms(2026, 6, 15, 12, 0, 0).unwrap();
        let yesterday_now = west.with_ymd_and_hms(2026, 6, 15, 0, 30, 0).unwrap();
        let cases = [
            (
                east.with_ymd_and_hms(2026, 6, 8, 0, 1, 0).unwrap(),
                this_week_now,
                SidebarTimeBucket::ThisWeek,
            ),
            (
                east.with_ymd_and_hms(2026, 6, 7, 23, 59, 59).unwrap(),
                this_week_now,
                SidebarTimeBucket::Earlier,
            ),
            (
                west.with_ymd_and_hms(2026, 6, 14, 23, 59, 0).unwrap(),
                yesterday_now,
                SidebarTimeBucket::Yesterday,
            ),
        ];

        for (updated_at, now, expected) in cases {
            assert_eq!(
                sidebar_time_bucket(updated_at.with_timezone(&Utc), now),
                expected
            );
        }
    }

    #[test]
    fn sidebar_list_items_matrix_covers_headers_and_search_density() {
        let now = Local.with_ymd_and_hms(2026, 6, 15, 12, 0, 0).unwrap();
        let pastes = vec![
            summary("today", (now - Duration::hours(1)).with_timezone(&Utc)),
            summary("yesterday", (now - Duration::days(1)).with_timezone(&Utc)),
            summary("week", (now - Duration::days(3)).with_timezone(&Utc)),
            summary("earlier", (now - Duration::days(9)).with_timezone(&Utc)),
        ];

        assert_eq!(
            build_sidebar_list_items(&pastes, "", now),
            vec![
                SidebarListItem::Header(SidebarTimeBucket::Today),
                SidebarListItem::Row(0),
                SidebarListItem::Header(SidebarTimeBucket::Yesterday),
                SidebarListItem::Row(1),
                SidebarListItem::Header(SidebarTimeBucket::ThisWeek),
                SidebarListItem::Row(2),
                SidebarListItem::Header(SidebarTimeBucket::Earlier),
                SidebarListItem::Row(3),
            ]
        );
        assert_eq!(
            build_sidebar_list_items(&pastes, "needle", now),
            vec![
                SidebarListItem::Row(0),
                SidebarListItem::Row(1),
                SidebarListItem::Row(2),
                SidebarListItem::Row(3),
            ]
        );
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
