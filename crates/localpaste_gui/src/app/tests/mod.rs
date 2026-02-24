//! Integration-style app tests that exercise state, editor, and highlight flows.

use super::highlight::align_old_lines_by_hash;
use super::*;
use crate::backend::{BackendHandle, CoreCmd, CoreEvent};
use chrono::Utc;
use crossbeam_channel::{unbounded, Receiver, Sender, TryRecvError};
use eframe::egui::TextBuffer;
use localpaste_server::LockOwnerId;
use syntect::util::LinesWithEndings;
use tempfile::TempDir;

struct TestHarness {
    _dir: TempDir,
    app: LocalPasteApp,
    cmd_rx: Receiver<CoreCmd>,
}

#[derive(Debug)]
struct FakeHighlightLine {
    hash: u64,
    name: &'static str,
}

fn aligned_names(aligned: &[Option<FakeHighlightLine>]) -> Vec<Option<&'static str>> {
    aligned
        .iter()
        .map(|line| line.as_ref().map(|line| line.name))
        .collect()
}

fn test_summary(id: &str, name: &str, language: Option<&str>, content_len: usize) -> PasteSummary {
    PasteSummary {
        id: id.to_string(),
        name: name.to_string(),
        language: language.map(ToString::to_string),
        content_len,
        updated_at: Utc::now(),
        folder_id: None,
        tags: Vec::new(),
    }
}

/// Builds a single-character shaped galley for geometry-sensitive UI tests.
///
/// # Returns
/// Shared galley instance produced by egui's test context.
///
/// # Panics
/// Panics if egui test context fails to produce a galley.
pub(super) fn shaped_test_galley() -> Arc<egui::Galley> {
    let mut galley = None;
    egui::__run_test_ctx(|ctx| {
        galley = Some(ctx.fonts_mut(|fonts| {
            fonts.layout_no_wrap(
                "x".to_owned(),
                egui::FontId::monospace(14.0),
                egui::Color32::LIGHT_GRAY,
            )
        }));
    });
    galley.expect("test galley")
}

/// Configures deterministic font/style settings for virtual-editor test contexts.
pub(super) fn configure_virtual_editor_test_ctx(ctx: &egui::Context) {
    ctx.set_fonts(egui::FontDefinitions::empty());
    let mut style = (*ctx.style()).clone();
    style.text_styles.insert(
        egui::TextStyle::Name(EDITOR_TEXT_STYLE.into()),
        egui::FontId::new(14.0, egui::FontFamily::Monospace),
    );
    ctx.set_style(style);
}

fn run_editor_panel_once(app: &mut LocalPasteApp, ctx: &egui::Context, input: egui::RawInput) {
    let _ = ctx.run(input, |ctx| {
        app.render_editor_panel(ctx);
    });
}

fn make_app() -> TestHarness {
    let (cmd_tx, cmd_rx) = unbounded();
    let (_evt_tx, evt_rx) = unbounded();
    let dir = TempDir::new().expect("temp dir");
    let db_path = dir.path().join("db");
    let db_path_str = db_path.to_string_lossy().to_string();
    let db = Database::new(&db_path_str).expect("db");
    let locks = Arc::new(PasteLockManager::default());
    let server_db = db.share().expect("share db");
    let config = Config {
        db_path: db_path_str.clone(),
        port: 0,
        max_paste_size: 10 * 1024 * 1024,
        auto_save_interval: 2000,
        auto_backup: false,
    };
    let state = AppState::with_locks(config, server_db, locks.clone());
    let server = EmbeddedServer::start(state, false).expect("server");
    let server_addr = server.addr();
    let server_used_fallback = server.used_fallback();

    let app = LocalPasteApp {
        backend: BackendHandle::from_test_channels(cmd_tx, evt_rx),
        all_pastes: vec![test_summary("alpha", "Alpha", None, 7)],
        pastes: vec![test_summary("alpha", "Alpha", None, 7)],
        selected_id: Some("alpha".to_string()),
        selected_paste: Some(Paste::new("content".to_string(), "Alpha".to_string())),
        edit_name: "Alpha".to_string(),
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
        selected_content: EditorBuffer::new("content".to_string()),
        editor_lines: EditorLineIndex::default(),
        editor_mode: EditorMode::VirtualPreview,
        virtual_selection: VirtualSelectionState::default(),
        virtual_editor_buffer: RopeBuffer::new("content"),
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
        highlight_worker: spawn_highlight_worker(),
        highlight_pending: None,
        highlight_render: None,
        highlight_staged: None,
        highlight_staged_invalidation: None,
        highlight_version: 0,
        highlight_edit_hint: None,
        db_path: db_path_str,
        locks,
        lock_owner_id: LockOwnerId::new("test-owner".to_string()),
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
        autosave_delay: Duration::from_millis(2000),
        shortcut_help_open: false,
        focus_editor_next: false,
        style_applied: false,
        window_checked: false,
        last_refresh_at: Instant::now(),
        query_perf: QueryPerfCounters::default(),
        perf_log_enabled: false,
        frame_samples: VecDeque::with_capacity(PERF_SAMPLE_CAP),
        last_frame_at: None,
        last_perf_log_at: Instant::now(),
        last_interaction_at: None,
        last_virtual_click_at: None,
        last_virtual_click_pos: None,
        last_virtual_click_line: None,
        last_virtual_click_count: 0,
        paste_as_new_pending_frames: 0,
        paste_as_new_clipboard_requested_at: None,
        editor_input_trace_enabled: false,
        highlight_trace_enabled: false,
    };

    TestHarness {
        _dir: dir,
        app,
        cmd_rx,
    }
}

fn make_app_with_event_tx() -> (TestHarness, Sender<CoreEvent>) {
    let mut harness = make_app();
    let (cmd_tx, cmd_rx) = unbounded();
    let (evt_tx, evt_rx) = unbounded();
    harness.app.backend = BackendHandle::from_test_channels(cmd_tx, evt_rx);
    harness.cmd_rx = cmd_rx;
    (harness, evt_tx)
}

fn recv_cmd(rx: &Receiver<CoreCmd>) -> CoreCmd {
    rx.recv_timeout(Duration::from_millis(200))
        .expect("expected outbound command")
}

mod collections_and_search;
mod focus_and_paste_routing;
mod highlight_behaviors;
mod keyboard_navigation_audit;
mod save_and_metadata;
mod shutdown_behavior;
mod state_basics;
mod virtual_editor_behaviors;
