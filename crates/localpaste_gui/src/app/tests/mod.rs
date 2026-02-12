//! Integration-style app tests that exercise state, editor, and highlight flows.

use super::highlight::align_old_lines_by_hash;
use super::*;
use crate::backend::CoreEvent;
use chrono::Utc;
use crossbeam_channel::unbounded;
use eframe::egui::TextBuffer;
use syntect::util::LinesWithEndings;
use tempfile::TempDir;

struct TestHarness {
    _dir: TempDir,
    app: LocalPasteApp,
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

fn make_app() -> TestHarness {
    let (cmd_tx, _cmd_rx) = unbounded();
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
        backend: BackendHandle { cmd_tx, evt_rx },
        all_pastes: vec![PasteSummary {
            id: "alpha".to_string(),
            name: "Alpha".to_string(),
            language: None,
            content_len: 7,
            updated_at: Utc::now(),
            folder_id: None,
            tags: Vec::new(),
        }],
        pastes: vec![PasteSummary {
            id: "alpha".to_string(),
            name: "Alpha".to_string(),
            language: None,
            content_len: 7,
            updated_at: Utc::now(),
            folder_id: None,
            tags: Vec::new(),
        }],
        folders: Vec::new(),
        selected_id: Some("alpha".to_string()),
        selected_paste: Some(Paste::new("content".to_string(), "Alpha".to_string())),
        edit_name: "Alpha".to_string(),
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
        folder_dialog: None,
        selected_content: EditorBuffer::new("content".to_string()),
        editor_cache: EditorLayoutCache::default(),
        editor_lines: EditorLineIndex::default(),
        editor_mode: EditorMode::TextEdit,
        virtual_selection: VirtualSelectionState::default(),
        virtual_editor_buffer: RopeBuffer::new("content"),
        virtual_editor_state: VirtualEditorState::default(),
        virtual_editor_history: VirtualEditorHistory::default(),
        virtual_layout: WrapLayoutCache::default(),
        virtual_drag_active: false,
        virtual_editor_active: false,
        virtual_viewport_height: 0.0,
        virtual_line_height: 1.0,
        virtual_wrap_width: 0.0,
        highlight_worker: spawn_highlight_worker(),
        highlight_pending: None,
        highlight_render: None,
        highlight_staged: None,
        highlight_version: 0,
        syntect: SyntectSettings::default(),
        db_path: db_path_str,
        locks,
        _server: server,
        server_addr,
        server_used_fallback,
        status: None,
        save_status: SaveStatus::Saved,
        last_edit_at: None,
        save_in_flight: false,
        autosave_delay: Duration::from_millis(2000),
        focus_editor_next: false,
        style_applied: false,
        window_checked: false,
        last_refresh_at: Instant::now(),
        perf_log_enabled: false,
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
        editor_input_trace_enabled: false,
        highlight_trace_enabled: false,
    };

    TestHarness { _dir: dir, app }
}

#[test]
fn paste_missing_clears_selection_and_removes_list_entry() {
    let mut harness = make_app();
    harness.app.apply_event(CoreEvent::PasteMissing {
        id: "alpha".to_string(),
    });

    assert!(harness.app.pastes.is_empty());
    assert!(harness.app.selected_id.is_none());
    assert!(harness.app.selected_paste.is_none());
    assert_eq!(harness.app.selected_content.len(), 0);
    assert!(harness.app.status.is_some());
}

#[test]
fn paste_missing_non_selected_removes_list_entry() {
    let mut harness = make_app();
    harness.app.pastes.push(PasteSummary {
        id: "beta".to_string(),
        name: "Beta".to_string(),
        language: None,
        content_len: 4,
        updated_at: Utc::now(),
        folder_id: None,
        tags: Vec::new(),
    });

    harness.app.apply_event(CoreEvent::PasteMissing {
        id: "beta".to_string(),
    });

    assert_eq!(harness.app.pastes.len(), 1);
    assert_eq!(harness.app.pastes[0].id, "alpha");
    assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));
    assert!(harness.app.selected_paste.is_some());
}

