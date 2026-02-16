//! Virtual preview/editor rendering extracted from the main editor panel.

use super::super::*;
use eframe::egui;
use tracing::info;

impl LocalPasteApp {
    pub(super) fn render_virtual_preview_panel(
        &mut self,
        ui: &mut egui::Ui,
        row_height: f32,
        editor_height: f32,
        editor_font: &egui::FontId,
        highlight_render_match: Option<&HighlightRender>,
        use_plain: bool,
    ) {
        let scroll = egui::ScrollArea::vertical()
            .id_salt("editor_scroll")
            .max_height(editor_height)
            .auto_shrink([false; 2]);

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
                let render_line =
                    highlight_render_match.and_then(|render| render.lines.get(line_idx));
                let job = build_virtual_line_job(ui, line, editor_font, render_line, use_plain);
                let line_chars = self.editor_lines.line_len_chars(line_idx);
                let galley = ui.fonts_mut(|f| f.layout_job(job));
                let row_width = ui.available_width();
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(row_width, row_height), sense);
                if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
                }
                if pending_action.is_none() && (response.drag_started() || response.clicked()) {
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
                            pending_action = Some(RowAction::DragStart { cursor: vcursor });
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
                                    pending_action = Some(RowAction::Click { cursor: vcursor });
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
                            pointer_pos.y >= row.rect.min.y && pointer_pos.y <= row.rect.max.y
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
    }

    pub(super) fn render_virtual_editor_panel(
        &mut self,
        ui: &mut egui::Ui,
        row_height: f32,
        editor_height: f32,
        editor_font: &egui::FontId,
        highlight_render_match: Option<&HighlightRender>,
        use_plain: bool,
    ) {
        let scroll = egui::ScrollArea::vertical()
            .id_salt("editor_scroll")
            .max_height(editor_height)
            .auto_shrink([false; 2]);

        let editor_id = egui::Id::new(VIRTUAL_EDITOR_ID);
        if self.focus_editor_next {
            ui.memory_mut(|m| m.request_focus(editor_id));
            self.virtual_editor_state.has_focus = true;
            self.reset_virtual_caret_blink();
            self.focus_editor_next = false;
        }

        let wrap_width = ui.available_width().max(1.0);
        let perf_enabled = self.perf_log_enabled;
        let frame_started = perf_enabled.then(Instant::now);
        let mut layout_rebuild_ms = 0.0f32;
        let mut visible_rows = 0usize;
        let mut galley_hits = 0usize;
        let mut galley_misses = 0usize;
        let mut galley_build_ms = 0.0f32;
        let mut paint_ms = 0.0f32;
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
            line_count,
        ) {
            let rebuild_started = perf_enabled.then(Instant::now);
            self.virtual_layout.rebuild(
                &self.virtual_editor_buffer,
                wrap_width,
                self.virtual_line_height,
                char_width,
            );
            if let Some(started) = rebuild_started {
                layout_rebuild_ms = started.elapsed().as_secs_f32() * 1000.0;
            }
        }
        let total_rows = self.virtual_layout.total_rows().max(1);
        self.virtual_galley_cache.prepare_frame(
            line_count,
            VirtualGalleyContext::new(
                wrap_width,
                use_plain,
                editor_font,
                ui.visuals().text_color(),
            ),
        );
        let mut focused = ui.memory(|m| m.has_focus(editor_id));
        let mut editor_interacted = false;
        scroll.show_rows(ui, self.virtual_line_height, total_rows, |ui, range| {
            ui.set_min_width(wrap_width);
            visible_rows = range.len();
            struct RowRender {
                segment_start: usize,
                segment_chars: usize,
                ends_line: bool,
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
                    line_idx: usize,
                    line_start: usize,
                    column_in_line: usize,
                },
                DragStart {
                    global: usize,
                },
            }
            let mut rows = Vec::with_capacity(range.len());
            let mut pending_action: Option<RowAction> = None;
            let mut last_synced_line: Option<usize> = None;
            for row_idx in range {
                let (line_idx, row_in_line) = self.virtual_layout.row_to_line(row_idx);
                let line_start = self.virtual_editor_buffer.line_col_to_char(line_idx, 0);
                let line_chars = self.virtual_layout.line_chars(line_idx);
                let segment_range = self
                    .virtual_layout
                    .row_char_range(&self.virtual_editor_buffer, row_idx);
                let segment_chars = segment_range.end.saturating_sub(segment_range.start);
                let segment_start_in_line = segment_range.start.saturating_sub(line_start);
                let line_start_byte = self.virtual_editor_buffer.rope().char_to_byte(line_start);
                let segment_start_byte = self
                    .virtual_editor_buffer
                    .rope()
                    .char_to_byte(segment_range.start)
                    .saturating_sub(line_start_byte);
                let segment_end_byte = self
                    .virtual_editor_buffer
                    .rope()
                    .char_to_byte(segment_range.end)
                    .saturating_sub(line_start_byte);
                let line_visual_rows = self.virtual_layout.line_visual_rows(line_idx);
                let ends_line = row_in_line.saturating_add(1) >= line_visual_rows;
                let render_line =
                    highlight_render_match.and_then(|render| render.lines.get(line_idx));
                if last_synced_line != Some(line_idx) {
                    self.virtual_galley_cache
                        .sync_line_rows(line_idx, line_visual_rows);
                    last_synced_line = Some(line_idx);
                }
                let galley = if let Some(cached) =
                    self.virtual_galley_cache.get(line_idx, row_in_line)
                {
                    if perf_enabled {
                        galley_hits = galley_hits.saturating_add(1);
                    }
                    cached
                } else {
                    let build_started = perf_enabled.then(Instant::now);
                    self.virtual_editor_buffer
                        .slice_chars_into(segment_range.clone(), &mut self.virtual_line_scratch);
                    let mut job = build_virtual_line_segment_job_owned(
                        ui,
                        std::mem::take(&mut self.virtual_line_scratch),
                        editor_font,
                        render_line,
                        use_plain,
                        segment_start_byte..segment_end_byte,
                    );
                    job.wrap.max_width = wrap_width;
                    let shaped = ui.fonts_mut(|f| f.layout_job(job));
                    self.virtual_galley_cache
                        .insert(line_idx, row_in_line, shaped.clone());
                    if let Some(started) = build_started {
                        galley_build_ms += started.elapsed().as_secs_f32() * 1000.0;
                        galley_misses = galley_misses.saturating_add(1);
                    }
                    shaped
                };
                let row_width = ui.available_width();
                let (rect, response) = ui.allocate_exact_size(
                    egui::vec2(row_width, self.virtual_line_height),
                    egui::Sense::click_and_drag(),
                );
                if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
                }
                if pending_action.is_none() && (response.drag_started() || response.clicked()) {
                    if let Some(pointer_pos) = response.interact_pointer_pos() {
                        let local_pos = pointer_pos - rect.min;
                        let cursor = galley.cursor_from_pos(local_pos);
                        let local_col = cursor.index.min(segment_chars);
                        let global = segment_range.start.saturating_add(local_col);
                        if response.drag_started() {
                            self.reset_virtual_click_streak();
                            editor_interacted = true;
                            pending_action = Some(RowAction::DragStart { global });
                        } else {
                            let click_count = self.register_virtual_click(line_idx, pointer_pos);
                            self.last_virtual_click_count = click_count;
                            match click_count {
                                3 => {
                                    editor_interacted = true;
                                    pending_action = Some(RowAction::Triple { line_idx });
                                }
                                2 => {
                                    editor_interacted = true;
                                    pending_action = Some(RowAction::Double {
                                        line_idx,
                                        line_start,
                                        column_in_line: segment_start_in_line
                                            .saturating_add(local_col)
                                            .min(line_chars),
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
                    segment_start: segment_range.start,
                    segment_chars,
                    ends_line,
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
                        self.reset_virtual_caret_blink();
                    }
                    RowAction::Triple { line_idx } => {
                        self.virtual_select_line(line_idx);
                        self.reset_virtual_caret_blink();
                    }
                    RowAction::Double {
                        line_idx,
                        line_start,
                        column_in_line,
                    } => {
                        let word_range = {
                            self.virtual_editor_buffer.line_without_newline_into(
                                line_idx,
                                &mut self.virtual_line_scratch,
                            );
                            word_range_at(self.virtual_line_scratch.as_str(), column_in_line)
                        };
                        if let Some((start, end)) = word_range {
                            let global_start = line_start.saturating_add(start);
                            let global_end = line_start.saturating_add(end);
                            self.virtual_editor_state
                                .set_cursor(global_start, self.virtual_editor_buffer.len_chars());
                            self.virtual_editor_state.move_cursor(
                                global_end,
                                self.virtual_editor_buffer.len_chars(),
                                true,
                            );
                        } else {
                            let global = line_start.saturating_add(column_in_line);
                            self.virtual_editor_state
                                .set_cursor(global, self.virtual_editor_buffer.len_chars());
                        }
                        self.virtual_editor_state.clear_preferred_column();
                        self.reset_virtual_caret_blink();
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
                        self.reset_virtual_caret_blink();
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
                            pointer_pos.y >= row.rect.min.y && pointer_pos.y <= row.rect.max.y
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
                            .segment_start
                            .saturating_add(cursor.index.min(row.segment_chars));
                        self.virtual_editor_state.move_cursor(
                            global,
                            self.virtual_editor_buffer.len_chars(),
                            true,
                        );
                        self.reset_virtual_caret_blink();
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
            let now = Instant::now();
            let blink_ticks = now
                .duration_since(self.virtual_caret_phase_start)
                .as_millis()
                / CARET_BLINK_INTERVAL.as_millis().max(1);
            let caret_visible = blink_ticks % 2 == 0;
            let paint_started = perf_enabled.then(Instant::now);
            for row in rows {
                let galley = row.galley;
                if let Some(selection) =
                    self.virtual_selection_for_line(row.segment_start, row.segment_chars)
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
                    .galley(row.rect.min, galley.clone(), ui.visuals().text_color());

                if focused && caret_visible {
                    let cursor = self.virtual_editor_state.cursor();
                    let segment_end = row.segment_start.saturating_add(row.segment_chars);
                    let shows_caret = if cursor < segment_end {
                        true
                    } else {
                        cursor == segment_end && row.ends_line
                    };
                    if cursor >= row.segment_start && shows_caret {
                        let local_col = cursor.saturating_sub(row.segment_start);
                        let caret_rect = galley.pos_from_cursor(CCursor::new(local_col));
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
            if let Some(started) = paint_started {
                paint_ms = started.elapsed().as_secs_f32() * 1000.0;
            }
        });
        focused = ui.memory(|m| m.has_focus(editor_id));
        self.virtual_editor_state.has_focus = focused;
        self.virtual_editor_active = focused
            || self.virtual_editor_state.has_focus
            || self.virtual_drag_active
            || editor_interacted;
        if let Some(started) = frame_started {
            let total_ms = started.elapsed().as_secs_f32() * 1000.0;
            info!(
                target: "localpaste_gui::perf",
                event = "virtual_editor_render",
                visible_rows = visible_rows,
                galley_hits = galley_hits,
                galley_misses = galley_misses,
                galley_build_ms = galley_build_ms,
                layout_rebuild_ms = layout_rebuild_ms,
                paint_ms = paint_ms,
                total_ms = total_ms,
                "virtual editor frame breakdown"
            );
        }
    }
}
