//! Native egui app skeleton for the LocalPaste rewrite.

mod editor;
mod highlight;
mod highlight_flow;
mod state_ops;
mod style;
mod ui;
mod util;
mod virtual_editor;
mod virtual_ops;
mod virtual_view;

use crate::backend::{spawn_backend, BackendHandle, PasteSummary};
use editor::{EditorBuffer, EditorLineIndex, EditorMode};
use eframe::egui::{self, text::CCursor, Color32, RichText, Stroke, TextStyle};
use egui_extras::syntax_highlighting::CodeTheme;
use highlight::{
    build_virtual_line_job, spawn_highlight_worker, syntect_language_hint, syntect_theme_key,
    EditorLayoutCache, HighlightRender, HighlightRequestMeta, HighlightWorker, SyntectSettings,
};
use localpaste_core::models::paste::Paste;
use localpaste_core::{Config, Database};
use localpaste_server::{AppState, EmbeddedServer, PasteLockManager};
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, Instant};
use style::*;
use tracing::{info, warn};
use util::{display_language_label, word_range_at};
use virtual_editor::{
    commands_from_events, RopeBuffer, VirtualEditorHistory, VirtualEditorState,
    VirtualInputCommand, WrapLayoutCache,
};
use virtual_view::{VirtualCursor, VirtualSelectionState};

