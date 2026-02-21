//! Virtual editor input/editing tests including IME and selection behavior.

use super::*;

fn run_virtual_editor_frame(
    app: &mut LocalPasteApp,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
) -> bool {
    let focus_id = egui::Id::new(VIRTUAL_EDITOR_ID);
    let egui_focus_pre = ctx.memory(|m| m.has_focus(focus_id));
    let focus_active_pre =
        app.is_virtual_editor_mode() && (app.virtual_editor_state.has_focus || egui_focus_pre);

    let mut immediate_focus_commands = Vec::new();
    let mut deferred_focus_commands = Vec::new();
    let mut deferred_copy_commands = Vec::new();
    for command in commands_from_events(events.as_slice(), focus_active_pre) {
        match classify_virtual_command(&command, focus_active_pre) {
            VirtualCommandBucket::ImmediateFocus => immediate_focus_commands.push(command),
            VirtualCommandBucket::DeferredFocus => deferred_focus_commands.push(command),
            VirtualCommandBucket::DeferredCopy => deferred_copy_commands.push(command),
        }
    }

    let _ = app.apply_virtual_commands(ctx, &immediate_focus_commands);

    let raw_input = egui::RawInput {
        events,
        ..Default::default()
    };
    let _ = ctx.run(raw_input, |ctx| {
        app.render_editor_panel(ctx);
    });

    let has_virtual_selection_post = app.virtual_editor_state.selection_range().is_some();
    let focus_active_post = app.editor_mode == EditorMode::VirtualEditor
        && (app.virtual_editor_active
            || app.virtual_editor_state.has_focus
            || ctx.memory(|m| m.has_focus(focus_id)));
    let copy_ready_post = focus_active_post || has_virtual_selection_post;

    if focus_active_post {
        let _ = app.apply_virtual_commands(ctx, &deferred_focus_commands);
    }
    if copy_ready_post {
        let _ = app.apply_virtual_commands(ctx, &deferred_copy_commands);
    }

    focus_active_pre
}

