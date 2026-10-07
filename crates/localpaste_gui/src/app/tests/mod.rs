//! Integration-style app tests that exercise state, editor, and highlight flows.

use super::highlight::align_old_lines_by_hash;
use super::*;
use crate::backend::{BackendHandle, CoreCmd, CoreEvent};
use chrono::Utc;
use crossbeam_channel::{unbounded, Receiver, Sender, TryRecvError};
use eframe::App as _;
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

fn aligned_names(aligned: &[Option<(usize, FakeHighlightLine)>]) -> Vec<Option<&'static str>> {
    aligned
        .iter()
        .map(|line| line.as_ref().map(|(_, line)| line.name))
        .collect()
}

fn test_summary(id: &str, name: &str, language: Option<&str>, content_len: usize) -> PasteSummary {
    test_summary_at(id, name, language, content_len, Utc::now())
}

fn test_summary_at(
    id: &str,
    name: &str,
    language: Option<&str>,
    content_len: usize,
    updated_at: chrono::DateTime<Utc>,
) -> PasteSummary {
    PasteSummary {
        id: id.to_string(),
        name: name.to_string(),
        language: language.map(ToString::to_string),
        content_len,
        updated_at,
        folder_id: None,
        tags: Vec::new(),
        derived: Default::default(),
        match_excerpt: None,
    }
}

fn test_summary_with_folder(
    id: &str,
    name: &str,
    language: Option<&str>,
    content_len: usize,
    folder_id: &str,
) -> PasteSummary {
    PasteSummary {
        folder_id: Some(folder_id.to_string()),
        ..test_summary(id, name, language, content_len)
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

/// Reset the virtual editor and rebuild wrapping metrics for a test buffer.
///
/// # Arguments
/// - `app`: App under test.
/// - `text`: Replacement buffer text.
/// - `wrap_width`: Wrap width used for layout reconstruction.
pub(super) fn configure_virtual_editor_with_wrap(
    app: &mut LocalPasteApp,
    text: &str,
    wrap_width: f32,
) {
    app.reset_virtual_editor(text);
    app.virtual_layout
        .rebuild(&app.virtual_editor_buffer, wrap_width, 1.0, 1.0);
}

/// Position the virtual editor cursor at a logical line/column pair for tests.
///
/// # Arguments
/// - `app`: App under test.
/// - `line`: Zero-based logical line index.
/// - `col`: Zero-based logical column within `line`.
pub(super) fn set_virtual_cursor_at(app: &mut LocalPasteApp, line: usize, col: usize) {
    let len = app.virtual_editor_buffer.len_chars();
    let pos = app.virtual_editor_buffer.line_col_to_char(line, col);
    app.virtual_editor_state.set_cursor(pos, len);
}

/// Asserts the virtual editor cursor's logical line/column coordinates.
///
/// # Arguments
/// - `app`: App under test.
/// - `expected`: Expected zero-based `(line, column)` cursor coordinates.
///
/// # Panics
/// Panics when the actual cursor coordinates do not match `expected`.
pub(super) fn assert_cursor_line_col(app: &LocalPasteApp, expected: (usize, usize)) {
    let line_col = app
        .virtual_editor_buffer
        .char_to_line_col(app.virtual_editor_state.cursor());
    assert_eq!(line_col, expected);
}

/// Replaces the active editor buffer through the live rope-backed path.
///
/// # Arguments
/// - `app`: App under test.
/// - `text`: Replacement buffer text.
pub(super) fn set_active_content(app: &mut LocalPasteApp, text: &str) {
    app.reset_virtual_editor(text);
}

/// Inserts text into the active editor buffer at a character index.
///
/// # Arguments
/// - `app`: App under test.
/// - `text`: Text to insert.
/// - `char_index`: Global character insertion position.
pub(super) fn insert_active_text(app: &mut LocalPasteApp, text: &str, char_index: usize) {
    let idx = char_index.min(app.virtual_editor_buffer.len_chars());
    let _ = app.virtual_editor_buffer.replace_char_range(idx..idx, text);
}

/// Builds a pressed key event with the provided modifier state.
///
/// # Arguments
/// - `key`: Logical egui key code to emit.
/// - `modifiers`: Modifier state carried by the event.
///
/// # Returns
/// A pressed [`egui::Event::Key`] test event.
pub(super) fn key_event(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

/// Returns the runtime-accurate primary command modifiers for the host platform.
///
/// # Returns
/// Modifier state that matches how egui reports `Cmd`/`Ctrl` shortcuts on the
/// active platform.
pub(super) fn primary_command_modifiers() -> egui::Modifiers {
    #[cfg(target_os = "macos")]
    {
        egui::Modifiers {
            command: true,
            ..Default::default()
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        egui::Modifiers {
            ctrl: true,
            command: true,
            ..Default::default()
        }
    }
}

/// Runs a closure with the virtual-editor platform keymap overridden.
///
/// # Arguments
/// - `platform`: Platform flavor to use for virtual-editor input mapping during `run`.
/// - `run`: Closure executed while the test platform override is active.
///
/// # Returns
/// The closure result.
pub(super) fn with_platform<R>(
    platform: super::virtual_editor::PlatformFlavor,
    run: impl FnOnce() -> R,
) -> R {
    struct ResetGuard;

    impl Drop for ResetGuard {
        fn drop(&mut self) {
            super::virtual_editor::set_test_platform(None);
        }
    }

    super::virtual_editor::set_test_platform(Some(platform));
    let _guard = ResetGuard;
    run()
}

/// Builds a command-modified pressed key event for platform-agnostic shortcut tests.
///
/// # Arguments
/// - `key`: Logical egui key code to emit with the command modifier set.
///
/// # Returns
/// A pressed [`egui::Event::Key`] test event carrying `command: true`.
pub(super) fn command_key_event(key: egui::Key) -> egui::Event {
    key_event(key, primary_command_modifiers())
}

fn run_editor_panel_once_output(
    app: &mut LocalPasteApp,
    ctx: &egui::Context,
    input: egui::RawInput,
) -> egui::FullOutput {
    ctx.run(input, |ctx| {
        app.render_editor_panel(ctx);
    })
}

fn run_editor_panel_once(app: &mut LocalPasteApp, ctx: &egui::Context, input: egui::RawInput) {
    let _ = run_editor_panel_once_output(app, ctx, input);
}

/// Settles direct editor geometry across four frames without acquiring app styling.
fn render_editor_frames(app: &mut LocalPasteApp, ctx: &egui::Context, width: f32) {
    for _ in 0..4 {
        run_editor_panel_once(
            app,
            ctx,
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 600.0),
                )),
                ..Default::default()
            },
        );
    }
}

