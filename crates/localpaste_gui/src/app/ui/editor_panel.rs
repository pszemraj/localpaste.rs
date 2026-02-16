//! Central editor panel rendering for TextEdit, virtual preview, and virtual editor modes.

use super::super::*;
use eframe::egui;

impl LocalPasteApp {
    pub(crate) fn render_editor_panel(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            let selected_meta = self.selected_paste.as_ref().map(|paste| paste.id.clone());

            if let Some(id) = selected_meta {
                let language = self.edit_language.clone();
                let is_large = self.active_text_len_bytes() >= HIGHLIGHT_PLAIN_THRESHOLD;
                let lang_label = display_language_label(
                    language.as_deref(),
                    self.edit_language_is_manual,
                    is_large,
                );
                let visible_tags = compact_header_tags(self.edit_tags.as_str());

                let mut pending_language_filter: Option<Option<String>> = None;
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

                        if ui.small_button(format!("[{}]", lang_label)).clicked() {
                            pending_language_filter =
                                Some(self.edit_language.clone().and_then(|value| {
                                    let trimmed = value.trim();
                                    if trimmed.is_empty() {
                                        None
                                    } else {
                                        Some(trimmed.to_string())
                                    }
                                }));
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
                if let Some(language_filter) = pending_language_filter {
                    self.set_active_language_filter(language_filter);
                }
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
                if self.editor_mode == EditorMode::VirtualPreview {
                    ui.label(
                        RichText::new("Virtual preview (read-only)")
                            .small()
                            .color(COLOR_TEXT_MUTED),
                    );
                    ui.add_space(4.0);
                } else if self.editor_mode == EditorMode::VirtualEditor {
                    ui.label(
                        RichText::new("Virtual editor (rope-backed)")
                            .small()
                            .color(COLOR_TEXT_MUTED),
                    );
                    ui.add_space(4.0);
                }
                let editor_height = ui.available_height();
                let mut response = None;
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
                let mut content_snapshot_for_dispatch: Option<String> = None;
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
                    let content_snapshot = content_snapshot_for_dispatch
                        .take()
                        .unwrap_or_else(|| self.active_snapshot());
                    self.dispatch_highlight_request(
                        revision,
                        content_snapshot,
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
                        if async_mode {
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
                    self.virtual_editor_active = false;
                    scroll.show(ui, |ui| {
                        ui.set_min_size(egui::vec2(ui.available_width(), editor_height));
                        let rows_that_fit = ((editor_height / row_height).ceil() as usize).max(1);

                        let edit = egui::TextEdit::multiline(&mut self.selected_content)
                            .font(editor_style)
                            .desired_width(f32::INFINITY)
                            .desired_rows(rows_that_fit)
                            .lock_focus(true)
                            .hint_text("Start typing...");

                        let mut editor_cache = std::mem::take(&mut self.editor_cache);
                        let syntect = &self.syntect;
                        let highlight_version = self.highlight_version;
                        let mut layouter =
                            |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap_width: f32| {
                                editor_cache.layout(EditorLayoutRequest {
                                    ui,
                                    text,
                                    text_revision: Some(revision),
                                    wrap_width,
                                    language_hint: language_hint.as_str(),
                                    use_plain,
                                    theme: theme.as_ref(),
                                    highlight_render: highlight_render_match,
                                    highlight_version,
                                    editor_font: &editor_font,
                                    syntect,
                                })
                            };
                        let disable_builtin_double_click = async_mode;
                        let previous_double_click = if disable_builtin_double_click {
                            Some(ui.ctx().options_mut(|options| {
                                let previous = options.input_options.max_double_click_delay;
                                options.input_options.max_double_click_delay = 0.0;
                                previous
                            }))
                        } else {
                            None
                        };
                        let output = edit.layouter(&mut layouter).show(ui);
                        if let Some(previous) = previous_double_click {
                            ui.ctx().options_mut(|options| {
                                options.input_options.max_double_click_delay = previous;
                            });
                        }
                        self.editor_cache = editor_cache;
                        if disable_builtin_double_click && output.response.clicked() {
                            let text_snapshot = self.selected_content.to_string();
                            self.handle_large_editor_click(&output, &text_snapshot, true);
                        }
                        if self.focus_editor_next || output.response.clicked() {
                            output.response.request_focus();
                            self.focus_editor_next = false;
                        }
                        response = Some(output.response);
                    });
                }
                self.highlight_render = highlight_render;
                if response.map(|r| r.changed()).unwrap_or(false) {
                    self.mark_dirty();
                    let _ = self.selected_content.take_edit_delta();
                }
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