/// Native egui application shell for the rewrite.
///
/// Owns the UI state and communicates with the background worker via channels so
/// the `update` loop never blocks on database I/O.
pub struct LocalPasteApp {
    backend: BackendHandle,
    pastes: Vec<PasteSummary>,
    selected_id: Option<String>,
    selected_paste: Option<Paste>,
    selected_content: EditorBuffer,
    editor_cache: EditorLayoutCache,
    editor_lines: EditorLineIndex,
    editor_mode: EditorMode,
    virtual_selection: VirtualSelectionState,
    virtual_editor_buffer: RopeBuffer,
    virtual_editor_state: VirtualEditorState,
    virtual_editor_history: VirtualEditorHistory,
    virtual_layout: WrapLayoutCache,
    virtual_drag_active: bool,
    virtual_viewport_height: f32,
    virtual_line_height: f32,
    virtual_wrap_width: f32,
    highlight_worker: HighlightWorker,
    highlight_pending: Option<HighlightRequestMeta>,
    highlight_render: Option<HighlightRender>,
    highlight_staged: Option<HighlightRender>,
    highlight_version: u64,
    last_interaction_at: Option<Instant>,
    last_editor_click_at: Option<Instant>,
    last_editor_click_pos: Option<egui::Pos2>,
    last_editor_click_count: u8,
    last_virtual_click_at: Option<Instant>,
    last_virtual_click_pos: Option<egui::Pos2>,
    last_virtual_click_line: Option<usize>,
    last_virtual_click_count: u8,
    virtual_editor_active: bool,
    syntect: SyntectSettings,
    db_path: String,
    locks: Arc<PasteLockManager>,
    _server: EmbeddedServer,
    server_addr: SocketAddr,
    server_used_fallback: bool,
    status: Option<StatusMessage>,
    save_status: SaveStatus,
    last_edit_at: Option<Instant>,
    save_in_flight: bool,
    autosave_delay: Duration,
    focus_editor_next: bool,
    style_applied: bool,
    window_checked: bool,
    last_refresh_at: Instant,
    perf_log_enabled: bool,
    frame_samples: VecDeque<f32>,
    last_frame_at: Option<Instant>,
    last_perf_log_at: Instant,
    editor_input_trace_enabled: bool,
    highlight_trace_enabled: bool,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum SaveStatus {
    Saved,
    Dirty,
    Saving,
}

const AUTO_REFRESH_INTERVAL: Duration = Duration::from_secs(3);
const STATUS_TTL: Duration = Duration::from_secs(5);
pub(crate) const DEFAULT_WINDOW_SIZE: [f32; 2] = [1100.0, 720.0];
pub(crate) const MIN_WINDOW_SIZE: [f32; 2] = [900.0, 600.0];
const HIGHLIGHT_PLAIN_THRESHOLD: usize = 256 * 1024;
const HIGHLIGHT_DEBOUNCE: Duration = Duration::from_millis(150);
const HIGHLIGHT_DEBOUNCE_MIN_BYTES: usize = 64 * 1024;
const HIGHLIGHT_APPLY_IDLE: Duration = Duration::from_millis(200);
const EDITOR_DOUBLE_CLICK_WINDOW: Duration = Duration::from_millis(300);
const EDITOR_DOUBLE_CLICK_DISTANCE: f32 = 8.0;
const VIRTUAL_EDITOR_ID: &str = "virtual_editor_input";
const VIRTUAL_OVERSCAN_LINES: usize = 3;
const PERF_LOG_INTERVAL: Duration = Duration::from_secs(2);
const PERF_SAMPLE_CAP: usize = 240;

struct StatusMessage {
    text: String,
    expires_at: Instant,
}

#[derive(Default, Debug, Clone, Copy)]
struct VirtualApplyResult {
    changed: bool,
    copied: bool,
    cut: bool,
    pasted: bool,
}

fn is_editor_word_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn env_flag_enabled(name: &str) -> bool {
    std::env::var(name)
        .map(|value| {
            let lowered = value.trim().to_ascii_lowercase();
            !(lowered.is_empty() || lowered == "0" || lowered == "false")
        })
        .unwrap_or(false)
}

fn next_virtual_click_count(
    last_at: Option<Instant>,
    last_pos: Option<egui::Pos2>,
    _last_line: Option<usize>,
    last_count: u8,
    _line_idx: usize,
    pointer_pos: egui::Pos2,
    now: Instant,
) -> u8 {
    let is_continuation = if let (Some(last_at), Some(last_pos)) = (last_at, last_pos) {
        now.duration_since(last_at) <= EDITOR_DOUBLE_CLICK_WINDOW
            && last_pos.distance(pointer_pos) <= EDITOR_DOUBLE_CLICK_DISTANCE
    } else {
        false
    };
    if is_continuation {
        last_count.saturating_add(1).min(3)
    } else {
        1
    }
}

fn paint_virtual_selection_overlay(
    painter: &egui::Painter,
    row_rect: egui::Rect,
    galley: &egui::Galley,
    selection: Range<usize>,
    selection_fill: Color32,
) {
    if selection.start >= selection.end {
        return;
    }
    let mut consumed = 0usize;
    for placed_row in &galley.rows {
        let row_chars = placed_row.char_count_excluding_newline();
        let local_start = selection.start.saturating_sub(consumed).min(row_chars);
        let local_end = selection.end.saturating_sub(consumed).min(row_chars);
        if local_end > local_start {
            let left = row_rect.min.x + placed_row.pos.x + placed_row.x_offset(local_start);
            let mut right = row_rect.min.x + placed_row.pos.x + placed_row.x_offset(local_end);
            if right <= left {
                right = left + 1.0;
            }
            let top = row_rect.min.y + placed_row.pos.y;
            let bottom = top + placed_row.height();
            let rect = egui::Rect::from_min_max(egui::pos2(left, top), egui::pos2(right, bottom));
            painter.rect_filled(rect, 2.0, selection_fill);
        }
        consumed = consumed.saturating_add(placed_row.char_count_including_newline());
        if consumed >= selection.end {
            break;
        }
    }
}

impl LocalPasteApp {
    /// Construct a new app instance from the current environment config.
    ///
    /// Opens the embedded database, spawns the backend worker thread, and kicks
    /// off the initial list request so the UI has data to render on first paint.
    ///
    /// # Returns
    /// The initialized [`LocalPasteApp`] ready to be handed to `eframe`.
    ///
    /// # Errors
    /// Returns an error if the database path is invalid or the underlying store
    /// cannot be opened.
    pub fn new() -> Result<Self, localpaste_core::AppError> {
        let config = Config::from_env();
        let db_path = config.db_path.clone();
        let autosave_delay = Duration::from_millis(config.auto_save_interval);
        let db = Database::new(&config.db_path)?;
        info!("native GUI opened database at {}", config.db_path);

        let locks = Arc::new(PasteLockManager::default());
        let server_db = db.share()?;
        let state = AppState::with_locks(config.clone(), server_db, locks.clone());
        let allow_public = std::env::var("ALLOW_PUBLIC_ACCESS").is_ok();
        if allow_public {
            warn!("Public access enabled - server will accept requests from any origin");
        }
        let server = EmbeddedServer::start(state, allow_public)?;
        let server_addr = server.addr();
        let server_used_fallback = server.used_fallback();

        let backend = spawn_backend(db);
        let highlight_worker = spawn_highlight_worker();

        let mut app = Self {
            backend,
            pastes: Vec::new(),
            selected_id: None,
            selected_paste: None,
            selected_content: EditorBuffer::new(String::new()),
            editor_cache: EditorLayoutCache::default(),
            editor_lines: EditorLineIndex::default(),
            editor_mode: EditorMode::from_env(),
            virtual_selection: VirtualSelectionState::default(),
            virtual_editor_buffer: RopeBuffer::new(""),
            virtual_editor_state: VirtualEditorState::default(),
            virtual_editor_history: VirtualEditorHistory::default(),
            virtual_layout: WrapLayoutCache::default(),
            virtual_drag_active: false,
            virtual_editor_active: false,
            virtual_viewport_height: 0.0,
            virtual_line_height: 1.0,
            virtual_wrap_width: 0.0,
            highlight_worker,
            highlight_pending: None,
            highlight_render: None,
            highlight_staged: None,
            highlight_version: 0,
            syntect: SyntectSettings::default(),
            db_path,
            locks,
            _server: server,
            server_addr,
            server_used_fallback,
            status: None,
            save_status: SaveStatus::Saved,
            last_edit_at: None,
            save_in_flight: false,
            autosave_delay,
            focus_editor_next: false,
            style_applied: false,
            window_checked: false,
            last_refresh_at: Instant::now(),
            perf_log_enabled: env_flag_enabled("LOCALPASTE_EDITOR_PERF_LOG"),
            frame_samples: VecDeque::with_capacity(PERF_SAMPLE_CAP),
            last_frame_at: None,
            last_perf_log_at: Instant::now(),
            last_interaction_at: None,
            last_editor_click_at: None,
            last_editor_click_pos: None,
            last_editor_click_count: 0,
            last_virtual_click_at: None,
            last_virtual_click_pos: None,
            last_virtual_click_line: None,
            last_virtual_click_count: 0,
            editor_input_trace_enabled: env_flag_enabled("LOCALPASTE_EDITOR_INPUT_TRACE"),
            highlight_trace_enabled: env_flag_enabled("LOCALPASTE_HIGHLIGHT_TRACE"),
        };
        app.request_refresh();
        Ok(app)
    }