/// Asserts that the app's rendered caret fits inside its observed viewport.
fn assert_caret_visible(app: &LocalPasteApp) {
    assert!(
        app.virtual_viewport.caret_visible(),
        "caret {:?}, viewport {:?}, offset {}",
        app.virtual_viewport.caret,
        app.virtual_viewport.rect,
        app.virtual_viewport.offset_y
    );
}

/// Finds the center of an exact rendered text label for pointer-driven tests.
fn rendered_label_center(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) if text.galley.job.text == label => {
                Some(text.pos + text.galley.size() / 2.0)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing rendered label {label}"))
}

/// Builds one unmodified pointer-button step; callers retain their frame policy.
fn primary_pointer_events(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}

/// Runs a full app update pass with the supplied raw egui events.
///
/// # Arguments
/// - `app`: App under test.
/// - `ctx`: egui context used for the frame.
/// - `events`: Raw input events to deliver during the frame.
pub(super) fn run_full_update(
    app: &mut LocalPasteApp,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
) {
    let _ = run_full_update_with_input(
        app,
        ctx,
        egui::RawInput {
            events,
            ..Default::default()
        },
    );
}

/// Runs a full app update pass with explicit raw egui input.
///
/// # Arguments
/// - `app`: App under test.
/// - `ctx`: egui context used for the frame.
/// - `input`: Raw input to deliver during the frame.
///
/// # Returns
/// Full egui frame output produced by the update pass.
pub(super) fn run_full_update_with_input(
    app: &mut LocalPasteApp,
    ctx: &egui::Context,
    mut input: egui::RawInput,
) -> egui::FullOutput {
    app.ensure_style(ctx);
    let mut frame = eframe::Frame::_new_kittest();
    app.raw_input_hook(ctx, &mut input);
    ctx.run(input, |ctx| {
        app.update(ctx, &mut frame);
    })
}

/// Runs exactly one discovery frame in the standard viewport, retaining its output.
fn run_discovery_frame_once(
    app: &mut LocalPasteApp,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    run_full_update_with_input(
        app,
        ctx,
        egui::RawInput {
            screen_rect: Some(virtual_editor_focus_support::screen_rect()),
            events,
            ..Default::default()
        },
    )
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
        search_case_sensitive: false,
    };
    let state = AppState::with_locks(config.clone(), server_db, locks.clone());
    let server = EmbeddedServer::start(state, false).expect("server");

    let mut app = LocalPasteApp::from_resources(
        &config,
        db.paste_version_retention_limit(),
        BackendHandle::from_test_channels(cmd_tx, evt_rx),
        spawn_highlight_worker(),
        locks,
        LockOwnerId::new("test-owner".to_string()),
        server,
    );
    app.all_pastes = vec![test_summary("alpha", "Alpha", None, 7)];
    app.pastes = vec![test_summary("alpha", "Alpha", None, 7)];
    app.selected_id = Some("alpha".to_string());
    app.selected_paste = Some(Paste::new("content".to_string(), "Alpha".to_string()));
    app.edit_name = "Alpha".to_string();
    app.virtual_editor_buffer = RopeBuffer::new("content");

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
    loop {
        let cmd = rx
            .recv_timeout(Duration::from_millis(200))
            .expect("expected outbound command");
        if matches!(cmd, CoreCmd::ListPasteVersions { .. }) {
            continue;
        }
        return cmd;
    }
}

mod backend_dispatch;
mod collections_and_search;
mod creation_and_projection;
mod discovery_input_order;
mod discovery_keyboard;
mod discovery_scopes;
mod editor_find;
mod editor_ux_regressions;
mod focus_and_paste_routing;
mod highlight_behaviors;
mod history_reset;
mod keyboard_navigation_audit;
mod persistence;
mod picker_copy;
mod picker_find;
mod picker_scroll;
mod save_and_metadata;
mod selected_delete;
mod selection_guard_regressions;
mod shortcut_help;
mod shutdown_behavior;
mod startup_styles;
mod state_basics;
mod state_toasts;
mod time_filters;
mod version_async_status;
mod version_modal_caching;
mod version_overlay_exclusivity;
mod virtual_editor_behaviors;
mod virtual_editor_focus;
mod virtual_editor_focus_shortcuts;
mod virtual_editor_focus_support;