#[test]
fn editor_buffer_tracks_char_len() {
    let mut buffer = EditorBuffer::new("ab".to_string());
    assert_eq!(buffer.chars_len(), 2);

    buffer.insert_text("\u{00E9}", 1);
    assert_eq!(buffer.chars_len(), 3);

    buffer.delete_char_range(1..2);
    assert_eq!(buffer.chars_len(), 2);

    buffer.replace_with("xyz");
    assert_eq!(buffer.chars_len(), 3);

    buffer.clear();
    assert_eq!(buffer.chars_len(), 0);
}

#[test]
fn highlight_cache_reuses_layout_when_unchanged() {
    let mut cache = EditorLayoutCache::default();
    let buffer = EditorBuffer::new("def foo():\n    return 1\n".to_string());
    let syntect = SyntectSettings::default();

    egui::__run_test_ctx(|ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let font = egui::FontId::monospace(14.0);
            let theme = CodeTheme::dark(14.0);
            let _ = cache.layout(
                ui,
                &buffer,
                400.0,
                "py",
                false,
                Some(&theme),
                None,
                0,
                &font,
                &syntect,
            );
            let first_ms = cache.last_highlight_ms;
            let line_count = LinesWithEndings::from(buffer.as_str()).count();
            let _ = cache.layout(
                ui,
                &buffer,
                400.0,
                "py",
                false,
                Some(&theme),
                None,
                0,
                &font,
                &syntect,
            );

            assert_eq!(cache.last_highlight_ms, first_ms);
            assert_eq!(cache.highlight_line_count(), line_count);
        });
    });
}

#[test]
fn highlight_cache_updates_after_line_edit() {
    let mut cache = EditorLayoutCache::default();
    let mut buffer = EditorBuffer::new("line1\nline2\nline3\n".to_string());
    let syntect = SyntectSettings::default();

    egui::__run_test_ctx(|ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let font = egui::FontId::monospace(14.0);
            let theme = CodeTheme::dark(14.0);
            let _ = cache.layout(
                ui,
                &buffer,
                400.0,
                "py",
                false,
                Some(&theme),
                None,
                0,
                &font,
                &syntect,
            );

            buffer.insert_text("x", 0);

            let _ = cache.layout(
                ui,
                &buffer,
                400.0,
                "py",
                false,
                Some(&theme),
                None,
                0,
                &font,
                &syntect,
            );
            let line_count = LinesWithEndings::from(buffer.as_str()).count();
            assert_eq!(cache.highlight_line_count(), line_count);
        });
    });
}

#[test]
fn align_old_lines_handles_middle_insert() {
    let old = vec![
        FakeHighlightLine { hash: 1, name: "a" },
        FakeHighlightLine { hash: 2, name: "b" },
        FakeHighlightLine { hash: 3, name: "c" },
        FakeHighlightLine { hash: 4, name: "d" },
    ];
    let aligned = align_old_lines_by_hash(old, &[1, 2, 99, 3, 4], |line| line.hash);
    assert_eq!(
        aligned_names(&aligned),
        vec![Some("a"), Some("b"), None, Some("c"), Some("d")]
    );
}

#[test]
fn align_old_lines_handles_middle_delete() {
    let old = vec![
        FakeHighlightLine { hash: 1, name: "a" },
        FakeHighlightLine { hash: 2, name: "b" },
        FakeHighlightLine { hash: 3, name: "c" },
        FakeHighlightLine { hash: 4, name: "d" },
    ];
    let aligned = align_old_lines_by_hash(old, &[1, 3, 4], |line| line.hash);
    assert_eq!(
        aligned_names(&aligned),
        vec![Some("a"), Some("c"), Some("d")]
    );
}

#[test]
fn align_old_lines_handles_middle_replace() {
    let old = vec![
        FakeHighlightLine { hash: 1, name: "a" },
        FakeHighlightLine { hash: 2, name: "b" },
        FakeHighlightLine { hash: 4, name: "d" },
    ];
    let aligned = align_old_lines_by_hash(old, &[1, 77, 4], |line| line.hash);
    assert_eq!(aligned_names(&aligned), vec![Some("a"), None, Some("d")]);
}

