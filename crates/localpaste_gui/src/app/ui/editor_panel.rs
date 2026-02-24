//! Central editor panel rendering for virtual preview and virtual editor modes.

use super::super::*;
use super::properties_drawer::{
    apply_language_choice, auto_language_choice_key, auto_language_status_label,
    selected_language_choice_text,
};
use eframe::egui;

impl LocalPasteApp {
    /// Renders the primary editor panel, including metadata toolbar and mode UI.
    ///
    /// # Panics
    /// Panics if a virtual-editor highlight row fails internal consistency checks.
    pub(crate) fn render_editor_panel(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            let selected_meta = self.selected_paste.as_ref().map(|paste| paste.id.clone());

            if let Some(id) = selected_meta {
                let language = self.edit_language.clone();
                let is_large = self.active_text_len_bytes() >= HIGHLIGHT_PLAIN_THRESHOLD;
                let visible_tags = compact_header_tags(self.edit_tags.as_str());

                let mut pending_tag_search: Option<String> = None;
                let mut apply_metadata = false;
                let mut copy_requested = false;
                let mut copy_link_requested = false;
                let mut duplicate_requested = false;
                let mut export_requested = false;
                let mut open_properties = false;
                let mut delete_requested = false;
                ui.scope(|ui| {
                    apply_compact_meta_row_style(ui);
                    ui.horizontal_wrapped(|ui| {
                        let title_width = (ui.available_width() * 0.32).clamp(180.0, 380.0);
                        let name_response = ui.add(
                            egui::TextEdit::singleline(&mut self.edit_name)
                                .font(egui::TextStyle::Button)
                                .desired_width(title_width)
                                .hint_text("Untitled paste"),
                        );
                        if name_response.changed() {
                            self.metadata_dirty = true;
                        }
                        if name_response.lost_focus() && self.metadata_dirty {
                            apply_metadata = true;
                        }
                        if name_response.has_focus()
                            && ui.input(|input| input.key_pressed(egui::Key::Enter))
                        {
                            apply_metadata = true;
                        }
                        if name_response.has_focus()
                            && ui.input(|input| input.key_pressed(egui::Key::Escape))
                        {
                            if let Some(paste) = self.selected_paste.as_ref() {
                                self.edit_name = paste.name.clone();
                                self.metadata_dirty = self.edit_name != paste.name
                                    || self.edit_language != paste.language
                                    || self.edit_language_is_manual != paste.language_is_manual
                                    || self.edit_tags != paste.tags.join(", ");
                            }
                        }

                        let current_manual_value = self
                            .edit_language
                            .as_deref()
                            .map(localpaste_core::detection::canonical::canonicalize)
                            .filter(|value| !value.is_empty())
                            .unwrap_or_else(|| "text".to_string());
                        let mut language_choice = if self.edit_language_is_manual {
                            current_manual_value.clone()
                        } else {
                            auto_language_choice_key().to_string()
                        };
                        let previous_language_choice = language_choice.clone();
                        let auto_label = auto_language_status_label();
                        let selected_language_text = selected_language_choice_text(
                            language_choice.as_str(),
                            auto_label.as_str(),
                        );
                        egui::ComboBox::from_id_salt("header_language_select")
                            .selected_text(selected_language_text)
                            .width(160.0)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut language_choice,
                                    auto_language_choice_key().to_string(),
                                    "Auto",
                                );
                                for option in
                                    localpaste_core::detection::canonical::MANUAL_LANGUAGE_OPTIONS
                                {
                                    ui.selectable_value(
                                        &mut language_choice,
                                        option.value.to_string(),
                                        option.label,
                                    );
                                }
                            });
                        if language_choice != previous_language_choice {
                            apply_language_choice(
                                &mut self.edit_language_is_manual,
                                &mut self.edit_language,
                                &mut self.metadata_dirty,
                                language_choice.as_str(),
                                current_manual_value.as_str(),
                            );
                        }
                        for tag in &visible_tags {
                            if ui.small_button(format!("#{}", tag)).clicked() {
                                pending_tag_search = Some(tag.clone());
                            }
                        }
                        ui.separator();
                        if ui
                            .add_enabled(
                                self.metadata_dirty && !self.metadata_save_in_flight,
                                egui::Button::new("Apply"),
                            )
                            .clicked()
                        {
                            apply_metadata = true;
                        }
                        if ui.small_button("Copy").clicked() {
                            copy_requested = true;
                        }
                        if ui.small_button("Copy Link").clicked() {
                            copy_link_requested = true;
                        }
                        if ui.small_button("Duplicate").clicked() {
                            duplicate_requested = true;
                        }
                        if ui.small_button("Export").clicked() {
                            export_requested = true;
                        }
                        if ui.small_button("Properties").clicked() {
                            open_properties = true;
                        }
                        if ui.small_button("Delete").clicked() {
                            delete_requested = true;
                        }
                    });
                });
                if let Some(tag) = pending_tag_search {
                    self.set_search_query(tag);
                }
                if apply_metadata {
                    self.save_metadata_now();
                }
                if copy_requested {
                    self.clipboard_outgoing = Some(self.active_snapshot());
                    self.set_status("Copied paste content.");
                }
                if copy_link_requested {
                    self.clipboard_outgoing =
                        Some(super::super::util::api_paste_link_for_copy(self.server_addr, &id));
                    self.set_status("Copied API paste link.");
                }
                if duplicate_requested {
                    self.create_new_paste_with_content(self.active_snapshot());
                    self.set_status("Duplicated paste into a new draft.");
                }
                if export_requested {
                    self.export_selected_paste();
                }
                if open_properties {
                    self.properties_drawer_open = true;
                }
                if delete_requested {
                    self.delete_selected();
                }

                ui.label(
                    RichText::new(id.clone())
                        .small()
                        .monospace()
                        .color(COLOR_TEXT_MUTED),
                );
                ui.add_space(6.0);
                let editor_height = ui.available_height();
                let editor_style = TextStyle::Name(EDITOR_TEXT_STYLE.into());
                let editor_font = ui
                    .style()
                    .text_styles
                    .get(&editor_style)
                    .cloned()
                    .unwrap_or_else(|| TextStyle::Monospace.resolve(ui.style()));
                let language_hint = syntect_language_hint(language.as_deref().unwrap_or("text"));
                let theme = (!is_large).then(|| CodeTheme::from_memory(ui.ctx(), ui.style()));
                let theme_key = theme
                    .as_ref()
                    .map(syntect_theme_key)
                    .unwrap_or("base16-mocha.dark");
                let revision = self.active_revision();
                let text_len = self.active_text_len_bytes();
                if is_large
                    && (self.highlight_pending.is_some()
                        || self.highlight_render.is_some()
                        || self.highlight_staged.is_some())
                {
                    // Crossing into plain-threshold mode should drop any stale
                    // staged/current highlight state so large buffers stay plain.
                    self.clear_highlight_state();
                }
                let use_virtual_preview = self.editor_mode == EditorMode::VirtualPreview;
                let use_virtual_editor = self.editor_mode == EditorMode::VirtualEditor;
                let needs_worker_render = use_virtual_preview || use_virtual_editor;
                let async_mode =
                    !is_large && (text_len >= HIGHLIGHT_DEBOUNCE_MIN_BYTES || needs_worker_render);
                let debounce_window = self.highlight_debounce_window(text_len, async_mode);
                let debounce_active = self
                    .last_edit_at
                    .map(|last| last.elapsed() < debounce_window)
                    .unwrap_or(false);
                let should_request = async_mode
                    && self.should_request_highlight(
                        &language_hint,
                        theme_key,
                        debounce_active,
                        id.as_str(),
                    );
                if should_request {
                    let request_text = if self.is_virtual_editor_mode() {
                        HighlightRequestText::Rope(self.virtual_editor_buffer.rope().clone())
                    } else {
                        HighlightRequestText::Owned(self.selected_content.to_string())
                    };
                    self.dispatch_highlight_request(
                        revision,
                        request_text,
                        &language_hint,
                        theme_key,
                        id.as_str(),
                    );
                }
                let has_render = self
                    .highlight_render
                    .as_ref()
                    .filter(|render| {
                        render.matches_exact(
                            revision,
                            text_len,
                            &language_hint,
                            theme_key,
                            id.as_str(),
                        )
                    })
                    .is_some();
                let has_context_render = self
                    .highlight_render
                    .as_ref()
                    .filter(|render| render.matches_context(id.as_str(), &language_hint, theme_key))
                    .is_some();
                let has_staged_render = self
                    .highlight_staged
                    .as_ref()
                    .filter(|render| {
                        render.matches_exact(
                            revision,
                            text_len,
                            &language_hint,
                            theme_key,
                            id.as_str(),
                        )
                    })
                    .is_some();
                let has_staged_context = self
                    .highlight_staged
                    .as_ref()
                    .filter(|render| render.matches_context(id.as_str(), &language_hint, theme_key))
                    .is_some();
                // `is_large` and `should_request_highlight` share the same
                // threshold guard; once large, we force plain rendering and do
                // not allow context-only highlight fallback.
                let use_plain = if is_large {
                    true
                } else if async_mode {
                    !(has_context_render || has_staged_context)
                } else {
                    debounce_active && !has_render
                };
                if self.highlight_trace_enabled {
                    self.trace_highlight(
                        "frame",
                        format!(
                            "revision={} text_len={} async={} has_render={} has_render_ctx={} has_staged={} has_staged_ctx={} use_plain={}",
                            revision,
                            text_len,
                            async_mode,
                            has_render,
                            has_context_render,
                            has_staged_render,
                            has_staged_context,
                            use_plain
                        )
                        .as_str(),
                    );
                }
                let highlight_render = self.highlight_render.take();
                let highlight_render_match = highlight_render
                    .as_ref()
                    .filter(|render| {
                        render.matches_exact(
                            revision,
                            text_len,
                            &language_hint,
                            theme_key,
                            id.as_str(),
                        )
                    })
                    .or_else(|| {
                        if async_mode && !is_large {
                            highlight_render.as_ref().filter(|render| {
                                render.matches_context(id.as_str(), &language_hint, theme_key)
                            })
                        } else {
                            None
                        }
                    });
                let row_height = ui.text_style_height(&editor_style);

                let scroll = egui::ScrollArea::vertical()
                    .id_salt("editor_scroll")
                    .max_height(editor_height)
                    .auto_shrink([false; 2]);
                if use_virtual_preview {
                    self.render_virtual_preview_panel(
                        ui,
                        row_height,
                        editor_height,
                        &editor_font,
                        highlight_render_match,
                        use_plain,
                    );
                } else if use_virtual_editor {
                    self.render_virtual_editor_panel(
                        ui,
                        row_height,
                        editor_height,
                        &editor_font,
                        highlight_render_match,
                        use_plain,
                    );
                } else {
                    // Defensive fallback for impossible mode values.
                    self.virtual_editor_active = false;
                    scroll.show(ui, |_| {});
                }
                self.highlight_render = highlight_render;
            } else if self.selected_id.is_some() {
                self.virtual_editor_active = false;
                ui.label(RichText::new("Loading paste...").color(COLOR_TEXT_MUTED));
            } else {
                self.virtual_editor_active = false;
                ui.label(RichText::new("Select a paste from the sidebar.").color(COLOR_TEXT_MUTED));
            }
        });
    }
}

