//! Virtual preview/editor rendering extracted from the main editor panel.

use super::super::*;
use eframe::egui;
use tracing::info;

mod support;

use support::*;

/// Rendering flags for the interactive rope-backed virtual editor surface.
#[derive(Clone, Copy)]
pub(super) struct VirtualEditorRenderOptions<'a> {
    /// Optional precomputed highlight render payload.
    pub(super) highlight_render_match: Option<&'a HighlightRender>,
    /// When `true`, bypass syntax-highlighted rendering.
    pub(super) use_plain: bool,
    /// Whether same-frame editor-chrome actions should preserve editor focus.
    pub(super) preserve_focus_from_editor_chrome: bool,
}

impl LocalPasteApp {
    fn queue_virtual_cursor_follow_scroll(
        &mut self,
        scroll_offset_y: f32,
        editor_height: f32,
    ) -> bool {
        let cursor_row = self.virtual_cursor_row_index(self.virtual_editor_state.cursor());
        let viewport_rows = ((editor_height / self.virtual_line_height).floor().max(1.0)) as usize;
        if let Some(offset) = follow_cursor_scroll_offset_y(
            true,
            cursor_row,
            scroll_offset_y,
            viewport_rows,
            self.virtual_line_height,
        ) {
            self.virtual_pending_scroll_offset_y = Some(offset.max(0.0));
            return true;
        }
        false
    }