#[test]
fn editor_line_index_tracks_lines_and_trailing_newlines() {
    let buffer = EditorBuffer::new("alpha\nbeta\n".to_string());
    let mut index = EditorLineIndex::default();
    index.ensure_for(buffer.revision(), buffer.as_str());

    assert_eq!(index.line_count(), 3);
    assert_eq!(index.line_without_newline(buffer.as_str(), 0), "alpha");
    assert_eq!(index.line_without_newline(buffer.as_str(), 1), "beta");
    assert_eq!(index.line_without_newline(buffer.as_str(), 2), "");
}

#[test]
fn virtual_selection_text_multiline_preserves_single_newlines() {
    let mut harness = make_app();
    harness
        .app
        .selected_content
        .reset("alpha\nbeta\ngamma".to_string());
    harness.app.virtual_selection.select_range(
        VirtualCursor { line: 0, column: 2 },
        VirtualCursor { line: 2, column: 3 },
    );

    let copied = harness.app.virtual_selection_text().expect("copied text");
    assert_eq!(copied, "pha\nbeta\ngam");
}

#[test]
fn virtual_selection_text_preserves_blank_line_boundaries() {
    let mut harness = make_app();
    harness.app.selected_content.reset("a\n\nb".to_string());
    harness.app.virtual_selection.select_range(
        VirtualCursor { line: 0, column: 1 },
        VirtualCursor { line: 2, column: 0 },
    );

    let copied = harness.app.virtual_selection_text().expect("copied text");
    assert_eq!(copied, "\n\n");
}

#[test]
fn virtual_select_line_includes_newline_for_non_terminal_line() {
    let mut harness = make_app();
    harness.app.reset_virtual_editor("one\ntwo\nthree");
    harness.app.virtual_select_line(1);
    let copied = harness.app.virtual_selected_text().expect("copied text");
    assert_eq!(copied, "two\n");
}

#[test]
fn virtual_select_line_last_line_excludes_missing_newline() {
    let mut harness = make_app();
    harness.app.reset_virtual_editor("one\ntwo");
    harness.app.virtual_select_line(1);
    let copied = harness.app.virtual_selected_text().expect("copied text");
    assert_eq!(copied, "two");
}

#[test]
fn staged_highlight_waits_for_idle() {
    let mut harness = make_app();
    harness.app.selected_content.insert_text("x", 0);
    let active_revision = harness.app.selected_content.revision();
    let active_len = harness.app.selected_content.len();
    harness.app.highlight_render = Some(HighlightRender {
        paste_id: "alpha".to_string(),
        revision: active_revision.saturating_sub(1),
        text_len: active_len,
        language_hint: "py".to_string(),
        theme_key: "base16-mocha.dark".to_string(),
        lines: Vec::new(),
    });
    let render = HighlightRender {
        paste_id: "alpha".to_string(),
        revision: active_revision,
        text_len: active_len,
        language_hint: "py".to_string(),
        theme_key: "base16-mocha.dark".to_string(),
        lines: Vec::new(),
    };
    harness.app.highlight_staged = Some(render.clone());
    let now = Instant::now();
    harness.app.last_interaction_at = Some(now);
    harness.app.maybe_apply_staged_highlight(now);
    assert_eq!(
        harness
            .app
            .highlight_render
            .as_ref()
            .map(|render| render.revision),
        Some(0)
    );
    assert!(harness.app.highlight_staged.is_some());

    let idle_now = now + HIGHLIGHT_APPLY_IDLE + Duration::from_millis(10);
    harness.app.maybe_apply_staged_highlight(idle_now);
    assert_eq!(
        harness
            .app
            .highlight_render
            .as_ref()
            .map(|render| render.revision),
        Some(1)
    );
    assert!(harness.app.highlight_staged.is_none());
}

#[test]
fn staged_highlight_applies_immediately_without_current_render() {
    let mut harness = make_app();
    let active_revision = harness.app.selected_content.revision();
    let active_len = harness.app.selected_content.len();
    harness.app.highlight_staged = Some(HighlightRender {
        paste_id: "alpha".to_string(),
        revision: active_revision,
        text_len: active_len,
        language_hint: "py".to_string(),
        theme_key: "base16-mocha.dark".to_string(),
        lines: Vec::new(),
    });
    let now = Instant::now();
    harness.app.last_interaction_at = Some(now);
    harness.app.maybe_apply_staged_highlight(now);
    assert!(harness.app.highlight_render.is_some());
}