fn compact_header_tags(input: &str) -> Vec<String> {
    let mut tags = Vec::new();
    for tag in input.split(',') {
        let trimmed = tag.trim();
        if trimmed.is_empty() {
            continue;
        }
        if tags
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(trimmed))
        {
            continue;
        }
        tags.push(trimmed.to_string());
        if tags.len() >= 4 {
            break;
        }
    }
    tags
}

fn apply_compact_meta_row_style(ui: &mut egui::Ui) {
    let mut compact_style = (**ui.style()).clone();
    if let Some(body_font) = compact_style
        .text_styles
        .get(&egui::TextStyle::Body)
        .cloned()
    {
        compact_style.text_styles.insert(
            egui::TextStyle::Body,
            egui::FontId::new((body_font.size - 2.0).max(10.0), body_font.family),
        );
    }
    if let Some(button_font) = compact_style
        .text_styles
        .get(&egui::TextStyle::Button)
        .cloned()
    {
        compact_style.text_styles.insert(
            egui::TextStyle::Button,
            egui::FontId::new((button_font.size - 2.0).max(10.0), button_font.family),
        );
    }
    compact_style.spacing.button_padding = egui::vec2(8.0, 4.0);
    compact_style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    compact_style.spacing.interact_size.y = 28.0;
    ui.set_style(compact_style);
}
