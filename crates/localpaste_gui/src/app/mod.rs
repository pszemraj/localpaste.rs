//! Native egui app skeleton for the LocalPaste rewrite.

mod editor;
mod highlight;
mod highlight_flow;
mod interaction_helpers;
mod paste_intent;
mod perf_trace;
mod shutdown;
mod state_accessors;
mod state_feedback;
mod state_ops;
mod style;
mod text_coords;
mod ui;
mod util;
mod virtual_editor;
mod virtual_ops;
mod virtual_ops_apply;
mod virtual_ops_click;
mod virtual_view;

use crate::backend::{spawn_backend_with_locks_and_owner, BackendHandle, PasteSummary};
use editor::{EditorBuffer, EditorLineIndex, EditorMode};
use eframe::egui::{self, text::CCursor, RichText, Stroke, TextStyle};
use egui_extras::syntax_highlighting::CodeTheme;
use highlight::{
    build_virtual_line_job, build_virtual_line_segment_job_owned, spawn_highlight_worker,
    syntect_language_hint, syntect_theme_key, EditorLayoutCache, EditorLayoutRequest,
    HighlightRender, HighlightRequestMeta, HighlightRequestText, HighlightWorker,
    HighlightWorkerResult, SyntectSettings, VirtualEditHint,
};
pub(super) use interaction_helpers::{
    classify_virtual_command, drag_autoscroll_delta, is_command_shift_shortcut,
    is_editor_word_char, is_plain_command_shortcut, next_virtual_click_count,
    paint_virtual_selection_overlay, should_route_sidebar_arrows, VirtualCommandBucket,
};
use localpaste_core::models::paste::Paste;
use localpaste_core::{Config, Database};
use localpaste_server::{AppState, EmbeddedServer, LockOwnerId, PasteLockManager};
use perf_trace::VirtualInputPerfStats;
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::ops::Range;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};
use style::*;
use tracing::{info, warn};
use util::{display_language_label, env_flag_enabled, word_range_at};
use virtual_editor::{
    commands_from_events, RopeBuffer, VirtualCommandRoute, VirtualEditorHistory,
    VirtualEditorState, VirtualGalleyCache, VirtualGalleyContext, VirtualInputCommand,
    WrapBoundaryAffinity, WrapLayoutCache,
};
use virtual_view::{VirtualCursor, VirtualSelectionState};