#[test]
fn staged_highlight_drops_stale_revision_without_version_bump() {
    let mut harness = make_app();
    harness.app.highlight_version = 19;
    let active_len = harness.app.selected_content.len();
    harness.app.highlight_staged = Some(HighlightRender {
        paste_id: "alpha".to_string(),
        revision: harness.app.selected_content.revision().saturating_add(1),
        text_len: active_len,
        language_hint: "py".to_string(),
        theme_key: "base16-mocha.dark".to_string(),
        lines: Vec::new(),
    });

    harness.app.maybe_apply_staged_highlight(Instant::now());

    assert!(harness.app.highlight_staged.is_none());
    assert!(harness.app.highlight_render.is_none());
    assert_eq!(harness.app.highlight_version, 19);
}

#[test]
fn staged_highlight_drops_stale_text_len_without_version_bump() {
    let mut harness = make_app();
    harness.app.highlight_version = 23;
    let active_revision = harness.app.selected_content.revision();
    harness.app.highlight_staged = Some(HighlightRender {
        paste_id: "alpha".to_string(),
        revision: active_revision,
        text_len: harness.app.selected_content.len().saturating_add(1),
        language_hint: "py".to_string(),
        theme_key: "base16-mocha.dark".to_string(),
        lines: Vec::new(),
    });

    harness.app.maybe_apply_staged_highlight(Instant::now());

    assert!(harness.app.highlight_staged.is_none());
    assert!(harness.app.highlight_render.is_none());
    assert_eq!(harness.app.highlight_version, 23);
}

#[test]
fn highlight_request_skips_when_staged_matches() {
    let mut harness = make_app();
    let render = HighlightRender {
        paste_id: "alpha".to_string(),
        revision: 0,
        text_len: harness.app.selected_content.len(),
        language_hint: "py".to_string(),
        theme_key: "base16-mocha.dark".to_string(),
        lines: Vec::new(),
    };
    harness.app.highlight_staged = Some(render);
    let should = harness.app.should_request_highlight(
        0,
        harness.app.selected_content.len(),
        "py",
        "base16-mocha.dark",
        false,
        "alpha",
    );
    assert!(!should);
}

#[test]
fn queue_highlight_render_ignores_older_revision_when_current_exists() {
    let mut harness = make_app();
    harness.app.highlight_render = Some(HighlightRender {
        paste_id: "alpha".to_string(),
        revision: 9,
        text_len: harness.app.selected_content.len(),
        language_hint: "py".to_string(),
        theme_key: "base16-mocha.dark".to_string(),
        lines: Vec::new(),
    });
    harness.app.queue_highlight_render(HighlightRender {
        paste_id: "alpha".to_string(),
        revision: 4,
        text_len: harness.app.selected_content.len(),
        language_hint: "py".to_string(),
        theme_key: "base16-mocha.dark".to_string(),
        lines: Vec::new(),
    });

    assert!(harness.app.highlight_staged.is_none());
    assert_eq!(
        harness
            .app
            .highlight_render
            .as_ref()
            .map(|render| render.revision),
        Some(9)
    );
}

#[test]
fn paste_saved_keeps_existing_highlight_render() {
    let mut harness = make_app();
    harness.app.highlight_version = 7;
    harness.app.highlight_render = Some(HighlightRender {
        paste_id: "alpha".to_string(),
        revision: 42,
        text_len: harness.app.selected_content.len(),
        language_hint: "py".to_string(),
        theme_key: "base16-mocha.dark".to_string(),
        lines: Vec::new(),
    });

    let mut paste = Paste::new("content".to_string(), "Alpha".to_string());
    paste.id = "alpha".to_string();
    harness.app.apply_event(CoreEvent::PasteSaved { paste });

    assert!(harness.app.highlight_render.is_some());
    assert_eq!(harness.app.highlight_version, 7);
}

#[test]
fn virtual_copy_reports_copied_without_text_mutation() {
    let mut harness = make_app();
    harness.app.reset_virtual_editor("abcdef");
    let len = harness.app.virtual_editor_buffer.len_chars();
    harness.app.virtual_editor_state.set_cursor(1, len);
    harness.app.virtual_editor_state.move_cursor(4, len, true);
    let ctx = egui::Context::default();

    let result = harness
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::Copy]);
    assert!(!result.changed);
    assert!(result.copied);
    assert_eq!(harness.app.virtual_editor_buffer.to_string(), "abcdef");
}