    /// Renders the interactive rope-backed virtual editor surface.
    ///
    /// # Arguments
    /// - `ui`: Target UI region.
    /// - `row_height`: Height per rendered row.
    /// - `editor_height`: Available viewport height.
    /// - `editor_font`: Font id used to shape line galleys.
    /// - `options`: Highlight/focus rendering flags for this frame.
    ///
    /// # Panics
    /// Panics if shaped virtual row galleys unexpectedly wrap.
    pub(super) fn render_virtual_editor_panel(
        &mut self,
        ui: &mut egui::Ui,
        row_height: f32,
        editor_height: f32,
        editor_font: &egui::FontId,
        options: VirtualEditorRenderOptions<'_>,
    ) {
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt("editor_scroll")
            .max_height(editor_height)
            .auto_shrink([false; 2]);
        if let Some(offset) = self.virtual_pending_scroll_offset_y.take() {
            scroll = scroll.vertical_scroll_offset(offset.max(0.0));
        }

        let editor_id = egui::Id::new(VIRTUAL_EDITOR_ID);
        let focus_editor_requested = self.focus_editor_next;
        let editor_shortcuts_unblocked_for_frame = !self.editor_shortcuts_blocked();
        let other_keyboard_input_has_focus = ui.memory(|m| {
            m.focused()
                .map(|focused_id| focused_id != editor_id)
                .unwrap_or(false)
        }) && ui.ctx().wants_keyboard_input();
        if other_keyboard_input_has_focus && !focus_editor_requested {
            self.virtual_editor_state.has_focus = false;
        }
        if focus_editor_requested {
            self.virtual_editor_state.has_focus = true;
            request_virtual_editor_focus(ui, editor_id, editor_shortcuts_unblocked_for_frame);
            self.reset_virtual_caret_blink();
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
        let mut visible_row_range: Option<std::ops::Range<usize>> = None;
        self.virtual_line_height = row_height.max(1.0);
        let line_number_font = line_number_font_for_row_height(self.virtual_line_height);
        let editor_char_width = ui.fonts_mut(|f| {
            f.layout_no_wrap(
                "W".to_owned(),
                editor_font.clone(),
                ui.visuals().text_color(),
            )
            .size()
            .x
            .max(1.0)
        });
        let line_count = self.virtual_editor_buffer.line_count();
        let line_number_char_width = ui.fonts_mut(|f| {
            f.layout_no_wrap(
                "W".to_owned(),
                line_number_font.clone(),
                ui.visuals().text_color(),
            )
            .size()
            .x
            .max(1.0)
        });
        let line_number_gutter = line_number_gutter_width(line_count, line_number_char_width);
        let content_wrap_width =
            (wrap_width - line_number_gutter - VIRTUAL_EDITOR_TEXT_INSET).max(editor_char_width);
        self.virtual_wrap_width = wrap_width;
        self.virtual_viewport_height = editor_height;
        if self.virtual_layout.needs_rebuild(
            self.virtual_editor_buffer.revision(),
            content_wrap_width,
            self.virtual_line_height,
            editor_char_width,
            line_count,
        ) {
            let rebuild_started = perf_enabled.then(Instant::now);
            self.virtual_layout.rebuild(
                &self.virtual_editor_buffer,
                content_wrap_width,
                self.virtual_line_height,
                editor_char_width,
            );
            if let Some(started) = rebuild_started {
                layout_rebuild_ms = started.elapsed().as_secs_f32() * 1000.0;
            }
        }
        const EOF_PADDING_ROWS: usize = 3;
        let content_rows = self.virtual_layout.total_rows().max(1);
        let total_rows = content_rows.saturating_add(EOF_PADDING_ROWS);
        self.virtual_galley_cache.prepare_frame(
            line_count,
            VirtualGalleyContext::new(
                content_wrap_width,
                options.use_plain,
                editor_font,
                ui.visuals().text_color(),
                ui.ctx().pixels_per_point(),
            ),
        );
        let mut focused =
            ui.memory(|m| m.has_focus(editor_id)) || self.virtual_editor_state.has_focus;
        let had_focus = focused;
        let mut ime_cursor_rect: Option<egui::Rect> = None;
        let mut editor_pointer_action_handled = false;
        let scroll_output =
            scroll.show_rows(ui, self.virtual_line_height, total_rows, |ui, range| {
                ui.set_min_width(wrap_width);
                visible_rows = range.len();
                visible_row_range = Some(range.clone());
                struct RowRender {
                    line_idx: usize,
                    segment_start: usize,
                    segment_chars: usize,
                    starts_line: bool,
                    ends_line: bool,
                    rect: egui::Rect,
                    text_rect: egui::Rect,
                    text_origin: egui::Pos2,
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
                for row_idx in range.clone() {
                    if row_idx >= content_rows {
                        let row_width = ui.available_width();
                        let (_, response) = ui.allocate_exact_size(
                            egui::vec2(row_width, self.virtual_line_height),
                            virtual_row_hit_test_sense(),
                        );
                        if response.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
                        }
                        continue;
                    }
                    let (line_idx, row_in_line) = self.virtual_layout.row_to_line(row_idx);
                    let line_start = self.virtual_editor_buffer.line_col_to_char(line_idx, 0);
                    let line_chars = self.virtual_layout.line_chars(line_idx);
                    let segment_range = self
                        .virtual_layout
                        .row_char_range(&self.virtual_editor_buffer, row_idx);
                    let segment_chars = segment_range.end.saturating_sub(segment_range.start);
                    let segment_start_in_line = segment_range.start.saturating_sub(line_start);
                    let line_start_byte =
                        self.virtual_editor_buffer.rope().char_to_byte(line_start);
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
                    let starts_line = row_in_line == 0;
                    let ends_line = row_in_line.saturating_add(1) >= line_visual_rows;
                    let render_line = options
                        .highlight_render_match
                        .and_then(|render| render.lines.get(line_idx));
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
                        self.virtual_editor_buffer.slice_chars_into(
                            segment_range.clone(),
                            &mut self.virtual_line_scratch,
                        );
                        // `Galley` retains `LayoutJob.text`, so each cache miss needs an owned
                        // per-row `String` anyway; move the scratch buffer into the job to avoid
                        // an extra clone/allocation on the miss path.
                        let mut job = build_virtual_line_segment_job_owned(
                            ui,
                            std::mem::take(&mut self.virtual_line_scratch),
                            editor_font,
                            render_line,
                            options.use_plain,
                            segment_start_byte..segment_end_byte,
                        );
                        job.wrap.max_width = f32::INFINITY;
                        let shaped = ui.fonts_mut(|f| f.layout_job(job));
                        debug_assert!(
                            shaped.rows.len() <= 1,
                            "virtual row segment produced wrapped galley"
                        );
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
                        virtual_row_hit_test_sense(),
                    );
                    let text_min_x = (rect.min.x + line_number_gutter + VIRTUAL_EDITOR_TEXT_INSET)
                        .min(rect.max.x);
                    let text_origin = egui::pos2(text_min_x, rect.min.y);
                    let text_rect = egui::Rect::from_min_max(text_origin, rect.max);
                    if response.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
                    }
                    let (primary_pressed_on_row, current_pointer_pos) = ui.input(|input| {
                        let pointer_pos = input
                            .pointer
                            .interact_pos()
                            .or_else(|| input.pointer.latest_pos());
                        let pressed_on_row =
                            input.pointer.button_pressed(egui::PointerButton::Primary)
                                && pointer_pos.map(|pos| rect.contains(pos)).unwrap_or(false);
                        (pressed_on_row, pointer_pos)
                    });
                    if pending_action.is_none()
                        && (response.drag_started() || response.clicked() || primary_pressed_on_row)
                    {
                        let pointer_pos = response.interact_pointer_pos().or(current_pointer_pos);
                        if let Some(pointer_pos) = pointer_pos {
                            let clamped_x = pointer_pos.x.clamp(text_rect.min.x, text_rect.max.x);
                            let clamped_y = pointer_pos.y.clamp(rect.min.y, rect.max.y);
                            let local_pos = egui::vec2(
                                (clamped_x - text_origin.x).max(0.0),
                                (clamped_y - text_origin.y).max(0.0),
                            );
                            let cursor = galley.cursor_from_pos(local_pos);
                            let local_col = cursor.index.min(segment_chars);
                            let global = segment_range.start.saturating_add(local_col);
                            if response.drag_started() {
                                self.reset_virtual_click_streak();
                                pending_action = Some(RowAction::DragStart { global });
                            } else if response.clicked() {
                                let click_count = self.register_virtual_click(pointer_pos);
                                match click_count {
                                    3 => {
                                        pending_action = Some(RowAction::Triple { line_idx });
                                    }
                                    2 => {
                                        pending_action = Some(RowAction::Double {
                                            line_idx,
                                            line_start,
                                            column_in_line: segment_start_in_line
                                                .saturating_add(local_col)
                                                .min(line_chars),
                                        });
                                    }
                                    _ => {
                                        pending_action = Some(RowAction::Click { global });
                                    }
                                }
                            } else {
                                pending_action = Some(RowAction::Click { global });
                            }
                        }
                    }
                    rows.push(RowRender {
                        line_idx,
                        segment_start: segment_range.start,
                        segment_chars,
                        starts_line,
                        ends_line,
                        rect,
                        text_rect,
                        text_origin,
                        galley,
                    });
                }

                if let Some(action) = pending_action {
                    self.virtual_editor_state.has_focus = true;
                    request_virtual_editor_focus(
                        ui,
                        editor_id,
                        editor_shortcuts_unblocked_for_frame,
                    );
                    focused = true;
                    editor_pointer_action_handled = true;
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
                            let word_bounds = {
                                self.virtual_editor_buffer.line_without_newline_into(
                                    line_idx,
                                    &mut self.virtual_line_scratch,
                                );
                                virtual_editor_double_click_selection_bounds(
                                    line_start,
                                    column_in_line,
                                    self.virtual_line_scratch.as_str(),
                                    |global| self.clamp_virtual_cursor_for_render(global),
                                )
                            };
                            if let Some((global_start, global_end)) = word_bounds {
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
                                let global = self.clamp_virtual_cursor_for_render(
                                    line_start.saturating_add(column_in_line),
                                );
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
                    ui.ctx().request_repaint();
                }

                let pointer_pos = ui.input(|input| {
                    input
                        .pointer
                        .interact_pos()
                        .or_else(|| input.pointer.latest_pos())
                });
                let pointer_down = ui.input(|input| input.pointer.primary_down());
                if pointer_down && self.virtual_drag_active {
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
                                pointer_pos
                                    .x
                                    .clamp(row.text_rect.min.x, row.text_rect.max.x),
                                pointer_pos.y.clamp(row.rect.min.y, row.rect.max.y),
                            );
                            let local_pos = egui::vec2(
                                (clamped_pos.x - row.text_origin.x).max(0.0),
                                (clamped_pos.y - row.text_origin.y).max(0.0),
                            );
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
                let clamped_caret_cursor =
                    self.clamp_virtual_cursor_for_render(self.virtual_editor_state.cursor());
                let paint_started = perf_enabled.then(Instant::now);
                for row in rows {
                    let galley = row.galley;
                    if let Some(selection) =
                        self.virtual_selection_for_line(row.segment_start, row.segment_chars)
                    {
                        paint_virtual_selection_overlay(
                            ui.painter(),
                            row.text_rect,
                            galley.as_ref(),
                            selection,
                            selection_fill,
                        );
                    }
                    if row.starts_line {
                        ui.painter().text(
                            egui::pos2(
                                row.text_rect.min.x - VIRTUAL_EDITOR_LINE_NUMBER_PADDING,
                                row.rect.center().y,
                            ),
                            egui::Align2::RIGHT_CENTER,
                            (row.line_idx.saturating_add(1)).to_string(),
                            line_number_font.clone(),
                            COLOR_TEXT_MUTED,
                        );
                    }
                    ui.painter()
                        .galley(row.text_origin, galley.clone(), ui.visuals().text_color());

                    if focused {
                        let cursor = clamped_caret_cursor;
                        let affinity = self.virtual_editor_state.wrap_boundary_affinity();
                        let segment_end = row.segment_start.saturating_add(row.segment_chars);
                        let at_row_start = cursor == row.segment_start;
                        let at_row_end = cursor == segment_end;
                        let shows_caret = if cursor < segment_end {
                            !(at_row_start
                                && !row.starts_line
                                && affinity == WrapBoundaryAffinity::Upstream)
                        } else {
                            at_row_end
                                && (row.ends_line || affinity == WrapBoundaryAffinity::Upstream)
                        };
                        if cursor >= row.segment_start && shows_caret {
                            let local_col = cursor.saturating_sub(row.segment_start);
                            let caret_rect = galley.pos_from_cursor(CCursor::new(local_col));
                            let x = (row.text_origin.x + caret_rect.min.x).max(row.text_origin.x);
                            let y_min = row.text_origin.y + caret_rect.min.y;
                            let mut y_max = row.text_origin.y + caret_rect.max.y;
                            if y_max <= y_min {
                                y_max = y_min + self.virtual_line_height.max(1.0);
                            }
                            let global_caret_rect = egui::Rect::from_min_max(
                                egui::pos2(x, y_min),
                                egui::pos2(x, y_max),
                            );
                            ime_cursor_rect = Some(global_caret_rect);
                            if caret_visible {
                                ui.painter().line_segment(
                                    [egui::pos2(x, y_min), egui::pos2(x, y_max)],
                                    Stroke::new(1.0, ui.visuals().text_color()),
                                );
                            }
                        }
                    }
                }
                if let Some(started) = paint_started {
                    paint_ms = started.elapsed().as_secs_f32() * 1000.0;
                }
            });
        let follow_requested = self.virtual_follow_cursor_next_frame;
        if follow_requested {
            self.virtual_follow_cursor_next_frame = false;
        }
        if had_focus && !self.virtual_drag_active {
            let cursor_row = self.virtual_cursor_row_index(self.virtual_editor_state.cursor());
            let viewport_rows =
                ((editor_height / self.virtual_line_height).floor().max(1.0)) as usize;
            if let Some(offset) = follow_cursor_scroll_offset_y(
                follow_requested,
                cursor_row,
                scroll_output.state.offset.y,
                viewport_rows,
                self.virtual_line_height,
            ) {
                self.virtual_pending_scroll_offset_y = Some(offset.max(0.0));
            }
        }
        // Include scrollbar gutter when classifying inside/outside editor clicks.
        // Scrollbar interaction should not be treated as an external blur.
        let interaction_rect = editor_interaction_rect(scroll_output.inner_rect, wrap_width);
        // Keep a focusable widget for this ID alive each frame so keyboard focus
        // persists between pointer interactions.
        let focus_response = ui.interact(
            interaction_rect,
            editor_id,
            egui::Sense::focusable_noninteractive(),
        );
        let mut egui_focus = ui.memory(|m| m.has_focus(editor_id));
        // Treat any primary click inside the editor viewport as an explicit focus
        // claim, even when no row hit-test action fired (e.g. empty space below
        // the last visual row).
        let primary_pressed =
            ui.input(|input| input.pointer.button_pressed(egui::PointerButton::Primary));
        let pointer_press_pos = ui.input(|input| {
            input
                .pointer
                .interact_pos()
                .or_else(|| input.pointer.latest_pos())
        });
        let clicked_inside_editor = pointer_press_pos
            .map(|pos| primary_pressed && interaction_rect.contains(pos))
            .unwrap_or(false);
        let clicked_inside_editor_content = pointer_press_pos
            .map(|pos| primary_pressed && scroll_output.inner_rect.contains(pos))
            .unwrap_or(false);
        if clicked_inside_editor {
            self.virtual_editor_state.has_focus = true;
            request_virtual_editor_focus(ui, editor_id, editor_shortcuts_unblocked_for_frame);
            egui_focus = true;
            if clicked_inside_editor_content && !editor_pointer_action_handled {
                let eof =
                    self.clamp_virtual_cursor_for_render(self.virtual_editor_buffer.len_chars());
                self.virtual_editor_state
                    .set_cursor(eof, self.virtual_editor_buffer.len_chars());
                self.virtual_editor_state.clear_preferred_column();
                self.reset_virtual_click_streak();
                self.reset_virtual_caret_blink();
                ui.ctx().request_repaint();
            }
        }
        let clicked_outside_editor = pointer_press_pos
            .map(|pos| primary_pressed && !interaction_rect.contains(pos))
            .unwrap_or(false);
        let explicit_blur = should_explicitly_blur_virtual_editor(
            clicked_outside_editor,
            options.preserve_focus_from_editor_chrome,
        );
        if explicit_blur {
            self.virtual_editor_state.has_focus = false;
            ui.memory_mut(|m| m.surrender_focus(editor_id));
            egui_focus = false;
        }
        if egui_focus {
            self.virtual_editor_state.has_focus = true;
        } else if self.virtual_editor_state.has_focus {
            request_virtual_editor_focus(ui, editor_id, editor_shortcuts_unblocked_for_frame);
            egui_focus = true;
        }
        if focus_editor_requested && egui_focus {
            self.focus_editor_next = false;
        }
        if focus_response.gained_focus() || (egui_focus && !had_focus) {
            self.reset_virtual_caret_blink();
            ui.ctx().request_repaint();
        }
        focused = egui_focus || self.virtual_editor_state.has_focus;
        let editor_shortcuts_available = focused && editor_shortcuts_unblocked_for_frame;
        if focused {
            // egui owns this filter and replaces it on focus changes; setting
            // it only while focused avoids stale ownership after blur.
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    editor_id,
                    virtual_editor_focus_lock_filter(editor_shortcuts_available),
                );
            });
            let cursor_rect = ime_cursor_rect.or_else(|| {
                let range = visible_row_range.as_ref()?;
                let cursor_row = self.virtual_cursor_row_index(self.virtual_editor_state.cursor());
                let row_offset = if cursor_row < range.start {
                    0
                } else if cursor_row >= range.end {
                    range.len().saturating_sub(1)
                } else {
                    cursor_row.saturating_sub(range.start)
                };
                let y_min = (interaction_rect.min.y + row_offset as f32 * self.virtual_line_height)
                    .clamp(
                        interaction_rect.min.y,
                        (interaction_rect.max.y - self.virtual_line_height)
                            .max(interaction_rect.min.y),
                    );
                let y_max = (y_min + self.virtual_line_height.max(1.0))
                    .min(interaction_rect.max.y)
                    .max(y_min + 1.0);
                let x = (interaction_rect.min.x + line_number_gutter + VIRTUAL_EDITOR_TEXT_INSET)
                    .clamp(interaction_rect.min.x, interaction_rect.max.x);
                Some(egui::Rect::from_min_max(
                    egui::pos2(x, y_min),
                    egui::pos2(x, y_max),
                ))
            });
            if let Some(cursor_rect) = cursor_rect {
                let to_global = ui
                    .ctx()
                    .layer_transform_to_global(ui.layer_id())
                    .unwrap_or_default();
                ui.output_mut(|output| {
                    output.ime = Some(egui::output::IMEOutput {
                        rect: to_global * interaction_rect,
                        cursor_rect: to_global * cursor_rect,
                    });
                });
            }
        }
        if editor_shortcuts_available {
            let route_started = Instant::now();
            let commands = ui.input(|input| {
                commands_from_events(&input.events, true)
                    .into_iter()
                    .filter(|command| !self.should_skip_virtual_command_for_paste_as_new(command))
                    .collect::<Vec<_>>()
            });
            let input_route_ms = route_started.elapsed().as_secs_f32() * 1000.0;
            self.nav_probe_record_applied_commands(&commands);
            consume_virtual_editor_owned_key_events(ui.ctx(), &commands);
            let apply_started = Instant::now();
            let apply_result = self.apply_virtual_commands(ui.ctx(), &commands);
            let apply_ms = apply_started.elapsed().as_secs_f32() * 1000.0;
            if !commands.is_empty() {
                self.virtual_editor_state.has_focus = true;
                focused = true;
            }
            if apply_result.pasted {
                self.virtual_paste_applied_this_frame = true;
            }
            if apply_result.changed {
                self.mark_dirty();
            }
            if apply_result.cursor_moved {
                let queued_follow_scroll = self.queue_virtual_cursor_follow_scroll(
                    scroll_output.state.offset.y,
                    editor_height,
                );
                self.virtual_follow_cursor_next_frame =
                    !queued_follow_scroll && apply_result.pasted;
            }
            if apply_result.changed || apply_result.cursor_moved {
                ui.ctx().request_repaint();
            }
            let selection_chars = self
                .virtual_editor_state
                .selection_range()
                .map(|range| range.end.saturating_sub(range.start))
                .unwrap_or(0);
            self.trace_input(InputTraceFrame {
                focus_active_pre: had_focus,
                focus_active_post: focused,
                egui_focus_pre: had_focus,
                egui_focus_post: focused,
                copy_ready_post: focused || selection_chars > 0,
                selection_chars,
                commands: &commands,
                apply_result,
            });
            self.trace_virtual_input_perf(
                &commands,
                VirtualInputPerfStats {
                    input_route_ms,
                    apply_ms,
                    apply_result,
                },
            );
            request_virtual_editor_focus(ui, editor_id, editor_shortcuts_unblocked_for_frame);
        }
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