/// Native egui application shell for the rewrite.
///
/// Owns the UI state and communicates with the background worker via channels so
/// the `update` loop never blocks on database I/O.
pub(crate) struct LocalPasteApp {
    backend: BackendHandle,
    all_pastes: Vec<PasteSummary>,
    pastes: Vec<PasteSummary>,
    selected_id: Option<String>,
    selected_paste: Option<Paste>,
    edit_name: String,
    edit_language: Option<String>,
    edit_language_is_manual: bool,
    edit_tags: String,
    metadata_dirty: bool,
    metadata_save_in_flight: bool,
    metadata_save_request: Option<MetadataDraftSnapshot>,
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
    palette_search_results: Vec<PasteSummary>,
    palette_search_last_sent: String,
    palette_search_last_input_at: Option<Instant>,
    pending_copy_action: Option<PaletteCopyAction>,
    pending_selection_id: Option<String>,
    clipboard_outgoing: Option<String>,
    selected_content: EditorBuffer,
    editor_cache: EditorLayoutCache,
    editor_lines: EditorLineIndex,
    text_editor_has_focus: bool,
    text_editor_focus_id: Option<egui::Id>,
    editor_mode: EditorMode,
    virtual_selection: VirtualSelectionState,
    virtual_editor_buffer: RopeBuffer,
    virtual_editor_state: VirtualEditorState,
    virtual_editor_history: VirtualEditorHistory,
    virtual_layout: WrapLayoutCache,
    virtual_galley_cache: VirtualGalleyCache,
    virtual_line_scratch: String,
    virtual_caret_phase_start: Instant,
    virtual_drag_active: bool,
    virtual_viewport_height: f32,
    virtual_line_height: f32,
    virtual_wrap_width: f32,
    highlight_worker: HighlightWorker,
    highlight_pending: Option<HighlightRequestMeta>,
    highlight_render: Option<HighlightRender>,
    highlight_staged: Option<HighlightRender>,
    highlight_staged_invalidation: Option<StagedHighlightInvalidation>,
    highlight_version: u64,
    highlight_edit_hint: Option<VirtualEditHint>,
    last_interaction_at: Option<Instant>,
    last_editor_click_at: Option<Instant>,
    last_editor_click_pos: Option<egui::Pos2>,
    last_editor_click_count: u8,
    last_virtual_click_at: Option<Instant>,
    last_virtual_click_pos: Option<egui::Pos2>,
    last_virtual_click_line: Option<usize>,
    last_virtual_click_count: u8,
    virtual_editor_active: bool,
    paste_as_new_pending_frames: u8,
    paste_as_new_clipboard_requested_at: Option<Instant>,
    syntect: SyntectSettings,
    db_path: String,
    locks: Arc<PasteLockManager>,
    lock_owner_id: LockOwnerId,
    _server: EmbeddedServer,
    server_addr: SocketAddr,
    server_used_fallback: bool,
    status: Option<StatusMessage>,
    toasts: VecDeque<ToastMessage>,
    export_result_rx: Option<mpsc::Receiver<ExportCompletion>>,
    save_status: SaveStatus,
    last_edit_at: Option<Instant>,
    save_in_flight: bool,
    save_request_revision: Option<u64>,
    autosave_delay: Duration,
    shortcut_help_open: bool,
    focus_editor_next: bool,
    style_applied: bool,
    window_checked: bool,
    last_refresh_at: Instant,
    query_perf: QueryPerfCounters,
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
#[derive(Clone, Debug)]
struct StagedHighlightInvalidation {
    base_revision: u64,
    base_text_len: usize,
    line_ranges: Vec<Range<usize>>,
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
#[doc = "Default initial window size for native GUI startup."]
pub(crate) const DEFAULT_WINDOW_SIZE: [f32; 2] = [1100.0, 720.0];
#[doc = "Minimum enforced window size to keep sidebar/editor controls usable."]
pub(crate) const MIN_WINDOW_SIZE: [f32; 2] = [900.0, 600.0];
const HIGHLIGHT_PLAIN_THRESHOLD: usize = 256 * 1024;
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(150);
const PALETTE_SEARCH_LIMIT: usize = 40;
#[doc = "Per-line render cap used by preview/virtual editor galleys."]
pub(crate) const MAX_RENDER_CHARS_PER_LINE: usize = 10_000;
const HIGHLIGHT_DEBOUNCE_MEDIUM: Duration = Duration::from_millis(35);
const HIGHLIGHT_DEBOUNCE_LARGE: Duration = Duration::from_millis(50);
const HIGHLIGHT_DEBOUNCE_TINY: Duration = Duration::from_millis(15);
const HIGHLIGHT_DEBOUNCE_LARGE_BYTES: usize = 64 * 1024;
const HIGHLIGHT_TINY_EDIT_MAX_CHARS: usize = 4;
const HIGHLIGHT_DEBOUNCE_MIN_BYTES: usize = 64 * 1024;
const HIGHLIGHT_APPLY_IDLE: Duration = Duration::from_millis(200);
const EDITOR_DOUBLE_CLICK_WINDOW: Duration = Duration::from_millis(300);
const EDITOR_DOUBLE_CLICK_DISTANCE: f32 = 8.0;
const DRAG_AUTOSCROLL_EDGE_DISTANCE: f32 = 24.0;
const DRAG_AUTOSCROLL_MIN_LINES_PER_FRAME: f32 = 0.5;
const DRAG_AUTOSCROLL_MAX_LINES_PER_FRAME: f32 = 2.5;
const CARET_BLINK_INTERVAL: Duration = Duration::from_millis(530);
const SHUTDOWN_SAVE_FLUSH_TIMEOUT: Duration = Duration::from_secs(2);
const VIRTUAL_EDITOR_ID: &str = "virtual_editor_input";
const TEXT_EDITOR_ID: &str = "text_editor_input";
const SEARCH_INPUT_ID: &str = "sidebar_search_input";
const PERF_LOG_INTERVAL: Duration = Duration::from_secs(2);
const PERF_SAMPLE_CAP: usize = 240;
const PASTE_AS_NEW_PENDING_TTL_FRAMES: u8 = 3;
const PASTE_AS_NEW_CLIPBOARD_WAIT_TIMEOUT: Duration = Duration::from_secs(2);

struct StatusMessage {
    text: String,
    expires_at: Instant,
}

struct ToastMessage {
    text: String,
    expires_at: Instant,
}

struct ExportCompletion {
    paste_id: String,
    path: String,
    result: Result<(), String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MetadataDraftSnapshot {
    name: String,
    language: Option<String>,
    language_is_manual: bool,
    tags_csv: String,
}

#[derive(Default, Debug, Clone, Copy)]
struct VirtualApplyResult {
    changed: bool,
    copied: bool,
    cut: bool,
    pasted: bool,
}

#[derive(Default, Debug, Clone)]
struct QueryPerfCounters {
    list_requests_sent: u64,
    list_results_applied: u64,
    list_last_roundtrip_ms: Option<f32>,
    search_requests_sent: u64,
    search_results_applied: u64,
    search_stale_drops: u64,
    search_skipped_cached: u64,
    search_skipped_debounce: u64,
    search_last_roundtrip_ms: Option<f32>,
    list_last_sent_at: Option<Instant>,
    search_last_sent_at: Option<Instant>,
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
    pub(crate) fn new() -> Result<Self, localpaste_core::AppError> {
        let config = Config::from_env();
        let db_path = config.db_path.clone();
        let autosave_delay = Duration::from_millis(config.auto_save_interval);
        let db = Database::new(&config.db_path)?;
        info!("native GUI opened database at {}", config.db_path);

        let locks = Arc::new(PasteLockManager::default());
        let server_db = db.share()?;
        let state = AppState::with_locks(config.clone(), server_db, locks.clone());
        let allow_public = localpaste_core::config::env_flag_enabled("ALLOW_PUBLIC_ACCESS");
        if allow_public {
            warn!("Public access enabled - server will accept requests from any origin");
        }
        let server = EmbeddedServer::start(state, allow_public)?;
        let server_addr = server.addr();
        let server_used_fallback = server.used_fallback();

        let lock_owner_id = crate::lock_owner::next_lock_owner_id("gui");
        let backend = spawn_backend_with_locks_and_owner(
            db,
            config.max_paste_size,
            locks.clone(),
            lock_owner_id.clone(),
        );
        let highlight_worker = spawn_highlight_worker();

        let mut app = Self {
            backend,
            all_pastes: Vec::new(),
            pastes: Vec::new(),
            selected_id: None,
            selected_paste: None,
            edit_name: String::new(),
            edit_language: None,
            edit_language_is_manual: false,
            edit_tags: String::new(),
            metadata_dirty: false,
            metadata_save_in_flight: false,
            metadata_save_request: None,
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
            palette_search_results: Vec::new(),
            palette_search_last_sent: String::new(),
            palette_search_last_input_at: None,
            pending_copy_action: None,
            pending_selection_id: None,
            clipboard_outgoing: None,
            selected_content: EditorBuffer::new(String::new()),
            editor_cache: EditorLayoutCache::default(),
            editor_lines: EditorLineIndex::default(),
            text_editor_has_focus: false,
            text_editor_focus_id: None,
            editor_mode: EditorMode::from_env(),
            virtual_selection: VirtualSelectionState::default(),
            virtual_editor_buffer: RopeBuffer::new(""),
            virtual_editor_state: VirtualEditorState::default(),
            virtual_editor_history: VirtualEditorHistory::default(),
            virtual_layout: WrapLayoutCache::default(),
            virtual_galley_cache: VirtualGalleyCache::default(),
            virtual_line_scratch: String::new(),
            virtual_caret_phase_start: Instant::now(),
            virtual_drag_active: false,
            virtual_editor_active: false,
            virtual_viewport_height: 0.0,
            virtual_line_height: 1.0,
            virtual_wrap_width: 0.0,
            highlight_worker,
            highlight_pending: None,
            highlight_render: None,
            highlight_staged: None,
            highlight_staged_invalidation: None,
            highlight_version: 0,
            highlight_edit_hint: None,
            syntect: SyntectSettings::default(),
            db_path,
            locks,
            lock_owner_id,
            _server: server,
            server_addr,
            server_used_fallback,
            status: None,
            toasts: VecDeque::with_capacity(TOAST_LIMIT),
            export_result_rx: None,
            save_status: SaveStatus::Saved,
            last_edit_at: None,
            save_in_flight: false,
            save_request_revision: None,
            autosave_delay,
            shortcut_help_open: false,
            focus_editor_next: false,
            style_applied: false,
            window_checked: false,
            last_refresh_at: Instant::now(),
            query_perf: QueryPerfCounters::default(),
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
            paste_as_new_pending_frames: 0,
            paste_as_new_clipboard_requested_at: None,
            editor_input_trace_enabled: env_flag_enabled("LOCALPASTE_EDITOR_INPUT_TRACE"),
            highlight_trace_enabled: env_flag_enabled("LOCALPASTE_HIGHLIGHT_TRACE"),
        };
        app.request_refresh();
        Ok(app)
    }