#[test]
fn virtual_cut_reports_copy_and_removes_selected_text() {
    let mut harness = make_app();
    harness.app.reset_virtual_editor("abcdef");
    let len = harness.app.virtual_editor_buffer.len_chars();
    harness.app.virtual_editor_state.set_cursor(1, len);
    harness.app.virtual_editor_state.move_cursor(4, len, true);
    let ctx = egui::Context::default();

    let result = harness
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::Cut]);
    assert!(result.changed);
    assert!(result.copied);
    assert!(result.cut);
    assert_eq!(harness.app.virtual_editor_buffer.to_string(), "aef");
}

#[test]
fn ime_commit_replaces_preedit_and_clears_state() {
    let mut harness = make_app();
    harness.app.reset_virtual_editor("ab");
    let len = harness.app.virtual_editor_buffer.len_chars();
    harness.app.virtual_editor_state.set_cursor(1, len);
    let ctx = egui::Context::default();

    let result = harness.app.apply_virtual_commands(
        &ctx,
        &[
            VirtualInputCommand::ImeEnabled,
            VirtualInputCommand::ImePreedit("に".to_string()),
            VirtualInputCommand::ImeCommit("日".to_string()),
            VirtualInputCommand::ImeDisabled,
        ],
    );

    assert!(result.changed);
    assert_eq!(harness.app.virtual_editor_buffer.to_string(), "a日b");
    assert!(!harness.app.virtual_editor_state.ime.enabled);
    assert!(harness.app.virtual_editor_state.ime.preedit_range.is_none());
    assert!(harness.app.virtual_editor_state.ime.preedit_text.is_empty());
}

#[test]
fn ime_disable_cancels_uncommitted_preedit_text() {
    let mut harness = make_app();
    harness.app.reset_virtual_editor("ab");
    let len = harness.app.virtual_editor_buffer.len_chars();
    harness.app.virtual_editor_state.set_cursor(1, len);
    let ctx = egui::Context::default();

    let result = harness.app.apply_virtual_commands(
        &ctx,
        &[
            VirtualInputCommand::ImeEnabled,
            VirtualInputCommand::ImePreedit("に".to_string()),
            VirtualInputCommand::ImeDisabled,
        ],
    );

    assert!(result.changed);
    assert_eq!(harness.app.virtual_editor_buffer.to_string(), "ab");
    assert!(!harness.app.virtual_editor_state.ime.enabled);
    assert!(harness.app.virtual_editor_state.ime.preedit_range.is_none());
    assert!(harness.app.virtual_editor_state.ime.preedit_text.is_empty());
}

#[test]
fn empty_preedit_clears_composition_and_allows_insert_text() {
    let mut harness = make_app();
    harness.app.reset_virtual_editor("ab");
    let len = harness.app.virtual_editor_buffer.len_chars();
    harness.app.virtual_editor_state.set_cursor(1, len);
    let ctx = egui::Context::default();

    let result = harness.app.apply_virtual_commands(
        &ctx,
        &[
            VirtualInputCommand::ImeEnabled,
            VirtualInputCommand::ImePreedit("に".to_string()),
            VirtualInputCommand::ImePreedit(String::new()),
            VirtualInputCommand::InsertText("x".to_string()),
        ],
    );

    assert!(result.changed);
    assert_eq!(harness.app.virtual_editor_buffer.to_string(), "axb");
    assert!(harness.app.virtual_editor_state.ime.preedit_range.is_none());
    assert!(harness.app.virtual_editor_state.ime.preedit_text.is_empty());
}