fn key_event(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

fn configure_virtual_editor_with_wrap(app: &mut LocalPasteApp, text: &str, wrap_width: f32) {
    app.reset_virtual_editor(text);
    app.virtual_layout
        .rebuild(&app.virtual_editor_buffer, wrap_width, 1.0, 1.0);
}

fn set_virtual_cursor_at(app: &mut LocalPasteApp, line: usize, col: usize) {
    let len = app.virtual_editor_buffer.len_chars();
    let pos = app.virtual_editor_buffer.line_col_to_char(line, col);
    app.virtual_editor_state.set_cursor(pos, len);
}

#[test]
fn virtual_copy_and_cut_report_expected_mutation_state() {
    struct ClipboardCase {
        use_cut: bool,
        expected_changed: bool,
        expected_cut: bool,
        expected_text: &'static str,
    }

    let cases = [
        ClipboardCase {
            use_cut: false,
            expected_changed: false,
            expected_cut: false,
            expected_text: "abcdef",
        },
        ClipboardCase {
            use_cut: true,
            expected_changed: true,
            expected_cut: true,
            expected_text: "aef",
        },
    ];

    for case in cases {
        let mut harness = make_app();
        harness.app.reset_virtual_editor("abcdef");
        let len = harness.app.virtual_editor_buffer.len_chars();
        harness.app.virtual_editor_state.set_cursor(1, len);
        harness.app.virtual_editor_state.move_cursor(4, len, true);
        let ctx = egui::Context::default();
        let command = if case.use_cut {
            VirtualInputCommand::Cut
        } else {
            VirtualInputCommand::Copy
        };

        let result = harness.app.apply_virtual_commands(&ctx, &[command]);
        assert_eq!(result.changed, case.expected_changed);
        assert!(result.copied);
        assert_eq!(result.cut, case.expected_cut);
        assert_eq!(
            harness.app.virtual_editor_buffer.to_string(),
            case.expected_text
        );
    }
}

#[test]
fn ime_commit_and_disable_clear_preedit_state_with_expected_buffer_results() {
    #[derive(Clone)]
    enum TailAction {
        Commit(&'static str),
        Mutate(VirtualInputCommand),
        None,
    }

    struct ImeCase {
        tail_action: TailAction,
        expected_text: &'static str,
    }

    let cases = [
        ImeCase {
            tail_action: TailAction::Commit("日"),
            expected_text: "a日b",
        },
        ImeCase {
            tail_action: TailAction::None,
            expected_text: "ab",
        },
        ImeCase {
            tail_action: TailAction::Mutate(VirtualInputCommand::Undo),
            expected_text: "ab",
        },
        ImeCase {
            tail_action: TailAction::Mutate(VirtualInputCommand::Backspace { word: false }),
            expected_text: "b",
        },
    ];

    for case in cases {
        let mut harness = make_app();
        harness.app.reset_virtual_editor("ab");
        let len = harness.app.virtual_editor_buffer.len_chars();
        harness.app.virtual_editor_state.set_cursor(1, len);
        let ctx = egui::Context::default();

        let mut commands = vec![VirtualInputCommand::ImeEnabled];
        commands.push(VirtualInputCommand::ImePreedit("に".to_string()));
        match case.tail_action {
            TailAction::Commit(text) => {
                commands.push(VirtualInputCommand::ImeCommit(text.to_string()));
            }
            TailAction::Mutate(command) => commands.push(command),
            TailAction::None => {}
        }
        commands.push(VirtualInputCommand::ImeDisabled);

        let result = harness
            .app
            .apply_virtual_commands(&ctx, commands.as_slice());

        assert!(result.changed);
        assert_eq!(
            harness.app.virtual_editor_buffer.to_string(),
            case.expected_text
        );
        assert!(!harness.app.virtual_editor_state.ime.enabled);
        assert!(harness.app.virtual_editor_state.ime.preedit_range.is_none());
        assert!(harness.app.virtual_editor_state.ime.preedit_text.is_empty());
    }
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
fn virtual_editor_focus_transition_matrix() {
    struct FocusCase {
        name: &'static str,
        focus_editor_next: bool,
        state_has_focus: bool,
        frames: Vec<Vec<egui::Event>>,
        expect_focus: bool,
    }

    let cases = [
        FocusCase {
            name: "focus-editor-next promotes focus",
            focus_editor_next: true,
            state_has_focus: false,
            frames: vec![Vec::new(), Vec::new()],
            expect_focus: true,
        },
        FocusCase {
            name: "state focus heals idle frame",
            focus_editor_next: false,
            state_has_focus: true,
            frames: vec![Vec::new()],
            expect_focus: true,
        },
        FocusCase {
            name: "state focus survives tab key frame",
            focus_editor_next: false,
            state_has_focus: true,
            frames: vec![vec![key_event(egui::Key::Tab, egui::Modifiers::default())]],
            expect_focus: false,
        },
    ];

    for case in cases {
        let mut harness = make_app();
        harness.app.editor_mode = EditorMode::VirtualEditor;
        harness.app.reset_virtual_editor("line one\nline two\n");
        harness.app.focus_editor_next = case.focus_editor_next;
        harness.app.virtual_editor_state.has_focus = case.state_has_focus;

        let ctx = egui::Context::default();
        configure_virtual_editor_test_ctx(&ctx);
        for events in case.frames {
            run_editor_panel_once(
                &mut harness.app,
                &ctx,
                egui::RawInput {
                    events,
                    ..Default::default()
                },
            );
        }

        let editor_id = egui::Id::new(VIRTUAL_EDITOR_ID);
        assert_eq!(
            ctx.memory(|m| m.has_focus(editor_id)),
            case.expect_focus,
            "focus mismatch for case: {}",
            case.name
        );
        assert_eq!(
            harness.app.virtual_editor_state.has_focus, case.expect_focus,
            "state mismatch for case: {}",
            case.name
        );
    }
}

#[test]
fn click_in_editor_viewport_without_row_hit_reclaims_focus() {
    let mut harness = make_app();
    harness.app.editor_mode = EditorMode::VirtualEditor;
    harness.app.reset_virtual_editor("line one\n");

    let ctx = egui::Context::default();
    configure_virtual_editor_test_ctx(&ctx);
    let screen_rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1200.0, 900.0));
    let editor_id = egui::Id::new(VIRTUAL_EDITOR_ID);

    run_editor_panel_once(
        &mut harness.app,
        &ctx,
        egui::RawInput {
            screen_rect: Some(screen_rect),
            ..Default::default()
        },
    );
    assert!(!ctx.memory(|m| m.has_focus(editor_id)));

    let click_pos = egui::pos2(240.0, 700.0);
    run_editor_panel_once(
        &mut harness.app,
        &ctx,
        egui::RawInput {
            screen_rect: Some(screen_rect),
            events: vec![
                egui::Event::PointerMoved(click_pos),
                egui::Event::PointerButton {
                    pos: click_pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::default(),
                },
            ],
            ..Default::default()
        },
    );

    assert!(ctx.memory(|m| m.has_focus(editor_id)));
    assert!(harness.app.virtual_editor_state.has_focus);
    assert_eq!(harness.app.virtual_editor_state.cursor(), 0);
}

#[test]
fn virtual_editor_enter_and_select_all_work_after_idle_frames() {
    let mut harness = make_app();
    harness.app.editor_mode = EditorMode::VirtualEditor;
    harness.app.reset_virtual_editor("alpha\n// beta\n");
    harness.app.focus_editor_next = true;

    let ctx = egui::Context::default();
    configure_virtual_editor_test_ctx(&ctx);
    let editor_id = egui::Id::new(VIRTUAL_EDITOR_ID);

    let _ = run_virtual_editor_frame(&mut harness.app, &ctx, Vec::new());
    assert!(ctx.memory(|m| m.has_focus(editor_id)));
    assert!(harness.app.virtual_editor_state.has_focus);

    for _ in 0..6 {
        let focus_active_pre = run_virtual_editor_frame(&mut harness.app, &ctx, Vec::new());
        assert!(focus_active_pre);
        assert!(ctx.memory(|m| m.has_focus(editor_id)));
        assert!(harness.app.virtual_editor_state.has_focus);
    }

    let len = harness.app.virtual_editor_buffer.len_chars();
    let insert_at = harness.app.virtual_editor_buffer.line_col_to_char(0, 5);
    harness.app.virtual_editor_state.set_cursor(insert_at, len);
    let enter_event = key_event(egui::Key::Enter, egui::Modifiers::default());
    let focus_active_pre = run_virtual_editor_frame(&mut harness.app, &ctx, vec![enter_event]);
    assert!(focus_active_pre);
    assert_eq!(
        harness.app.virtual_editor_buffer.to_string(),
        "alpha\n\n// beta\n"
    );

    for _ in 0..3 {
        let focus_active_pre = run_virtual_editor_frame(&mut harness.app, &ctx, Vec::new());
        assert!(focus_active_pre);
        assert!(ctx.memory(|m| m.has_focus(editor_id)));
    }

    let select_all_event = key_event(
        egui::Key::A,
        egui::Modifiers {
            command: true,
            ..Default::default()
        },
    );
    let focus_active_pre = run_virtual_editor_frame(&mut harness.app, &ctx, vec![select_all_event]);
    assert!(focus_active_pre);
    let full_len = harness.app.virtual_editor_buffer.len_chars();
    assert_eq!(
        harness.app.virtual_editor_state.selection_range(),
        Some(0..full_len)
    );
}

#[test]
fn virtual_vertical_move_target_matrix() {
    struct Case {
        text: &'static str,
        wrap_width: f32,
        start_line: usize,
        start_col: usize,
        desired_col: usize,
        up: bool,
        expected_line: usize,
        expected_col: usize,
    }

    let cases = [
        Case {
            text: "aaaa\nbbbb\ncccc\n",
            wrap_width: 200.0,
            start_line: 1,
            start_col: 2,
            desired_col: 2,
            up: true,
            expected_line: 0,
            expected_col: 2,
        },
        Case {
            text: "aaaa\nbbbb\ncccc\n",
            wrap_width: 200.0,
            start_line: 1,
            start_col: 2,
            desired_col: 2,
            up: false,
            expected_line: 2,
            expected_col: 2,
        },
        Case {
            text: "abcdefghij\nabcde\n",
            wrap_width: 200.0,
            start_line: 0,
            start_col: 8,
            desired_col: 8,
            up: false,
            expected_line: 1,
            expected_col: 5,
        },
    ];

    for case in cases {
        let mut harness = make_app();
        configure_virtual_editor_with_wrap(&mut harness.app, case.text, case.wrap_width);
        let start = harness
            .app
            .virtual_editor_buffer
            .line_col_to_char(case.start_line, case.start_col);
        let moved = harness.app.virtual_move_vertical_target(
            start,
            case.desired_col,
            case.up,
            WrapBoundaryAffinity::Downstream,
        );
        let (line, col) = harness.app.virtual_editor_buffer.char_to_line_col(moved);
        assert_eq!((line, col), (case.expected_line, case.expected_col));
    }
}

#[test]
fn wrap_boundary_navigation_command_matrix() {
    struct Case {
        text: &'static str,
        start_line: usize,
        start_col: usize,
        commands: Vec<VirtualInputCommand>,
        expected_line: usize,
        expected_col: usize,
    }

    let cases = [
        Case {
            text: "abcd\nab\n",
            start_line: 0,
            start_col: 4,
            commands: vec![VirtualInputCommand::MoveDown { select: false }],
            expected_line: 1,
            expected_col: 2,
        },
        Case {
            text: "wxyz\nabcdefgh\n",
            start_line: 1,
            start_col: 8,
            commands: vec![
                VirtualInputCommand::MoveUp { select: false },
                VirtualInputCommand::MoveUp { select: false },
            ],
            expected_line: 0,
            expected_col: 4,
        },
    ];

    for case in cases {
        let mut harness = make_app();
        configure_virtual_editor_with_wrap(&mut harness.app, case.text, 4.0);
        set_virtual_cursor_at(&mut harness.app, case.start_line, case.start_col);
        harness.app.virtual_editor_state.clear_preferred_column();
        let ctx = egui::Context::default();
        for command in case.commands {
            let _ = harness.app.apply_virtual_commands(&ctx, &[command]);
        }

        let (line, col) = harness
            .app
            .virtual_editor_buffer
            .char_to_line_col(harness.app.virtual_editor_state.cursor());
        assert_eq!((line, col), (case.expected_line, case.expected_col));
    }
}

#[test]
fn page_navigation_initializes_preferred_column_from_current_cursor() {
    let mut harness = make_app();
    configure_virtual_editor_with_wrap(
        &mut harness.app,
        "0123456789\nabcdefghij\nklmnopqrst\n",
        200.0,
    );
    harness.app.virtual_viewport_height = 1.0;
    harness.app.virtual_line_height = 1.0;

    let len = harness.app.virtual_editor_buffer.len_chars();
    let start = harness.app.virtual_editor_buffer.line_col_to_char(0, 5);
    harness.app.virtual_editor_state.set_cursor(start, len);
    harness.app.virtual_editor_state.clear_preferred_column();

    let ctx = egui::Context::default();
    let _ = harness
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::PageDown { select: false }]);
    let (line, col) = harness
        .app
        .virtual_editor_buffer
        .char_to_line_col(harness.app.virtual_editor_state.cursor());
    assert_eq!((line, col), (1, 5));

    harness.app.virtual_editor_state.clear_preferred_column();
    let _ = harness
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::PageUp { select: false }]);
    let (line, col) = harness
        .app
        .virtual_editor_buffer
        .char_to_line_col(harness.app.virtual_editor_state.cursor());
    assert_eq!((line, col), (0, 5));
}

