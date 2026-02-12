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
    EditorLayoutCache, EditorLayoutRequest, HighlightRender, HighlightRequestMeta, HighlightWorker,
    SyntectSettings,
};
use localpaste_core::models::{folder::Folder, paste::Paste};
use localpaste_core::{Config, Database};
use localpaste_server::{AppState, EmbeddedServer, PasteLockManager};
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, Instant};
use style::*;
use tracing::{info, warn};
use util::{display_language_label, env_flag_enabled, word_range_at};
use virtual_editor::{
    commands_from_events, RopeBuffer, VirtualCommandRoute, VirtualEditorHistory,
    VirtualEditorState, VirtualInputCommand, WrapLayoutCache,
};
use virtual_view::{VirtualCursor, VirtualSelectionState};

/// Native egui application shell for the rewrite.
///
/// Owns the UI state and communicates with the background worker via channels so
/// the `update` loop never blocks on database I/O.
pub struct LocalPasteApp {
    backend: BackendHandle,
    all_pastes: Vec<PasteSummary>,
    pastes: Vec<PasteSummary>,
    folders: Vec<Folder>,
    selected_id: Option<String>,
    selected_paste: Option<Paste>,
    edit_name: String,
    edit_language: Option<String>,
    edit_language_is_manual: bool,
    edit_folder_id: Option<String>,
    edit_tags: String,
    metadata_dirty: bool,
    search_query: String,
    search_last_input_at: Option<Instant>,
    search_last_sent: String,
    search_focus_requested: bool,
    active_collection: SidebarCollection,
    active_language_filter: Option<String>,
    properties_drawer_open: bool,
    command_palette_open: bool,
    command_palette_query: String,
    command_palette_selected: usize,
    pending_copy_action: Option<PaletteCopyAction>,
    clipboard_outgoing: Option<String>,
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
    toasts: VecDeque<ToastMessage>,
    save_status: SaveStatus,
    last_edit_at: Option<Instant>,
    save_in_flight: bool,
    autosave_delay: Duration,
    shortcut_help_open: bool,
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum SidebarCollection {
    All,
    Today,
    Week,
    Recent,
    Unfiled,
    Code,
    Config,
    Logs,
    Links,
}

#[derive(Debug, Clone)]
enum PaletteCopyAction {
    Raw(String),
    Fenced(String),
}

const AUTO_REFRESH_INTERVAL: Duration = Duration::from_secs(3);
const STATUS_TTL: Duration = Duration::from_secs(5);
const TOAST_TTL: Duration = Duration::from_secs(4);
const TOAST_LIMIT: usize = 4;
pub(crate) const DEFAULT_WINDOW_SIZE: [f32; 2] = [1100.0, 720.0];
pub(crate) const MIN_WINDOW_SIZE: [f32; 2] = [900.0, 600.0];
const HIGHLIGHT_PLAIN_THRESHOLD: usize = 256 * 1024;
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(150);
const HIGHLIGHT_DEBOUNCE: Duration = Duration::from_millis(150);
const HIGHLIGHT_DEBOUNCE_MIN_BYTES: usize = 64 * 1024;
const HIGHLIGHT_APPLY_IDLE: Duration = Duration::from_millis(200);
const EDITOR_DOUBLE_CLICK_WINDOW: Duration = Duration::from_millis(300);
const EDITOR_DOUBLE_CLICK_DISTANCE: f32 = 8.0;
const DRAG_AUTOSCROLL_EDGE_DISTANCE: f32 = 24.0;
const DRAG_AUTOSCROLL_MIN_LINES_PER_FRAME: f32 = 0.5;
const DRAG_AUTOSCROLL_MAX_LINES_PER_FRAME: f32 = 2.5;
const VIRTUAL_EDITOR_ID: &str = "virtual_editor_input";
const SEARCH_INPUT_ID: &str = "sidebar_search_input";
const VIRTUAL_OVERSCAN_LINES: usize = 3;
const PERF_LOG_INTERVAL: Duration = Duration::from_secs(2);
const PERF_SAMPLE_CAP: usize = 240;

struct StatusMessage {
    text: String,
    expires_at: Instant,
}

struct ToastMessage {
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

struct InputTraceFrame<'a> {
    focus_active_pre: bool,
    focus_active_post: bool,
    egui_focus_pre: bool,
    egui_focus_post: bool,
    copy_ready_post: bool,
    selection_chars: usize,
    immediate_focus_commands: &'a [VirtualInputCommand],
    deferred_focus_commands: &'a [VirtualInputCommand],
    deferred_copy_commands: &'a [VirtualInputCommand],
    apply_result: VirtualApplyResult,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VirtualCommandBucket {
    ImmediateFocus,
    DeferredFocus,
    DeferredCopy,
}

fn is_editor_word_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
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

fn drag_autoscroll_delta(pointer_y: f32, top: f32, bottom: f32, line_height: f32) -> f32 {
    if !pointer_y.is_finite()
        || !top.is_finite()
        || !bottom.is_finite()
        || !line_height.is_finite()
        || line_height <= 0.0
        || bottom <= top
    {
        return 0.0;
    }

    let outside_distance = if pointer_y < top {
        top - pointer_y
    } else if pointer_y > bottom {
        pointer_y - bottom
    } else {
        return 0.0;
    };

    // Scale autoscroll speed with distance beyond the viewport edge.
    let edge_distance = (line_height * 2.0).max(DRAG_AUTOSCROLL_EDGE_DISTANCE);
    let lines_per_frame = (outside_distance / edge_distance).clamp(
        DRAG_AUTOSCROLL_MIN_LINES_PER_FRAME,
        DRAG_AUTOSCROLL_MAX_LINES_PER_FRAME,
    );
    let delta = line_height * lines_per_frame;

    if pointer_y < top {
        delta
    } else {
        -delta
    }
}

fn classify_virtual_command(
    command: &VirtualInputCommand,
    focus_active_pre: bool,
) -> VirtualCommandBucket {
    match command.route() {
        VirtualCommandRoute::CopyOnly => VirtualCommandBucket::DeferredCopy,
        VirtualCommandRoute::FocusRequired => {
            if command.requires_post_focus() || !focus_active_pre {
                VirtualCommandBucket::DeferredFocus
            } else {
                VirtualCommandBucket::ImmediateFocus
            }
        }
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
            all_pastes: Vec::new(),
            pastes: Vec::new(),
            folders: Vec::new(),
            selected_id: None,
            selected_paste: None,
            edit_name: String::new(),
            edit_language: None,
            edit_language_is_manual: false,
            edit_folder_id: None,
            edit_tags: String::new(),
            metadata_dirty: false,
            search_query: String::new(),
            search_last_input_at: None,
            search_last_sent: String::new(),
            search_focus_requested: false,
            active_collection: SidebarCollection::All,
            active_language_filter: None,
            properties_drawer_open: false,
            command_palette_open: false,
            command_palette_query: String::new(),
            command_palette_selected: 0,
            pending_copy_action: None,
            clipboard_outgoing: None,
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
            toasts: VecDeque::with_capacity(TOAST_LIMIT),
            save_status: SaveStatus::Saved,
            last_edit_at: None,
            save_in_flight: false,
            autosave_delay,
            shortcut_help_open: false,
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
        app.request_folder_refresh();
        Ok(app)
    }

    fn trace_input(&self, frame: InputTraceFrame<'_>) {
        if !self.editor_input_trace_enabled {
            return;
        }
        info!(
            target: "localpaste_gui::input",
            mode = ?self.editor_mode,
            editor_active = self.virtual_editor_active,
            focus_active_pre = frame.focus_active_pre,
            focus_active_post = frame.focus_active_post,
            egui_focus_pre = frame.egui_focus_pre,
            egui_focus_post = frame.egui_focus_post,
            copy_ready_post = frame.copy_ready_post,
            selection_chars = frame.selection_chars,
            immediate_focus_count = frame.immediate_focus_commands.len(),
            deferred_focus_count = frame.deferred_focus_commands.len(),
            deferred_copy_count = frame.deferred_copy_commands.len(),
            immediate_focus = ?frame.immediate_focus_commands,
            deferred_focus = ?frame.deferred_focus_commands,
            deferred_copy = ?frame.deferred_copy_commands,
            changed = frame.apply_result.changed,
            copied = frame.apply_result.copied,
            cut = frame.apply_result.cut,
            pasted = frame.apply_result.pasted,
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

        let now = Instant::now();
        if let Some(status) = &self.status {
            if now >= status.expires_at {
                self.status = None;
            }
        }
        while self
            .toasts
            .front()
            .map(|toast| now >= toast.expires_at)
            .unwrap_or(false)
        {
            self.toasts.pop_front();
        }

        while let Ok(event) = self.backend.evt_rx.try_recv() {
            self.apply_event(event);
        }

        if let Some(text) = self.clipboard_outgoing.take() {
            ctx.send_cmd(egui::OutputCommand::CopyText(text));
        }

        while let Ok(render) = self.highlight_worker.rx.try_recv() {
            self.queue_highlight_render(render);
        }

        if !self.is_virtual_editor_mode() {
            self.virtual_editor_active = false;
        }

        let focus_id = egui::Id::new(VIRTUAL_EDITOR_ID);
        let egui_focus_pre = ctx.memory(|m| m.has_focus(focus_id));
        let has_virtual_selection_pre = self.virtual_editor_state.selection_range().is_some();
        let focus_active_pre = self.is_virtual_editor_mode()
            && (self.virtual_editor_active
                || self.virtual_editor_state.has_focus
                || egui_focus_pre);
        let copy_ready_pre = focus_active_pre || has_virtual_selection_pre;
        let mut saw_virtual_select_all = false;
        let mut saw_virtual_copy = false;
        let mut saw_virtual_cut = false;
        let mut saw_virtual_undo = false;
        let mut saw_virtual_redo = false;
        let mut saw_virtual_paste = false;
        let mut immediate_focus_commands: Vec<VirtualInputCommand> = Vec::new();
        let mut deferred_focus_commands: Vec<VirtualInputCommand> = Vec::new();
        let mut deferred_copy_commands: Vec<VirtualInputCommand> = Vec::new();
        let mut immediate_apply_result = VirtualApplyResult::default();
        if self.is_virtual_editor_mode() {
            let commands = ctx.input(|input| commands_from_events(&input.events, true));
            for command in commands {
                match classify_virtual_command(&command, focus_active_pre) {
                    VirtualCommandBucket::DeferredCopy => {
                        saw_virtual_copy = true;
                        deferred_copy_commands.push(command);
                    }
                    VirtualCommandBucket::DeferredFocus => {
                        match &command {
                            VirtualInputCommand::SelectAll => saw_virtual_select_all = true,
                            VirtualInputCommand::Cut => saw_virtual_cut = true,
                            VirtualInputCommand::Paste(_) => saw_virtual_paste = true,
                            VirtualInputCommand::Undo => saw_virtual_undo = true,
                            VirtualInputCommand::Redo => saw_virtual_redo = true,
                            _ => {}
                        }
                        deferred_focus_commands.push(command);
                    }
                    VirtualCommandBucket::ImmediateFocus => {
                        match &command {
                            VirtualInputCommand::SelectAll => saw_virtual_select_all = true,
                            VirtualInputCommand::Undo => saw_virtual_undo = true,
                            VirtualInputCommand::Redo => saw_virtual_redo = true,
                            _ => {}
                        }
                        immediate_focus_commands.push(command);
                    }
                }
            }
            immediate_apply_result = self.apply_virtual_commands(ctx, &immediate_focus_commands);
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
            if input.modifiers.command && input.key_pressed(egui::Key::S) {
                self.save_now();
                self.save_metadata_now();
            }
            if input.modifiers.command && input.key_pressed(egui::Key::F) {
                self.search_focus_requested = true;
            }
            if input.modifiers.command && input.key_pressed(egui::Key::K) {
                self.command_palette_open = !self.command_palette_open;
                self.command_palette_query.clear();
                self.command_palette_selected = 0;
            }
            if input.modifiers.command
                && (input.key_pressed(egui::Key::I) || input.key_pressed(egui::Key::P))
            {
                self.properties_drawer_open = !self.properties_drawer_open;
            }
            if input.key_pressed(egui::Key::F1) {
                self.shortcut_help_open = !self.shortcut_help_open;
            }
            if input.modifiers.command && input.key_pressed(egui::Key::C) {
                match self.editor_mode {
                    EditorMode::VirtualPreview => copy_virtual_preview = true,
                    EditorMode::VirtualEditor => {
                        if copy_ready_pre && !saw_virtual_copy {
                            fallback_virtual_copy = true;
                        }
                    }
                    EditorMode::TextEdit => {}
                }
            }
            if self.editor_mode == EditorMode::VirtualEditor && input.modifiers.command {
                if focus_active_pre && input.key_pressed(egui::Key::A) && !saw_virtual_select_all {
                    fallback_virtual_select_all = true;
                }
                if focus_active_pre && input.key_pressed(egui::Key::X) && !saw_virtual_cut {
                    fallback_virtual_cut = true;
                }
                if focus_active_pre && input.key_pressed(egui::Key::Z) {
                    if input.modifiers.shift {
                        if !saw_virtual_redo {
                            fallback_virtual_redo = true;
                        }
                    } else if !saw_virtual_undo {
                        fallback_virtual_undo = true;
                    }
                }
                if focus_active_pre && input.key_pressed(egui::Key::Y) && !saw_virtual_redo {
                    fallback_virtual_redo = true;
                }
                if focus_active_pre && input.key_pressed(egui::Key::V) && !saw_virtual_paste {
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
                deferred_copy_commands.push(VirtualInputCommand::Copy);
            }
            if fallback_virtual_cut {
                deferred_focus_commands.push(VirtualInputCommand::Cut);
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
        self.render_properties_drawer(ctx);
        self.render_editor_panel(ctx);
        self.render_command_palette(ctx);
        self.render_shortcut_help(ctx);

        let mut deferred_focus_apply_result = VirtualApplyResult::default();
        let mut deferred_copy_apply_result = VirtualApplyResult::default();
        let has_virtual_selection_post = self.virtual_editor_state.selection_range().is_some();
        let focus_active_post = self.editor_mode == EditorMode::VirtualEditor
            && (self.virtual_editor_active
                || self.virtual_editor_state.has_focus
                || ctx.memory(|m| m.has_focus(focus_id)));
        let copy_ready_post = focus_active_post || has_virtual_selection_post;
        if focus_active_post {
            deferred_focus_apply_result =
                self.apply_virtual_commands(ctx, &deferred_focus_commands);
            if deferred_focus_apply_result.changed {
                self.mark_dirty();
            }
        }
        if copy_ready_post {
            deferred_copy_apply_result = self.apply_virtual_commands(ctx, &deferred_copy_commands);
            if deferred_copy_apply_result.changed {
                self.mark_dirty();
            }
        }
        let virtual_paste_consumed = immediate_apply_result.pasted
            || deferred_focus_apply_result.pasted
            || deferred_copy_apply_result.pasted;
        if !ctx.wants_keyboard_input() && !focus_active_post && !virtual_paste_consumed {
            if let Some(text) = pasted_text {
                if !text.trim().is_empty() {
                    self.create_new_paste_with_content(text);
                }
            }
        }
        let combined_apply = VirtualApplyResult {
            changed: immediate_apply_result.changed
                || deferred_focus_apply_result.changed
                || deferred_copy_apply_result.changed,
            copied: immediate_apply_result.copied
                || deferred_focus_apply_result.copied
                || deferred_copy_apply_result.copied,
            cut: immediate_apply_result.cut
                || deferred_focus_apply_result.cut
                || deferred_copy_apply_result.cut,
            pasted: virtual_paste_consumed,
        };
        let selection_chars = self
            .virtual_editor_state
            .selection_range()
            .map(|range| range.end.saturating_sub(range.start))
            .unwrap_or(0);
        let egui_focus_post = ctx.memory(|m| m.has_focus(focus_id));
        self.trace_input(InputTraceFrame {
            focus_active_pre,
            focus_active_post,
            egui_focus_pre,
            egui_focus_post,
            copy_ready_post,
            selection_chars,
            immediate_focus_commands: &immediate_focus_commands,
            deferred_focus_commands: &deferred_focus_commands,
            deferred_copy_commands: &deferred_copy_commands,
            apply_result: combined_apply,
        });

        self.render_status_bar(ctx);
        self.render_toasts(ctx);

        self.maybe_dispatch_search();
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
        if let Some(toast) = self.toasts.front() {
            let until = toast.expires_at.saturating_duration_since(Instant::now());
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