#[test]
fn virtual_command_classification_respects_focus_policy() {
    assert_eq!(
        classify_virtual_command(&VirtualInputCommand::Copy, false),
        VirtualCommandBucket::DeferredCopy
    );
    assert_eq!(
        classify_virtual_command(&VirtualInputCommand::InsertText("x".to_string()), false),
        VirtualCommandBucket::DeferredFocus
    );
    assert_eq!(
        classify_virtual_command(&VirtualInputCommand::InsertText("x".to_string()), true),
        VirtualCommandBucket::ImmediateFocus
    );
    assert_eq!(
        classify_virtual_command(&VirtualInputCommand::Cut, true),
        VirtualCommandBucket::DeferredFocus
    );
    assert_eq!(
        classify_virtual_command(&VirtualInputCommand::Paste("x".to_string()), true),
        VirtualCommandBucket::DeferredFocus
    );
}

#[test]
fn off_focus_commands_do_not_mutate_virtual_editor_with_selection() {
    fn setup_selection(app: &mut LocalPasteApp) {
        app.reset_virtual_editor("abcdef");
        let len = app.virtual_editor_buffer.len_chars();
        app.virtual_editor_state.set_cursor(1, len);
        app.virtual_editor_state.move_cursor(4, len, true);
    }

    fn merge_apply(target: &mut VirtualApplyResult, src: VirtualApplyResult) {
        target.changed |= src.changed;
        target.copied |= src.copied;
        target.cut |= src.cut;
        target.pasted |= src.pasted;
    }

    fn route_and_apply(
        app: &mut LocalPasteApp,
        ctx: &egui::Context,
        command: VirtualInputCommand,
        focus_active_pre: bool,
        focus_active_post: bool,
        copy_ready_post: bool,
    ) -> VirtualApplyResult {
        let mut immediate = Vec::new();
        let mut deferred_focus = Vec::new();
        let mut deferred_copy = Vec::new();
        match classify_virtual_command(&command, focus_active_pre) {
            VirtualCommandBucket::ImmediateFocus => immediate.push(command),
            VirtualCommandBucket::DeferredFocus => deferred_focus.push(command),
            VirtualCommandBucket::DeferredCopy => deferred_copy.push(command),
        }

        let mut result = app.apply_virtual_commands(ctx, &immediate);
        if focus_active_post {
            merge_apply(
                &mut result,
                app.apply_virtual_commands(ctx, &deferred_focus),
            );
        }
        if copy_ready_post {
            merge_apply(&mut result, app.apply_virtual_commands(ctx, &deferred_copy));
        }
        result
    }

    let mut harness = make_app();
    let ctx = egui::Context::default();
    let blocked_commands = [
        VirtualInputCommand::InsertText("X".to_string()),
        VirtualInputCommand::Backspace { word: false },
        VirtualInputCommand::DeleteForward { word: false },
        VirtualInputCommand::Cut,
        VirtualInputCommand::Paste("ZZ".to_string()),
        VirtualInputCommand::ImeEnabled,
        VirtualInputCommand::ImePreedit("Z".to_string()),
        VirtualInputCommand::ImeCommit("Z".to_string()),
        VirtualInputCommand::ImeDisabled,
        VirtualInputCommand::MoveLeft {
            select: false,
            word: false,
        },
        VirtualInputCommand::SelectAll,
    ];

    for command in blocked_commands {
        setup_selection(&mut harness.app);
        let before_text = harness.app.virtual_editor_buffer.to_string();
        let before_cursor = harness.app.virtual_editor_state.cursor();
        let before_selection = harness.app.virtual_editor_state.selection_range();
        let before_ime = harness.app.virtual_editor_state.ime.clone();
        let result = route_and_apply(
            &mut harness.app,
            &ctx,
            command,
            false,
            false,
            true, // copy-ready because selection exists
        );

        assert_eq!(harness.app.virtual_editor_buffer.to_string(), before_text);
        assert_eq!(harness.app.virtual_editor_state.cursor(), before_cursor);
        assert_eq!(
            harness.app.virtual_editor_state.selection_range(),
            before_selection
        );
        assert_eq!(harness.app.virtual_editor_state.ime, before_ime);
        assert!(!result.changed);
        assert!(!result.cut);
        assert!(!result.pasted);
    }

    setup_selection(&mut harness.app);
    let before_text = harness.app.virtual_editor_buffer.to_string();
    let result = route_and_apply(
        &mut harness.app,
        &ctx,
        VirtualInputCommand::Copy,
        false,
        false,
        true,
    );
    assert_eq!(harness.app.virtual_editor_buffer.to_string(), before_text);
    assert!(!result.changed);
    assert!(result.copied);
}