#[test]
fn long_line_navigation_commands_cross_legacy_render_cap_without_truncation() {
    let mut harness = make_app();
    let text = format!(
        "{}\n",
        "a".repeat(MAX_RENDER_CHARS_PER_LINE.saturating_add(64))
    );
    configure_virtual_editor_with_wrap(&mut harness.app, text.as_str(), 50000.0);

    let len = harness.app.virtual_editor_buffer.len_chars();
    let legacy_cap = harness
        .app
        .virtual_editor_buffer
        .line_col_to_char(0, MAX_RENDER_CHARS_PER_LINE);
    harness.app.virtual_editor_state.set_cursor(legacy_cap, len);
    let ctx = egui::Context::default();
    let line_end = harness
        .app
        .virtual_editor_buffer
        .line_col_to_char(0, text.chars().count().saturating_sub(1));

    let right = harness.app.apply_virtual_commands(
        &ctx,
        &[VirtualInputCommand::MoveRight {
            select: false,
            word: false,
        }],
    );
    assert!(!right.changed);
    assert_eq!(
        harness.app.virtual_editor_state.cursor(),
        legacy_cap.saturating_add(1)
    );

    let move_end = harness
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::MoveEnd { select: false }]);
    assert!(!move_end.changed);
    assert_eq!(harness.app.virtual_editor_state.cursor(), line_end);

    harness.app.virtual_editor_state.set_cursor(legacy_cap, len);
    let delete_tail = harness
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::DeleteForward { word: true }]);
    assert!(delete_tail.changed);
    assert_eq!(
        harness.app.virtual_editor_buffer.line_len_chars(0),
        MAX_RENDER_CHARS_PER_LINE
    );
    assert_eq!(harness.app.virtual_editor_state.cursor(), legacy_cap);
}