    fn trace_input(
        &self,
        focused: bool,
        egui_focus: bool,
        selection_chars: usize,
        immediate: &[VirtualInputCommand],
        deferred: &[VirtualInputCommand],
        apply_result: VirtualApplyResult,
    ) {
        if !self.editor_input_trace_enabled {
            return;
        }
        info!(
            target: "localpaste_gui::input",
            mode = ?self.editor_mode,
            editor_active = self.virtual_editor_active,
            focused = focused,
            egui_focus = egui_focus,
            selection_chars = selection_chars,
            immediate = ?immediate,
            deferred = ?deferred,
            changed = apply_result.changed,
            copied = apply_result.copied,
            cut = apply_result.cut,
            pasted = apply_result.pasted,
            "virtual input frame"
        );
    }

    fn track_frame_metrics(&mut self) {
        let now = Instant::now();
        if let Some(last) = self.last_frame_at {
            let dt = now
                .saturating_duration_since(last)
                .as_secs_f32()
                .max(0.000_1);
            let frame_ms = dt * 1000.0;
            self.frame_samples.push_back(frame_ms);
            while self.frame_samples.len() > PERF_SAMPLE_CAP {
                self.frame_samples.pop_front();
            }
        }
        self.last_frame_at = Some(now);

        if !self.perf_log_enabled
            || now.saturating_duration_since(self.last_perf_log_at) < PERF_LOG_INTERVAL
        {
            return;
        }
        self.last_perf_log_at = now;
        if self.frame_samples.is_empty() {
            return;
        }
        let mut sorted: Vec<f32> = self.frame_samples.iter().copied().collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let avg_ms = sorted.iter().sum::<f32>() / sorted.len() as f32;
        let p95_idx = ((sorted.len() as f32 * 0.95).ceil() as usize).saturating_sub(1);
        let p95_ms = sorted.get(p95_idx).copied().unwrap_or(avg_ms);
        let fps = 1000.0 / avg_ms.max(0.001);
        info!(
            target: "localpaste_gui::perf",
            avg_fps = fps,
            p95_ms = p95_ms,
            samples = sorted.len(),
            "virtual editor frame stats"
        );
    }
}

impl eframe::App for LocalPasteApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ensure_style(ctx);
        self.track_frame_metrics();
        if !self.window_checked {
            let min_size = egui::vec2(MIN_WINDOW_SIZE[0], MIN_WINDOW_SIZE[1]);
            let current_size = ctx.input(|input| {
                input
                    .viewport()
                    .inner_rect
                    .map(|rect| rect.size())
                    .unwrap_or(min_size)
            });
            if current_size.x < min_size.x || current_size.y < min_size.y {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(min_size));
            }
            self.window_checked = true;
        }

        if let Some(status) = &self.status {
            if Instant::now() >= status.expires_at {
                self.status = None;
            }
        }

        while let Ok(event) = self.backend.evt_rx.try_recv() {
            self.apply_event(event);
        }

        while let Ok(render) = self.highlight_worker.rx.try_recv() {
            self.queue_highlight_render(render);
        }

        if !self.is_virtual_editor_mode() {
            self.virtual_editor_active = false;
        }

        let focus_id = egui::Id::new(VIRTUAL_EDITOR_ID);
        let egui_focus = ctx.memory(|m| m.has_focus(focus_id));
        let has_virtual_selection = self.virtual_editor_state.selection_range().is_some();
        let virtual_shortcut_active = self.is_virtual_editor_mode()
            && (self.virtual_editor_active
                || self.virtual_editor_state.has_focus
                || egui_focus
                || has_virtual_selection);
        let mut saw_virtual_select_all = false;
        let mut saw_virtual_copy = false;
        let mut saw_virtual_cut = false;
        let mut saw_virtual_undo = false;
        let mut saw_virtual_redo = false;
        let mut saw_virtual_paste = false;
        let mut immediate_virtual_commands: Vec<VirtualInputCommand> = Vec::new();
        let mut deferred_virtual_commands: Vec<VirtualInputCommand> = Vec::new();
        let mut immediate_apply_result = VirtualApplyResult::default();
        if self.is_virtual_editor_mode() {
            let commands = ctx.input(|input| commands_from_events(&input.events, true));
            for command in commands {
                match command {
                    VirtualInputCommand::Copy => {
                        saw_virtual_copy = true;
                        deferred_virtual_commands.push(VirtualInputCommand::Copy);
                    }
                    VirtualInputCommand::Cut => {
                        saw_virtual_cut = true;
                        deferred_virtual_commands.push(VirtualInputCommand::Cut);
                    }
                    VirtualInputCommand::Paste(text) => {
                        saw_virtual_paste = true;
                        deferred_virtual_commands.push(VirtualInputCommand::Paste(text));
                    }
                    other => {
                        if !virtual_shortcut_active {
                            continue;
                        }
                        match &other {
                            VirtualInputCommand::SelectAll => saw_virtual_select_all = true,
                            VirtualInputCommand::Undo => saw_virtual_undo = true,
                            VirtualInputCommand::Redo => saw_virtual_redo = true,
                            _ => {}
                        }
                        immediate_virtual_commands.push(other);
                    }
                }
            }
            immediate_apply_result = self.apply_virtual_commands(ctx, &immediate_virtual_commands);
            if immediate_apply_result.changed {
                self.mark_dirty();
            }
        }

        let mut copy_virtual_preview = false;
        let mut fallback_virtual_select_all = false;
        let mut fallback_virtual_copy = false;
        let mut fallback_virtual_cut = false;
        let mut fallback_virtual_undo = false;
        let mut fallback_virtual_redo = false;
        let mut request_virtual_paste = false;
        let mut pasted_text: Option<String> = None;
        let mut sidebar_direction: i32 = 0;
        let wants_keyboard_input_before = ctx.wants_keyboard_input();
        ctx.input(|input| {
            if !input.events.is_empty() || input.pointer.any_down() {
                self.last_interaction_at = Some(Instant::now());
            }
            if input.modifiers.command && input.key_pressed(egui::Key::N) {
                self.create_new_paste();
            }
            if input.modifiers.command && input.key_pressed(egui::Key::Delete) {
                self.delete_selected();
            }
            if input.modifiers.command && input.key_pressed(egui::Key::C) {
                match self.editor_mode {
                    EditorMode::VirtualPreview => copy_virtual_preview = true,
                    EditorMode::VirtualEditor => {
                        if virtual_shortcut_active && !saw_virtual_copy {
                            fallback_virtual_copy = true;
                        }
                    }
                    EditorMode::TextEdit => {}
                }
            }
            if self.editor_mode == EditorMode::VirtualEditor && input.modifiers.command {
                if virtual_shortcut_active
                    && input.key_pressed(egui::Key::A)
                    && !saw_virtual_select_all
                {
                    fallback_virtual_select_all = true;
                }
                if virtual_shortcut_active && input.key_pressed(egui::Key::X) && !saw_virtual_cut {
                    fallback_virtual_cut = true;
                }
                if virtual_shortcut_active && input.key_pressed(egui::Key::Z) {
                    if input.modifiers.shift {
                        if !saw_virtual_redo {
                            fallback_virtual_redo = true;
                        }
                    } else if !saw_virtual_undo {
                        fallback_virtual_undo = true;
                    }
                }
                if virtual_shortcut_active && input.key_pressed(egui::Key::Y) && !saw_virtual_redo {
                    fallback_virtual_redo = true;
                }
                if virtual_shortcut_active && input.key_pressed(egui::Key::V) && !saw_virtual_paste
                {
                    request_virtual_paste = true;
                }
            }
            for event in &input.events {
                if let egui::Event::Paste(text) = event {
                    pasted_text = Some(text.clone());
                }
            }
            if !wants_keyboard_input_before && !self.pastes.is_empty() {
                if input.key_pressed(egui::Key::ArrowDown) {
                    sidebar_direction = 1;
                } else if input.key_pressed(egui::Key::ArrowUp) {
                    sidebar_direction = -1;
                }
            }
        });
        if copy_virtual_preview
            && self.editor_mode == EditorMode::VirtualPreview
            && !ctx.wants_keyboard_input()
        {
            if let Some(selection) = self.virtual_selection_text() {
                ctx.send_cmd(egui::OutputCommand::CopyText(selection));
            }
        }
        if self.editor_mode == EditorMode::VirtualEditor {
            let mut fallback_commands = Vec::new();
            if fallback_virtual_select_all {
                fallback_commands.push(VirtualInputCommand::SelectAll);
            }
            if fallback_virtual_copy {
                deferred_virtual_commands.push(VirtualInputCommand::Copy);
            }
            if fallback_virtual_cut {
                deferred_virtual_commands.push(VirtualInputCommand::Cut);
            }
            if fallback_virtual_undo {
                fallback_commands.push(VirtualInputCommand::Undo);
            }
            if fallback_virtual_redo {
                fallback_commands.push(VirtualInputCommand::Redo);
            }
            if !fallback_commands.is_empty() {
                let fallback_result = self.apply_virtual_commands(ctx, &fallback_commands);
                immediate_apply_result.changed |= fallback_result.changed;
                immediate_apply_result.copied |= fallback_result.copied;
                immediate_apply_result.cut |= fallback_result.cut;
                immediate_apply_result.pasted |= fallback_result.pasted;
                if fallback_result.changed {
                    self.mark_dirty();
                }
            }
            if request_virtual_paste {
                ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
            }
        }

        if self.highlight_staged.is_some() {
            self.maybe_apply_staged_highlight(Instant::now());
        }

        if sidebar_direction != 0 {
            let current = self.selected_index().unwrap_or(0) as i32;
            let max_index = (self.pastes.len().saturating_sub(1)) as i32;
            let next = (current + sidebar_direction).clamp(0, max_index) as usize;
            if self.selected_index() != Some(next) {
                let next_id = self.pastes[next].id.clone();
                self.select_paste(next_id);
            }
        }
        self.render_top_bar(ctx);
        self.render_sidebar(ctx);
        self.render_editor_panel(ctx);

        let mut deferred_apply_result = VirtualApplyResult::default();
        let editor_active_now = self.editor_mode == EditorMode::VirtualEditor
            && (self.virtual_editor_active
                || self.virtual_editor_state.has_focus
                || ctx.memory(|m| m.has_focus(focus_id)));
        let virtual_clipboard_ready = editor_active_now
            || virtual_shortcut_active
            || self.virtual_editor_state.selection_range().is_some();
        if virtual_clipboard_ready {
            deferred_apply_result = self.apply_virtual_commands(ctx, &deferred_virtual_commands);
            if deferred_apply_result.changed {
                self.mark_dirty();
            }
        }
        let virtual_paste_consumed = immediate_apply_result.pasted || deferred_apply_result.pasted;
        if !ctx.wants_keyboard_input() && !virtual_clipboard_ready && !virtual_paste_consumed {
            if let Some(text) = pasted_text {
                if !text.trim().is_empty() {
                    self.create_new_paste_with_content(text);
                }
            }
        }
        let combined_apply = VirtualApplyResult {
            changed: immediate_apply_result.changed || deferred_apply_result.changed,
            copied: immediate_apply_result.copied || deferred_apply_result.copied,
            cut: immediate_apply_result.cut || deferred_apply_result.cut,
            pasted: virtual_paste_consumed,
        };
        let selection_chars = self
            .virtual_editor_state
            .selection_range()
            .map(|range| range.end.saturating_sub(range.start))
            .unwrap_or(0);
        let focused_now = self.virtual_editor_state.has_focus || editor_active_now;
        let egui_focus_now = ctx.memory(|m| m.has_focus(focus_id));
        self.trace_input(
            focused_now,
            egui_focus_now,
            selection_chars,
            &immediate_virtual_commands,
            &deferred_virtual_commands,
            combined_apply,
        );

        self.render_status_bar(ctx);

        self.maybe_autosave();
        if self.last_refresh_at.elapsed() >= AUTO_REFRESH_INTERVAL {
            self.request_refresh();
        }
        let mut repaint_after = if self.save_status == SaveStatus::Dirty {
            self.autosave_delay.min(AUTO_REFRESH_INTERVAL)
        } else {
            AUTO_REFRESH_INTERVAL
        };
        if let Some(status) = &self.status {
            let until = status.expires_at.saturating_duration_since(Instant::now());
            repaint_after = repaint_after.min(until);
        }
        ctx.request_repaint_after(repaint_after);
    }
}

impl Drop for LocalPasteApp {
    fn drop(&mut self) {
        if let Some(id) = self.selected_id.take() {
            self.locks.unlock(&id);
        }
    }
}

#[cfg(test)]
mod tests;