#[test]
fn virtual_click_counter_promotes_to_triple_and_resets() {
    let now = Instant::now();
    let p = egui::pos2(100.0, 200.0);
    let c1 = next_virtual_click_count(None, None, None, 0, 5, p, now);
    assert_eq!(c1, 1);
    let c2 = next_virtual_click_count(Some(now), Some(p), Some(5), c1, 5, p, now);
    assert_eq!(c2, 2);
    let c3 = next_virtual_click_count(Some(now), Some(p), Some(5), c2, 5, p, now);
    assert_eq!(c3, 3);

    let changed_line = next_virtual_click_count(Some(now), Some(p), Some(5), c3, 6, p, now);
    assert_eq!(changed_line, 3);

    let expired = next_virtual_click_count(
        Some(now),
        Some(p),
        Some(5),
        c3,
        5,
        p,
        now + EDITOR_DOUBLE_CLICK_WINDOW + Duration::from_millis(1),
    );
    assert_eq!(expired, 1);

    let far = egui::pos2(100.0 + EDITOR_DOUBLE_CLICK_DISTANCE + 1.0, 200.0);
    let distant = next_virtual_click_count(Some(now), Some(p), Some(5), c3, 5, far, now);
    assert_eq!(distant, 1);
}

#[test]
fn drag_autoscroll_delta_scrolls_up_when_pointer_above() {
    let delta = drag_autoscroll_delta(80.0, 100.0, 220.0, 20.0);
    assert!(delta > 0.0);
}

#[test]
fn drag_autoscroll_delta_scrolls_down_when_pointer_below() {
    let delta = drag_autoscroll_delta(260.0, 100.0, 220.0, 20.0);
    assert!(delta < 0.0);
}

#[test]
fn drag_autoscroll_delta_is_zero_inside_viewport() {
    let delta = drag_autoscroll_delta(150.0, 100.0, 220.0, 20.0);
    assert_eq!(delta, 0.0);
}

#[test]
fn word_range_at_selects_word() {
    let text = "hello world";
    let (start, end) = word_range_at(text, 1).expect("range");
    let selected: String = text.chars().skip(start).take(end - start).collect();
    assert_eq!(selected, "hello");
}

#[test]
fn search_results_respect_collection_filter() {
    let mut harness = make_app();
    harness
        .app
        .set_active_collection(SidebarCollection::Unfiled);

    let now = Utc::now();
    let with_folder = PasteSummary {
        id: "a".to_string(),
        name: "with-folder".to_string(),
        language: Some("rust".to_string()),
        content_len: 10,
        updated_at: now,
        folder_id: Some("folder-1".to_string()),
        tags: Vec::new(),
    };
    let unfiled = PasteSummary {
        id: "b".to_string(),
        name: "unfiled".to_string(),
        language: Some("rust".to_string()),
        content_len: 10,
        updated_at: now,
        folder_id: None,
        tags: Vec::new(),
    };

    harness.app.apply_event(CoreEvent::SearchResults {
        query: "rust".to_string(),
        items: vec![with_folder, unfiled.clone()],
    });

    assert_eq!(harness.app.pastes.len(), 1);
    assert_eq!(harness.app.pastes[0].id, unfiled.id);
}

#[test]
fn paste_list_filters_recent_collection() {
    let mut harness = make_app();
    harness.app.set_active_collection(SidebarCollection::Recent);
    let old = PasteSummary {
        id: "old".to_string(),
        name: "old".to_string(),
        language: None,
        content_len: 3,
        updated_at: Utc::now() - chrono::Duration::days(30),
        folder_id: None,
        tags: Vec::new(),
    };
    let fresh = PasteSummary {
        id: "fresh".to_string(),
        name: "fresh".to_string(),
        language: None,
        content_len: 5,
        updated_at: Utc::now(),
        folder_id: None,
        tags: Vec::new(),
    };

    harness.app.apply_event(CoreEvent::PasteList {
        items: vec![old, fresh.clone()],
    });
    assert_eq!(harness.app.pastes.len(), 1);
    assert_eq!(harness.app.pastes[0].id, fresh.id);
}