#[test]
fn word_navigation_crosses_line_boundaries() {
    let mut harness = make_app();
    configure_virtual_editor_with_wrap(&mut harness.app, "alpha\nbeta gamma", 200.0);

    let len = harness.app.virtual_editor_buffer.len_chars();
    let first_line_end = harness.app.virtual_editor_buffer.line_col_to_char(0, 5);
    harness
        .app
        .virtual_editor_state
        .set_cursor(first_line_end, len);
    let ctx = egui::Context::default();

    let _ = harness.app.apply_virtual_commands(
        &ctx,
        &[VirtualInputCommand::MoveRight {
            select: false,
            word: true,
        }],
    );
    assert_eq!(
        harness.app.virtual_editor_state.cursor(),
        harness.app.virtual_editor_buffer.line_col_to_char(1, 4)
    );

    let _ = harness.app.apply_virtual_commands(
        &ctx,
        &[VirtualInputCommand::MoveLeft {
            select: false,
            word: true,
        }],
    );
    assert_eq!(
        harness.app.virtual_editor_state.cursor(),
        harness.app.virtual_editor_buffer.line_col_to_char(1, 0)
    );
}

#[test]
fn word_delete_crosses_line_boundaries() {
    let ctx = egui::Context::default();

    let mut forward = make_app();
    configure_virtual_editor_with_wrap(&mut forward.app, "alpha\nbeta gamma", 200.0);
    let forward_len = forward.app.virtual_editor_buffer.len_chars();
    let first_line_end = forward.app.virtual_editor_buffer.line_col_to_char(0, 5);
    forward
        .app
        .virtual_editor_state
        .set_cursor(first_line_end, forward_len);
    let forward_result = forward
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::DeleteForward { word: true }]);
    assert!(forward_result.changed);
    assert_eq!(forward.app.virtual_editor_buffer.to_string(), "alpha gamma");

    let mut backward = make_app();
    configure_virtual_editor_with_wrap(&mut backward.app, "alpha\nbeta gamma", 200.0);
    let backward_len = backward.app.virtual_editor_buffer.len_chars();
    let second_line_start = backward.app.virtual_editor_buffer.line_col_to_char(1, 0);
    backward
        .app
        .virtual_editor_state
        .set_cursor(second_line_start, backward_len);
    let backward_result = backward
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::Backspace { word: true }]);
    assert!(backward_result.changed);
    assert_eq!(backward.app.virtual_editor_buffer.to_string(), "beta gamma");
}

