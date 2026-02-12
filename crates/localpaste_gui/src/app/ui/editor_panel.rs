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
                let lang_label = display_language_label(language.as_deref(), is_large);
                let visible_tags = compact_header_tags(self.edit_tags.as_str());

                let mut pending_language_filter: Option<Option<String>> = None;
                let mut pending_tag_search: Option<String> = None;
                let mut apply_metadata = false;
                let mut save_requested = false;
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
                            .add_enabled(self.metadata_dirty, egui::Button::new("Apply"))
                            .clicked()
                        {
                            apply_metadata = true;
                        }
                        if ui.small_button("Save").clicked() {
                            save_requested = true;
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
                if save_requested {
                    self.save_now();
                }
                if copy_requested {
                    self.clipboard_outgoing = Some(self.active_snapshot());
                    self.set_status("Copied paste content.");
                }
                if copy_link_requested {
                    self.clipboard_outgoing =
                        Some(format!("http://{}/api/paste/{}", self.server_addr, id));
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
                let debounce_active = self
                    .last_edit_at
                    .map(|last| {
                        self.active_text_len_bytes() >= HIGHLIGHT_DEBOUNCE_MIN_BYTES
                            && last.elapsed() < HIGHLIGHT_DEBOUNCE
                    })
                    .unwrap_or(false);
                let theme = (!is_large).then(|| CodeTheme::from_memory(ui.ctx(), ui.style()));
                let theme_key = theme
                    .as_ref()
                    .map(syntect_theme_key)
                    .unwrap_or("base16-mocha.dark");
                let revision = self.active_revision();
                let text_len = self.active_text_len_bytes();
                let async_mode = text_len >= HIGHLIGHT_DEBOUNCE_MIN_BYTES && !is_large;
                let should_request = async_mode
                    && self.should_request_highlight(
                        revision,
                        text_len,
                        &language_hint,
                        theme_key,
                        debounce_active,
                        id.as_str(),
                    );
                if should_request {
                    let content_snapshot = self.active_snapshot();
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
                let use_virtual_preview = self.editor_mode == EditorMode::VirtualPreview;
                let use_virtual_editor = self.editor_mode == EditorMode::VirtualEditor;

                let scroll = egui::ScrollArea::vertical()
                    .id_salt("editor_scroll")
                    .max_height(editor_height)
                    .auto_shrink([false; 2]);
                if use_virtual_preview {
                    let text = self.selected_content.as_str();
                    self.editor_lines
                        .ensure_for(self.selected_content.revision(), text);
                    let line_count = self.editor_lines.line_count();
                    let mut last_virtual_click_at = self.last_virtual_click_at;
                    let mut last_virtual_click_pos = self.last_virtual_click_pos;
                    let mut last_virtual_click_line = self.last_virtual_click_line;
                    let mut last_virtual_click_count = self.last_virtual_click_count;
                    scroll.show_rows(ui, row_height, line_count, |ui, range| {
                        ui.set_min_width(ui.available_width());
                        let sense = egui::Sense::click_and_drag();
                        struct RowRender {
                            line_idx: usize,
                            rect: egui::Rect,
                            galley: Arc<egui::Galley>,
                            line_chars: usize,
                        }
                        enum RowAction<'a> {
                            Triple {
                                line_idx: usize,
                                line_chars: usize,
                            },
                            Double {
                                cursor: VirtualCursor,
                                line: &'a str,
                            },
                            DragStart {
                                cursor: VirtualCursor,
                            },
                            Click {
                                cursor: VirtualCursor,
                            },
                        }
                        let mut rows = Vec::with_capacity(range.len());
                        let mut pending_action: Option<RowAction<'_>> = None;
                        for line_idx in range {
                            let line = self.editor_lines.line_without_newline(text, line_idx);
                            let render_line = highlight_render_match
                                .and_then(|render| render.lines.get(line_idx));
                            let job = build_virtual_line_job(
                                ui,
                                line,
                                &editor_font,
                                render_line,
                                use_plain,
                            );
                            let line_chars = line.chars().count();
                            let galley = ui.fonts_mut(|f| f.layout_job(job));
                            let row_width = ui.available_width();
                            let (rect, response) =
                                ui.allocate_exact_size(egui::vec2(row_width, row_height), sense);
                            if response.hovered() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
                            }
                            if pending_action.is_none()
                                && (response.drag_started() || response.clicked())
                            {
                                if let Some(pointer_pos) = response.interact_pointer_pos() {
                                    let local_pos = pointer_pos - rect.min;
                                    let cursor = galley.cursor_from_pos(local_pos);
                                    let vcursor = VirtualCursor {
                                        line: line_idx,
                                        column: cursor.index,
                                    };
                                    if response.drag_started() {
                                        last_virtual_click_at = None;
                                        last_virtual_click_pos = None;
                                        last_virtual_click_line = None;
                                        last_virtual_click_count = 0;
                                        pending_action =
                                            Some(RowAction::DragStart { cursor: vcursor });
                                    } else {
                                        let now = Instant::now();
                                        let click_count = next_virtual_click_count(
                                            last_virtual_click_at,
                                            last_virtual_click_pos,
                                            last_virtual_click_line,
                                            last_virtual_click_count,
                                            line_idx,
                                            pointer_pos,
                                            now,
                                        );
                                        last_virtual_click_at = Some(now);
                                        last_virtual_click_pos = Some(pointer_pos);
                                        last_virtual_click_line = Some(line_idx);
                                        last_virtual_click_count = click_count;
                                        match click_count {
                                            3 => {
                                                pending_action = Some(RowAction::Triple {
                                                    line_idx,
                                                    line_chars,
                                                });
                                            }
                                            2 => {
                                                pending_action = Some(RowAction::Double {
                                                    cursor: vcursor,
                                                    line,
                                                });
                                            }
                                            _ => {
                                                pending_action =
                                                    Some(RowAction::Click { cursor: vcursor });
                                            }
                                        }
                                    }
                                }
                            }
                            rows.push(RowRender {
                                line_idx,
                                rect,
                                galley,
                                line_chars,
                            });
                        }

                        if let Some(action) = pending_action {
                            match action {
                                RowAction::Triple {
                                    line_idx,
                                    line_chars,
                                } => {
                                    let start = VirtualCursor {
                                        line: line_idx,
                                        column: 0,
                                    };
                                    let end = if line_idx + 1 < line_count {
                                        VirtualCursor {
                                            line: line_idx + 1,
                                            column: 0,
                                        }
                                    } else {
                                        VirtualCursor {
                                            line: line_idx,
                                            column: line_chars,
                                        }
                                    };
                                    self.virtual_selection.select_range(start, end);
                                }
                                RowAction::Double { cursor, line } => {
                                    if let Some((start, end)) = word_range_at(line, cursor.column) {
                                        self.virtual_selection.select_range(
                                            VirtualCursor {
                                                line: cursor.line,
                                                column: start,
                                            },
                                            VirtualCursor {
                                                line: cursor.line,
                                                column: end,
                                            },
                                        );
                                    } else {
                                        self.virtual_selection.set_cursor(cursor);
                                    }
                                }
                                RowAction::DragStart { cursor } => {
                                    self.virtual_selection.begin_drag(cursor);
                                }
                                RowAction::Click { cursor } => {
                                    self.virtual_selection.set_cursor(cursor);
                                }
                            }
                        }

                        let pointer_pos = ui.input(|input| {
                            input
                                .pointer
                                .interact_pos()
                                .or_else(|| input.pointer.latest_pos())
                        });
                        let pointer_down = ui.input(|input| input.pointer.primary_down());
                        if pointer_down {
                            if let Some(pointer_pos) = pointer_pos {
                                let viewport_rect = ui.clip_rect();
                                let target_row = rows
                                    .iter()
                                    .find(|row| {
                                        pointer_pos.y >= row.rect.min.y
                                            && pointer_pos.y <= row.rect.max.y
                                    })
                                    .or_else(|| {
                                        let first = rows.first()?;
                                        let last = rows.last()?;
                                        if pointer_pos.y < first.rect.min.y {
                                            Some(first)
                                        } else if pointer_pos.y > last.rect.max.y {
                                            Some(last)
                                        } else {
                                            None
                                        }
                                    });
                                if let Some(row) = target_row {
                                    let clamped_pos = egui::pos2(
                                        pointer_pos.x.clamp(row.rect.min.x, row.rect.max.x),
                                        pointer_pos.y.clamp(row.rect.min.y, row.rect.max.y),
                                    );
                                    let local_pos = clamped_pos - row.rect.min;
                                    let cursor = row.galley.cursor_from_pos(local_pos);
                                    let vcursor = VirtualCursor {
                                        line: row.line_idx,
                                        column: cursor.index,
                                    };
                                    self.virtual_selection.update_drag(vcursor);
                                }
                                let scroll_delta = drag_autoscroll_delta(
                                    pointer_pos.y,
                                    viewport_rect.min.y,
                                    viewport_rect.max.y,
                                    row_height,
                                );
                                if scroll_delta != 0.0 {
                                    ui.scroll_with_delta(egui::vec2(0.0, scroll_delta));
                                }
                            }
                        } else {
                            self.virtual_selection.end_drag();
                        }

                        let selection_fill = ui.visuals().selection.bg_fill;
                        for row in rows {
                            let galley = row.galley;
                            if let Some(selection) = self
                                .virtual_selection
                                .selection_for_line(row.line_idx, row.line_chars)
                            {
                                paint_virtual_selection_overlay(
                                    ui.painter(),
                                    row.rect,
                                    galley.as_ref(),
                                    selection,
                                    selection_fill,
                                );
                            }
                            ui.painter()
                                .galley(row.rect.min, galley, ui.visuals().text_color());
                        }
                    });
                    self.last_virtual_click_at = last_virtual_click_at;
                    self.last_virtual_click_pos = last_virtual_click_pos;
                    self.last_virtual_click_line = last_virtual_click_line;
                    self.last_virtual_click_count = last_virtual_click_count;
                    self.virtual_editor_active = false;
                } else if use_virtual_editor {
                    let editor_id = egui::Id::new(VIRTUAL_EDITOR_ID);
                    if self.focus_editor_next {
                        ui.memory_mut(|m| m.request_focus(editor_id));
                        self.virtual_editor_state.has_focus = true;
                        self.focus_editor_next = false;
                    }

                    let wrap_width = ui.available_width().max(1.0);
                    let char_width = ui.fonts_mut(|f| {
                        f.layout_no_wrap(
                            "W".to_owned(),
                            editor_font.clone(),
                            ui.visuals().text_color(),
                        )
                        .size()
                        .x
                        .max(1.0)
                    });
                    self.virtual_line_height = row_height.max(1.0);
                    self.virtual_wrap_width = wrap_width;
                    self.virtual_viewport_height = editor_height;
                    let line_count = self.virtual_editor_buffer.line_count();
                    if self.virtual_layout.needs_rebuild(
                        self.virtual_editor_buffer.revision(),
                        wrap_width,
                        self.virtual_line_height,
                        char_width,
                        self.highlight_version,
                        line_count,
                    ) {
                        self.virtual_layout.rebuild(
                            &self.virtual_editor_buffer,
                            wrap_width,
                            self.virtual_line_height,
                            char_width,
                            self.highlight_version,
                        );
                    }
                    let total_height = self.virtual_layout.total_height();
                    let mut focused = ui.memory(|m| m.has_focus(editor_id));
                    let mut editor_interacted = false;
                    scroll.show_viewport(ui, |ui, viewport| {
                        ui.set_min_width(wrap_width);
                        ui.set_min_height(total_height.max(editor_height));
                        let content_origin = ui.min_rect().min;
                        let content_rect = egui::Rect::from_min_max(
                            content_origin,
                            egui::pos2(
                                content_origin.x + wrap_width,
                                content_origin.y + total_height.max(editor_height),
                            ),
                        );
                        let background_response =
                            ui.interact(content_rect, editor_id, egui::Sense::click());
                        if background_response.clicked() {
                            ui.memory_mut(|m| m.request_focus(editor_id));
                            focused = true;
                            self.virtual_editor_state.has_focus = true;
                            editor_interacted = true;
                        } else if background_response.lost_focus() {
                            self.virtual_editor_state.has_focus = false;
                        }
                        let visible = self.virtual_layout.visible_range(
                            viewport.min.y,
                            viewport.height(),
                            VIRTUAL_OVERSCAN_LINES,
                        );
                        struct RowRender {
                            line_start: usize,
                            line_chars: usize,
                            rect: egui::Rect,
                            galley: Arc<egui::Galley>,
                        }
                        enum RowAction {
                            Click {
                                global: usize,
                            },
                            Triple {
                                line_idx: usize,
                            },
                            Double {
                                line_start: usize,
                                line: String,
                                column: usize,
                            },
                            DragStart {
                                global: usize,
                            },
                        }
                        let mut rows = Vec::with_capacity(visible.len());
                        let mut pending_action: Option<RowAction> = None;
                        for line_idx in visible {
                            let line_start =
                                self.virtual_editor_buffer.line_col_to_char(line_idx, 0);
                            let line_owned =
                                self.virtual_editor_buffer.line_without_newline(line_idx);
                            let line = line_owned.as_str();
                            let line_chars = line.chars().count();
                            let render_line = highlight_render_match
                                .and_then(|render| render.lines.get(line_idx));
                            let mut job = build_virtual_line_job(
                                ui,
                                line,
                                &editor_font,
                                render_line,
                                use_plain,
                            );
                            job.wrap.max_width = wrap_width;
                            let galley = ui.fonts_mut(|f| f.layout_job(job));
                            let row_top = content_origin.y + self.virtual_layout.line_top(line_idx);
                            let row_bottom =
                                content_origin.y + self.virtual_layout.line_bottom(line_idx);
                            let rect = egui::Rect::from_min_max(
                                egui::pos2(content_origin.x, row_top),
                                egui::pos2(content_origin.x + wrap_width, row_bottom),
                            );
                            let response = ui.interact(
                                rect,
                                editor_id.with(line_idx),
                                egui::Sense::click_and_drag(),
                            );
                            if response.hovered() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
                            }
                            if pending_action.is_none()
                                && (response.drag_started() || response.clicked())
                            {
                                if let Some(pointer_pos) = response.interact_pointer_pos() {
                                    let local_pos = pointer_pos - rect.min;
                                    let cursor = galley.cursor_from_pos(local_pos);
                                    let global =
                                        line_start.saturating_add(cursor.index.min(line_chars));
                                    if response.drag_started() {
                                        self.reset_virtual_click_streak();
                                        editor_interacted = true;
                                        pending_action = Some(RowAction::DragStart { global });
                                    } else {
                                        let click_count =
                                            self.register_virtual_click(line_idx, pointer_pos);
                                        self.last_virtual_click_count = click_count;
                                        match click_count {
                                            3 => {
                                                editor_interacted = true;
                                                pending_action =
                                                    Some(RowAction::Triple { line_idx });
                                            }
                                            2 => {
                                                editor_interacted = true;
                                                pending_action = Some(RowAction::Double {
                                                    line_start,
                                                    line: line.to_string(),
                                                    column: cursor.index.min(line_chars),
                                                });
                                            }
                                            _ => {
                                                editor_interacted = true;
                                                pending_action = Some(RowAction::Click { global });
                                            }
                                        }
                                    }
                                }
                            }
                            rows.push(RowRender {
                                line_start,
                                line_chars,
                                rect,
                                galley,
                            });
                        }

                        if let Some(action) = pending_action {
                            ui.memory_mut(|m| m.request_focus(editor_id));
                            focused = true;
                            self.virtual_editor_state.has_focus = true;
                            editor_interacted = true;
                            match action {
                                RowAction::Click { global } => {
                                    self.virtual_editor_state
                                        .set_cursor(global, self.virtual_editor_buffer.len_chars());
                                    self.virtual_editor_state.clear_preferred_column();
                                }
                                RowAction::Triple { line_idx } => {
                                    self.virtual_select_line(line_idx);
                                }
                                RowAction::Double {
                                    line_start,
                                    line,
                                    column,
                                } => {
                                    if let Some((start, end)) = word_range_at(line.as_str(), column)
                                    {
                                        let global_start = line_start.saturating_add(start);
                                        let global_end = line_start.saturating_add(end);
                                        self.virtual_editor_state.set_cursor(
                                            global_start,
                                            self.virtual_editor_buffer.len_chars(),
                                        );
                                        self.virtual_editor_state.move_cursor(
                                            global_end,
                                            self.virtual_editor_buffer.len_chars(),
                                            true,
                                        );
                                    } else {
                                        let global = line_start.saturating_add(column);
                                        self.virtual_editor_state.set_cursor(
                                            global,
                                            self.virtual_editor_buffer.len_chars(),
                                        );
                                    }
                                    self.virtual_editor_state.clear_preferred_column();
                                }
                                RowAction::DragStart { global } => {
                                    self.virtual_editor_state
                                        .set_cursor(global, self.virtual_editor_buffer.len_chars());
                                    self.virtual_editor_state.move_cursor(
                                        global,
                                        self.virtual_editor_buffer.len_chars(),
                                        true,
                                    );
                                    self.virtual_drag_active = true;
                                    self.virtual_editor_state.clear_preferred_column();
                                }
                            }
                        }

                        let pointer_pos = ui.input(|input| {
                            input
                                .pointer
                                .interact_pos()
                                .or_else(|| input.pointer.latest_pos())
                        });
                        let pointer_down = ui.input(|input| input.pointer.primary_down());
                        if pointer_down && self.virtual_drag_active {
                            editor_interacted = true;
                            if let Some(pointer_pos) = pointer_pos {
                                let viewport_rect = ui.clip_rect();
                                let target_row = rows
                                    .iter()
                                    .find(|row| {
                                        pointer_pos.y >= row.rect.min.y
                                            && pointer_pos.y <= row.rect.max.y
                                    })
                                    .or_else(|| {
                                        let first = rows.first()?;
                                        let last = rows.last()?;
                                        if pointer_pos.y < first.rect.min.y {
                                            Some(first)
                                        } else if pointer_pos.y > last.rect.max.y {
                                            Some(last)
                                        } else {
                                            None
                                        }
                                    });
                                if let Some(row) = target_row {
                                    let clamped_pos = egui::pos2(
                                        pointer_pos.x.clamp(row.rect.min.x, row.rect.max.x),
                                        pointer_pos.y.clamp(row.rect.min.y, row.rect.max.y),
                                    );
                                    let local_pos = clamped_pos - row.rect.min;
                                    let cursor = row.galley.cursor_from_pos(local_pos);
                                    let global = row
                                        .line_start
                                        .saturating_add(cursor.index.min(row.line_chars));
                                    self.virtual_editor_state.move_cursor(
                                        global,
                                        self.virtual_editor_buffer.len_chars(),
                                        true,
                                    );
                                }
                                let scroll_delta = drag_autoscroll_delta(
                                    pointer_pos.y,
                                    viewport_rect.min.y,
                                    viewport_rect.max.y,
                                    self.virtual_line_height,
                                );
                                if scroll_delta != 0.0 {
                                    ui.scroll_with_delta(egui::vec2(0.0, scroll_delta));
                                }
                            }
                        } else if !pointer_down {
                            self.virtual_drag_active = false;
                        }

                        let selection_fill = ui.visuals().selection.bg_fill;
                        for row in rows {
                            let galley = row.galley;
                            if let Some(selection) =
                                self.virtual_selection_for_line(row.line_start, row.line_chars)
                            {
                                paint_virtual_selection_overlay(
                                    ui.painter(),
                                    row.rect,
                                    galley.as_ref(),
                                    selection,
                                    selection_fill,
                                );
                            }
                            ui.painter().galley(
                                row.rect.min,
                                galley.clone(),
                                ui.visuals().text_color(),
                            );

                            if focused {
                                let cursor = self.virtual_editor_state.cursor();
                                let line_end = row.line_start.saturating_add(row.line_chars);
                                if cursor >= row.line_start && cursor <= line_end {
                                    let local_col = cursor.saturating_sub(row.line_start);
                                    let caret_rect =
                                        galley.pos_from_cursor(CCursor::new(local_col));
                                    let x = row.rect.min.x + caret_rect.min.x;
                                    let y_min = row.rect.min.y + caret_rect.min.y;
                                    let y_max = row.rect.min.y + caret_rect.max.y;
                                    ui.painter().line_segment(
                                        [egui::pos2(x, y_min), egui::pos2(x, y_max)],
                                        Stroke::new(1.0, ui.visuals().text_color()),
                                    );
                                }
                            }
                        }
                    });
                    self.virtual_editor_active = focused
                        || self.virtual_editor_state.has_focus
                        || self.virtual_drag_active
                        || editor_interacted;
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