    fn acquire_paste_lock(&mut self, id: &str) -> bool {
        match self.locks.acquire(id, &self.lock_owner_id) {
            Ok(()) => true,
            Err(err) => {
                warn!(
                    "failed to acquire paste lock '{}' for GUI owner: {}",
                    id, err
                );
                self.set_status("Lock acquire failed; close and reopen the paste.");
                false
            }
        }
    }

    fn release_paste_lock(&mut self, id: &str) {
        if let Err(err) = self.locks.release(id, &self.lock_owner_id) {
            warn!(
                "failed to release paste lock '{}' for GUI owner: {}",
                id, err
            );
            self.set_status("Lock release failed; restart app if edits remain blocked.");
        }
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
        let p50_idx = (sorted.len().saturating_sub(1)) / 2;
        let p95_idx = ((sorted.len() as f32 * 0.95).ceil() as usize).saturating_sub(1);
        let p99_idx = ((sorted.len() as f32 * 0.99).ceil() as usize).saturating_sub(1);
        let p50_ms = sorted.get(p50_idx).copied().unwrap_or(avg_ms);
        let p95_ms = sorted.get(p95_idx).copied().unwrap_or(avg_ms);
        let p99_ms = sorted.get(p99_idx).copied().unwrap_or(avg_ms);
        let worst_ms = sorted.last().copied().unwrap_or(avg_ms);
        let slow_frames = sorted.iter().filter(|value| **value > 16.7).count();
        let fps = 1000.0 / avg_ms.max(0.001);
        let history = self.virtual_editor_history.perf_stats();
        info!(
            target: "localpaste_gui::perf",
            avg_fps = fps,
            avg_ms = avg_ms,
            p50_ms = p50_ms,
            p95_ms = p95_ms,
            p99_ms = p99_ms,
            worst_ms = worst_ms,
            slow_frames = slow_frames,
            samples = sorted.len(),
            list_sent = self.query_perf.list_requests_sent,
            list_applied = self.query_perf.list_results_applied,
            list_last_ms = self.query_perf.list_last_roundtrip_ms.unwrap_or(0.0),
            search_sent = self.query_perf.search_requests_sent,
            search_applied = self.query_perf.search_results_applied,
            search_stale_drops = self.query_perf.search_stale_drops,
            search_skipped_cached = self.query_perf.search_skipped_cached,
            search_skipped_debounce = self.query_perf.search_skipped_debounce,
            search_last_ms = self.query_perf.search_last_roundtrip_ms.unwrap_or(0.0),
            undo_len = history.undo_len,
            redo_len = history.redo_len,
            undo_bytes = history.undo_bytes,
            redo_invalidations = history.redo_invalidations,
            redo_hits = history.redo_hits,
            redo_misses = history.redo_misses,
            coalesced_edits = history.coalesced_edits,
            trim_evictions = history.trim_evictions,
            "local perf snapshot"
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
        self.poll_export_result();

        if let Some(text) = self.clipboard_outgoing.take() {
            ctx.send_cmd(egui::OutputCommand::CopyText(text));
        }

        while let Ok(result) = self.highlight_worker.rx.try_recv() {
            match result {
                HighlightWorkerResult::Render(render) => self.queue_highlight_render(render),
                HighlightWorkerResult::Patch(patch) => self.queue_highlight_patch(patch),
            }
        }

        if !self.is_virtual_editor_mode() {
            self.virtual_editor_active = false;
        }

        let focus_id = egui::Id::new(VIRTUAL_EDITOR_ID);
        let egui_focus_pre = ctx.memory(|m| m.has_focus(focus_id));
        let has_virtual_selection_pre = self.virtual_editor_state.selection_range().is_some();
        let focus_active_pre = self.is_virtual_editor_mode()
            && (self.virtual_editor_state.has_focus || egui_focus_pre);
        let copy_ready_pre = focus_active_pre || has_virtual_selection_pre;
        let explicit_paste_as_new_shortcut_pressed =
            self.maybe_arm_paste_as_new_shortcut_intent(ctx);
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
        let mut input_route_ms = 0.0f32;
        let mut immediate_apply_ms = 0.0f32;
        let mut deferred_focus_apply_ms = 0.0f32;
        let mut deferred_copy_apply_ms = 0.0f32;
        if self.is_virtual_editor_mode() {
            let route_started = Instant::now();
            let commands = ctx.input(|input| commands_from_events(&input.events, true));
            input_route_ms = route_started.elapsed().as_secs_f32() * 1000.0;
            for command in commands {
                if self.should_skip_virtual_command_for_paste_as_new(&command) {
                    continue;
                }
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
            let immediate_started = Instant::now();
            immediate_apply_result = self.apply_virtual_commands(ctx, &immediate_focus_commands);
            immediate_apply_ms += immediate_started.elapsed().as_secs_f32() * 1000.0;
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
        let mut request_paste_as_new = explicit_paste_as_new_shortcut_pressed;
        let mut plain_paste_shortcut_pressed = false;
        let mut pasted_text: Option<String> = None;
        let mut sidebar_direction: i32 = 0;
        let focus_promotion_requested =
            self.editor_mode == EditorMode::VirtualEditor && self.focus_editor_next;
        let wants_keyboard_input_before = ctx.wants_keyboard_input();
        let virtual_editor_focus_active_pre = focus_active_pre
            || (self.editor_mode == EditorMode::VirtualEditor && self.virtual_editor_active);
        ctx.input(|input| {
            if !input.events.is_empty() || input.pointer.any_down() {
                self.last_interaction_at = Some(Instant::now());
            }
            let plain_command = is_plain_command_shortcut(input.modifiers);
            let command_shift = is_command_shift_shortcut(input.modifiers);

            if plain_command && input.key_pressed(egui::Key::N) {
                self.create_new_paste();
            }
            if plain_command
                && input.key_pressed(egui::Key::Delete)
                && self.should_route_delete_selected_shortcut(
                    wants_keyboard_input_before,
                    virtual_editor_focus_active_pre,
                )
            {
                self.delete_selected();
            }
            if plain_command && input.key_pressed(egui::Key::S) {
                self.save_now();
                self.save_metadata_now();
            }
            if plain_command && input.key_pressed(egui::Key::F) {
                self.search_focus_requested = true;
            }
            if (plain_command && input.key_pressed(egui::Key::K))
                || (command_shift && input.key_pressed(egui::Key::P))
            {
                self.command_palette_open = !self.command_palette_open;
                self.command_palette_query.clear();
                self.command_palette_selected = 0;
                self.palette_search_results.clear();
                self.palette_search_last_sent.clear();
                self.palette_search_last_input_at = None;
            }
            if plain_command && input.key_pressed(egui::Key::I) {
                self.properties_drawer_open = !self.properties_drawer_open;
            }
            if command_shift && input.key_pressed(egui::Key::V) {
                request_paste_as_new = true;
            }
            if plain_command && input.key_pressed(egui::Key::V) {
                // A newer plain paste intent should take precedence over any older
                // explicit paste-as-new intent still waiting on clipboard payload.
                self.cancel_paste_as_new_intent();
                plain_paste_shortcut_pressed = true;
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
            }
            for event in &input.events {
                if let egui::Event::Paste(text) = event {
                    Self::merge_pasted_text(&mut pasted_text, text.as_str());
                }
            }
            if should_route_sidebar_arrows(
                wants_keyboard_input_before,
                input.modifiers,
                !self.pastes.is_empty(),
                focus_active_pre,
                self.command_palette_open,
                self.properties_drawer_open,
                self.shortcut_help_open,
            ) {
                if input.key_pressed(egui::Key::ArrowDown) {
                    sidebar_direction = 1;
                } else if input.key_pressed(egui::Key::ArrowUp) {
                    sidebar_direction = -1;
                }
            }
        });
        if request_paste_as_new {
            self.prepare_text_editor_for_paste_as_new(ctx);
        }
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
                let fallback_started = Instant::now();
                let fallback_result = self.apply_virtual_commands(ctx, &fallback_commands);
                immediate_apply_ms += fallback_started.elapsed().as_secs_f32() * 1000.0;
                immediate_apply_result.changed |= fallback_result.changed;
                immediate_apply_result.copied |= fallback_result.copied;
                immediate_apply_result.cut |= fallback_result.cut;
                immediate_apply_result.pasted |= fallback_result.pasted;
                if fallback_result.changed {
                    self.mark_dirty();
                }
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
        let text_editor_focus_post =
            self.editor_mode == EditorMode::TextEdit && self.text_editor_has_focus;
        let virtual_editor_focus_post = self.editor_mode == EditorMode::VirtualEditor
            && (self.virtual_editor_state.has_focus || ctx.memory(|m| m.has_focus(focus_id)));
        let editor_focus_for_plain_paste_post = if self.editor_mode == EditorMode::VirtualEditor {
            virtual_editor_focus_post
        } else {
            text_editor_focus_post
        };
        let editor_focus_post = if self.editor_mode == EditorMode::VirtualEditor {
            focus_active_post
        } else {
            text_editor_focus_post
        };
        let wants_keyboard_input_after = ctx.wants_keyboard_input();
        let (plain_request_virtual, plain_request_new) = self.resolve_plain_paste_shortcut_request(
            plain_paste_shortcut_pressed,
            editor_focus_for_plain_paste_post,
            saw_virtual_paste,
            wants_keyboard_input_after,
        );
        request_virtual_paste |= plain_request_virtual;
        request_paste_as_new |= plain_request_new;
        if self.editor_mode == EditorMode::VirtualEditor && request_virtual_paste {
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
        }
        if self.should_request_viewport_paste_for_new(request_paste_as_new, pasted_text.as_deref())
        {
            self.request_paste_as_new(ctx);
        }
        let copy_ready_post = focus_active_post || has_virtual_selection_post;
        if focus_active_post || focus_promotion_requested {
            let deferred_started = Instant::now();
            deferred_focus_apply_result =
                self.apply_virtual_commands(ctx, &deferred_focus_commands);
            deferred_focus_apply_ms = deferred_started.elapsed().as_secs_f32() * 1000.0;
            if deferred_focus_apply_result.changed {
                self.mark_dirty();
            }
        }
        if copy_ready_post {
            let deferred_started = Instant::now();
            deferred_copy_apply_result = self.apply_virtual_commands(ctx, &deferred_copy_commands);
            deferred_copy_apply_ms = deferred_started.elapsed().as_secs_f32() * 1000.0;
            if deferred_copy_apply_result.changed {
                self.mark_dirty();
            }
        }
        let virtual_paste_consumed = immediate_apply_result.pasted
            || deferred_focus_apply_result.pasted
            || deferred_copy_apply_result.pasted;
        let paste_as_new_consumed =
            self.maybe_consume_explicit_paste_as_new(&mut pasted_text, text_editor_focus_post);
        if !editor_focus_post && !ctx.wants_keyboard_input() && !virtual_paste_consumed {
            if let Some(text) = pasted_text {
                if Self::should_create_paste_from_clipboard(text.as_str()) {
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
            pasted: virtual_paste_consumed || paste_as_new_consumed,
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
        self.trace_virtual_input_perf(
            &immediate_focus_commands,
            &deferred_focus_commands,
            &deferred_copy_commands,
            VirtualInputPerfStats {
                input_route_ms,
                immediate_apply_ms,
                deferred_focus_apply_ms,
                deferred_copy_apply_ms,
                apply_result: combined_apply,
            },
        );

        self.render_status_bar(ctx);
        self.render_toasts(ctx);

        self.maybe_dispatch_palette_search();
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
        if self.editor_mode == EditorMode::VirtualEditor
            && (self.virtual_editor_active
                || self.virtual_editor_state.has_focus
                || ctx.memory(|m| m.has_focus(focus_id)))
        {
            let elapsed = Instant::now().saturating_duration_since(self.virtual_caret_phase_start);
            let interval_ms = CARET_BLINK_INTERVAL.as_millis().max(1);
            let remainder_ms = interval_ms - (elapsed.as_millis() % interval_ms);
            let until = Duration::from_millis(remainder_ms as u64).max(Duration::from_millis(1));
            repaint_after = repaint_after.min(until);
        }
        ctx.request_repaint_after(repaint_after);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.flush_pending_saves_for_shutdown();
    }
}

impl Drop for LocalPasteApp {
    fn drop(&mut self) {
        if let Some(id) = self.selected_id.take() {
            if let Err(err) = self.locks.release(&id, &self.lock_owner_id) {
                warn!("failed to release paste lock '{}' on drop: {}", id, err);
            }
        }
    }
}

#[cfg(test)]
mod tests;