#[test]
fn undo_restores_full_cursor_for_long_lines() {
    let mut harness = make_app();
    let long_line = "a".repeat(MAX_RENDER_CHARS_PER_LINE.saturating_add(64));
    configure_virtual_editor_with_wrap(&mut harness.app, long_line.as_str(), 50000.0);

    harness.app.virtual_select_line(0);
    let long_line_end = harness
        .app
        .virtual_editor_buffer
        .line_col_to_char(0, long_line.chars().count());
    assert_eq!(harness.app.virtual_editor_state.cursor(), long_line_end);

    let ctx = egui::Context::default();
    let replace_result = harness
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::InsertText("z".to_string())]);
    assert!(replace_result.changed);
    assert_eq!(harness.app.virtual_editor_buffer.to_string(), "z");

    let undo_result = harness
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::Undo]);
    assert!(undo_result.changed);
    assert_eq!(harness.app.virtual_editor_buffer.to_string(), long_line);

    assert_eq!(harness.app.virtual_editor_state.cursor(), long_line_end);

    let insert_result = harness
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::InsertText("y".to_string())]);
    assert!(insert_result.changed);
    assert_eq!(
        harness.app.virtual_editor_buffer.to_string(),
        format!("{long_line}y")
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
fn drag_autoscroll_delta_direction_matches_pointer_position() {
    enum DeltaDirection {
        Positive,
        Negative,
        Zero,
    }

    let cases = [
        (80.0, DeltaDirection::Positive),
        (260.0, DeltaDirection::Negative),
        (150.0, DeltaDirection::Zero),
    ];

    for (pointer_y, expected_direction) in cases {
        let delta = drag_autoscroll_delta(pointer_y, 100.0, 220.0, 20.0);
        match expected_direction {
            DeltaDirection::Positive => assert!(delta > 0.0),
            DeltaDirection::Negative => assert!(delta < 0.0),
            DeltaDirection::Zero => assert_eq!(delta, 0.0),
        }
    }
}

#[test]
fn word_range_at_selects_word() {
    let text = "hello world";
    let (start, end) = word_range_at(text, 1).expect("range");
    let selected: String = text.chars().skip(start).take(end - start).collect();
    assert_eq!(selected, "hello");
}

#[test]
fn caret_blink_reset_behavior_depends_on_cursor_or_text_change() {
    enum Expectation {
        Reset,
        Unchanged,
    }

    let cases = [
        (
            vec![VirtualInputCommand::MoveRight {
                select: false,
                word: false,
            }],
            Expectation::Reset,
        ),
        (vec![VirtualInputCommand::Copy], Expectation::Unchanged),
    ];

    for (commands, expected) in cases {
        let mut harness = make_app();
        harness.app.reset_virtual_editor("ab");
        harness.app.editor_mode = EditorMode::VirtualEditor;
        let before = Instant::now() - Duration::from_secs(3);
        harness.app.virtual_caret_phase_start = before;

        let ctx = egui::Context::default();
        let _ = harness
            .app
            .apply_virtual_commands(&ctx, commands.as_slice());

        match expected {
            Expectation::Reset => assert!(harness.app.virtual_caret_phase_start > before),
            Expectation::Unchanged => assert_eq!(harness.app.virtual_caret_phase_start, before),
        }
    }
}
