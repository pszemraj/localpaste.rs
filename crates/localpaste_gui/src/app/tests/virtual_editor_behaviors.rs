//! Virtual editor input/editing tests including IME and selection behavior.

use super::*;

fn run_virtual_editor_frame(
    app: &mut LocalPasteApp,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
) -> bool {
    let focus_id = egui::Id::new(VIRTUAL_EDITOR_ID);
    let egui_focus_pre = ctx.memory(|m| m.has_focus(focus_id));
    let focus_active_pre = app.is_virtual_editor_mode() && egui_focus_pre;

    let raw_input = egui::RawInput {
        events,
        ..Default::default()
    };
    let _ = ctx.run(raw_input, |ctx| {
        app.render_editor_panel(ctx);
    });

    focus_active_pre
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
    assert_eq!(harness.app.virtual_editor_state.cursor(), 0);
}

#[test]
fn same_frame_editor_click_and_arrow_moves_cursor_once() {
    let mut harness = make_app();
    harness.app.editor_mode = EditorMode::VirtualEditor;
    harness.app.reset_virtual_editor("alpha\n");

    let ctx = egui::Context::default();
    configure_virtual_editor_test_ctx(&ctx);
    let screen_rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1200.0, 900.0));
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
                key_event(egui::Key::ArrowRight, egui::Modifiers::default()),
            ],
            ..Default::default()
        },
    );

    let editor_id = egui::Id::new(VIRTUAL_EDITOR_ID);
    assert!(ctx.memory(|m| m.has_focus(editor_id)));
    assert_eq!(harness.app.virtual_editor_state.cursor(), 1);
    assert!(harness.app.virtual_editor_state.selection_range().is_none());
}

#[test]
fn stale_virtual_focus_does_not_steal_arrow_from_other_focus_owner() {
    let mut harness = make_app();
    harness.app.editor_mode = EditorMode::VirtualEditor;
    harness.app.reset_virtual_editor("alpha\n");

    let ctx = egui::Context::default();
    configure_virtual_editor_test_ctx(&ctx);
    let other_focus_id = egui::Id::new("title_like_text_input");
    ctx.memory_mut(|m| m.request_focus(other_focus_id));
    let mut title_text = "title".to_string();
    let _ = ctx.run(
        egui::RawInput {
            events: vec![key_event(egui::Key::ArrowRight, egui::Modifiers::default())],
            ..Default::default()
        },
        |ctx| {
            egui::TopBottomPanel::top("title_like_panel").show(ctx, |ui| {
                ui.add(egui::TextEdit::singleline(&mut title_text).id(other_focus_id));
            });
            harness.app.render_editor_panel(ctx);
        },
    );

    let editor_id = egui::Id::new(VIRTUAL_EDITOR_ID);
    assert!(!ctx.memory(|m| m.has_focus(editor_id)));
    assert_eq!(harness.app.virtual_editor_state.cursor(), 0);
}

#[test]
fn focused_virtual_editor_publishes_ime_cursor_rect() {
    let mut harness = make_app();
    harness.app.editor_mode = EditorMode::VirtualEditor;
    harness.app.reset_virtual_editor("alpha\nbeta\n");
    set_virtual_cursor_at(&mut harness.app, 0, 2);

    let ctx = egui::Context::default();
    configure_virtual_editor_test_ctx(&ctx);
    let editor_id = egui::Id::new(VIRTUAL_EDITOR_ID);
    ctx.memory_mut(|m| m.request_focus(editor_id));

    let screen_rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1200.0, 900.0));
    let output = run_editor_panel_once_output(
        &mut harness.app,
        &ctx,
        egui::RawInput {
            screen_rect: Some(screen_rect),
            ..Default::default()
        },
    );

    let ime = output
        .platform_output
        .ime
        .expect("focused virtual editor should publish IME output");
    assert!(ctx.memory(|m| m.has_focus(editor_id)));
    assert!(ime.rect.min.x.is_finite());
    assert!(ime.rect.min.y.is_finite());
    assert!(ime.rect.max.x.is_finite());
    assert!(ime.rect.max.y.is_finite());
    assert!(ime.cursor_rect.min.x.is_finite());
    assert!(ime.cursor_rect.min.y.is_finite());
    assert!(ime.cursor_rect.max.x.is_finite());
    assert!(ime.cursor_rect.max.y.is_finite());
    assert!(ime.rect.width() > 0.0);
    assert!(ime.rect.height() > 0.0);
    assert!(ime.cursor_rect.height() > 0.0);
    assert!(
        ime.rect.contains(ime.cursor_rect.center()),
        "IME cursor should stay inside the focused editor interaction rect"
    );
}

#[test]
fn focused_virtual_editor_owns_tab_without_focus_traversal() {
    let mut harness = make_app();
    harness.app.editor_mode = EditorMode::VirtualEditor;
    harness.app.reset_virtual_editor("alpha");

    let ctx = egui::Context::default();
    configure_virtual_editor_test_ctx(&ctx);
    let editor_id = egui::Id::new(VIRTUAL_EDITOR_ID);
    let other_focus_id = egui::Id::new("tab_traversal_target");
    ctx.memory_mut(|m| m.request_focus(editor_id));

    let mut other_text = "other".to_string();
    let screen_rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1200.0, 900.0));
    let mut render_with_neighbor = |events| {
        ctx.run(
            egui::RawInput {
                events,
                screen_rect: Some(screen_rect),
                ..Default::default()
            },
            |ctx| {
                egui::TopBottomPanel::top("tab_traversal_neighbor").show(ctx, |ui| {
                    ui.add(egui::TextEdit::singleline(&mut other_text).id(other_focus_id));
                });
                harness.app.render_editor_panel(ctx);
            },
        )
    };

    let _ = render_with_neighbor(Vec::new());
    assert!(ctx.memory(|m| m.has_focus(editor_id)));

    let _ = render_with_neighbor(vec![key_event(egui::Key::Tab, egui::Modifiers::default())]);

    assert!(ctx.memory(|m| m.has_focus(editor_id)));
    assert!(!ctx.memory(|m| m.has_focus(other_focus_id)));
    assert_eq!(harness.app.virtual_editor_buffer.to_string(), "    alpha");
    assert_eq!(other_text, "other");
}

#[test]
fn focused_editor_delete_chord_edits_text_without_dispatching_paste_delete() {
    let mut harness = make_app();
    harness.app.editor_mode = EditorMode::VirtualEditor;
    harness.app.reset_virtual_editor("alpha beta");
    let len = harness.app.virtual_editor_buffer.len_chars();
    harness.app.virtual_editor_state.set_cursor(0, len);

    let ctx = egui::Context::default();
    configure_virtual_editor_test_ctx(&ctx);
    let editor_id = egui::Id::new(VIRTUAL_EDITOR_ID);
    ctx.memory_mut(|m| m.request_focus(editor_id));
    run_full_update(&mut harness.app, &ctx, Vec::new());
    assert!(ctx.memory(|m| m.has_focus(editor_id)));

    #[cfg(target_os = "macos")]
    let (delete_event, expected_text) = (
        key_event(
            egui::Key::Delete,
            egui::Modifiers {
                command: true,
                ..Default::default()
            },
        ),
        "",
    );
    #[cfg(not(target_os = "macos"))]
    let (delete_event, expected_text) = (
        key_event(
            egui::Key::Delete,
            egui::Modifiers {
                ctrl: true,
                command: true,
                ..Default::default()
            },
        ),
        "beta",
    );

    run_full_update(&mut harness.app, &ctx, vec![delete_event]);

    assert!(ctx.memory(|m| m.has_focus(editor_id)));
    assert_eq!(harness.app.virtual_editor_buffer.to_string(), expected_text);
    assert!(matches!(
        harness.cmd_rx.try_recv(),
        Err(TryRecvError::Empty)
    ));
}

#[test]
fn focused_editor_keeps_command_arrow_focus_inside_real_app_chrome() {
    let mut harness = make_app();
    harness.app.editor_mode = EditorMode::VirtualEditor;
    harness
        .app
        .reset_virtual_editor("alpha\nbeta gamma\ndelta\n");
    set_virtual_cursor_at(&mut harness.app, 1, 4);

    let ctx = egui::Context::default();
    configure_virtual_editor_test_ctx(&ctx);
    let editor_id = egui::Id::new(VIRTUAL_EDITOR_ID);
    let search_id = egui::Id::new(SEARCH_INPUT_ID);
    let screen_rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1200.0, 900.0));

    ctx.memory_mut(|m| m.request_focus(editor_id));
    let _ = run_full_update_with_input(
        &mut harness.app,
        &ctx,
        egui::RawInput {
            screen_rect: Some(screen_rect),
            ..Default::default()
        },
    );
    assert!(ctx.memory(|m| m.has_focus(editor_id)));

    let command_only = egui::Modifiers {
        command: true,
        ..Default::default()
    };
    let command_shift = egui::Modifiers {
        command: true,
        shift: true,
        ..Default::default()
    };
    let cases = [
        ("command-left", egui::Key::ArrowLeft, command_only),
        ("command-up", egui::Key::ArrowUp, command_only),
        ("command-right", egui::Key::ArrowRight, command_only),
        ("command-down", egui::Key::ArrowDown, command_only),
        ("command-shift-left", egui::Key::ArrowLeft, command_shift),
        ("command-shift-up", egui::Key::ArrowUp, command_shift),
    ];

    for (name, key, modifiers) in cases {
        let _ = run_full_update_with_input(
            &mut harness.app,
            &ctx,
            egui::RawInput {
                screen_rect: Some(screen_rect),
                events: vec![key_event(key, modifiers)],
                modifiers,
                ..Default::default()
            },
        );

        let len = harness.app.virtual_editor_buffer.len_chars();
        let cursor = harness.app.virtual_editor_state.cursor();
        assert!(
            ctx.memory(|m| m.has_focus(editor_id)),
            "{name} should leave keyboard focus on the virtual editor"
        );
        assert!(
            !ctx.memory(|m| m.has_focus(search_id)),
            "{name} should not move focus to sidebar search"
        );
        assert!(
            cursor <= len,
            "{name} left cursor {cursor} outside buffer length {len}"
        );
        if let Some(range) = harness.app.virtual_editor_state.selection_range() {
            assert!(
                range.start <= range.end && range.end <= len,
                "{name} left selection {:?} outside buffer length {len}",
                range
            );
        }
    }
}

#[test]
fn virtual_editor_frame_consumes_pending_follow_scroll_offset() {
    let mut harness = make_app();
    harness.app.editor_mode = EditorMode::VirtualEditor;
    harness
        .app
        .reset_virtual_editor("line one\nline two\nline three\n");
    harness.app.virtual_pending_scroll_offset_y = Some(240.0);

    let ctx = egui::Context::default();
    configure_virtual_editor_test_ctx(&ctx);
    run_editor_panel_once(&mut harness.app, &ctx, egui::RawInput::default());

    assert!(
        harness.app.virtual_pending_scroll_offset_y.is_none(),
        "virtual editor frame should consume queued scroll-follow offset"
    );
}

#[test]
fn virtual_editor_enter_and_select_all_work_after_idle_frames() {
    let mut harness = make_app();
    harness.app.editor_mode = EditorMode::VirtualEditor;
    harness.app.reset_virtual_editor("alpha\n// beta\n");

    let ctx = egui::Context::default();
    configure_virtual_editor_test_ctx(&ctx);
    let editor_id = egui::Id::new(VIRTUAL_EDITOR_ID);
    ctx.memory_mut(|m| m.request_focus(editor_id));

    let _ = run_virtual_editor_frame(&mut harness.app, &ctx, Vec::new());
    assert!(ctx.memory(|m| m.has_focus(editor_id)));

    for _ in 0..6 {
        let focus_active_pre = run_virtual_editor_frame(&mut harness.app, &ctx, Vec::new());
        assert!(focus_active_pre);
        assert!(ctx.memory(|m| m.has_focus(editor_id)));
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

    let select_all_event = key_event(egui::Key::A, primary_command_modifiers());
    let focus_active_pre = run_virtual_editor_frame(&mut harness.app, &ctx, vec![select_all_event]);
    assert!(focus_active_pre);
    let full_len = harness.app.virtual_editor_buffer.len_chars();
    assert_eq!(
        harness.app.virtual_editor_state.selection_range(),
        Some(0..full_len)
    );
}

#[test]
fn virtual_editor_enter_in_focused_frame_inserts_top_newline() {
    let mut harness = make_app();
    harness.app.editor_mode = EditorMode::VirtualEditor;
    harness.app.reset_virtual_editor("alpha\nbeta\n");

    let ctx = egui::Context::default();
    configure_virtual_editor_test_ctx(&ctx);
    let editor_id = egui::Id::new(VIRTUAL_EDITOR_ID);
    ctx.memory_mut(|m| m.request_focus(editor_id));
    let enter_event = key_event(egui::Key::Enter, egui::Modifiers::default());
    let focus_active_pre = run_virtual_editor_frame(&mut harness.app, &ctx, vec![enter_event]);

    assert!(focus_active_pre);
    assert_eq!(
        harness.app.virtual_editor_buffer.to_string(),
        "\nalpha\nbeta\n"
    );
    assert_eq!(harness.app.virtual_editor_state.cursor(), 1);
}

#[test]
fn virtual_editor_shift_arrow_in_focused_frame_extends_selection() {
    let mut harness = make_app();
    harness.app.editor_mode = EditorMode::VirtualEditor;
    harness.app.reset_virtual_editor("alpha\n");

    let ctx = egui::Context::default();
    configure_virtual_editor_test_ctx(&ctx);
    let editor_id = egui::Id::new(VIRTUAL_EDITOR_ID);
    ctx.memory_mut(|m| m.request_focus(editor_id));
    let shift_right = key_event(
        egui::Key::ArrowRight,
        egui::Modifiers {
            shift: true,
            ..Default::default()
        },
    );
    let focus_active_pre = run_virtual_editor_frame(&mut harness.app, &ctx, vec![shift_right]);

    assert!(focus_active_pre);
    assert_eq!(harness.app.virtual_editor_state.cursor(), 1);
    assert_eq!(
        harness.app.virtual_editor_state.selection_range(),
        Some(0..1)
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
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::MoveLineEnd { select: false }]);
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
    let expected_after_right = harness.app.virtual_editor_buffer.line_col_to_char(1, 0);

    assert_eq!(
        harness.app.virtual_editor_state.cursor(),
        expected_after_right
    );

    let _ = harness.app.apply_virtual_commands(
        &ctx,
        &[VirtualInputCommand::MoveLeft {
            select: false,
            word: true,
        }],
    );
    let expected_after_left = harness.app.virtual_editor_buffer.line_col_to_char(0, 0);

    assert_eq!(
        harness.app.virtual_editor_state.cursor(),
        expected_after_left
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
    assert_eq!(
        forward.app.virtual_editor_buffer.to_string(),
        "alphabeta gamma"
    );

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
fn word_delete_forward_matches_word_navigation_boundaries() {
    let ctx = egui::Context::default();
    let mut harness = make_app();
    configure_virtual_editor_with_wrap(&mut harness.app, "foo bar", 200.0);

    let len = harness.app.virtual_editor_buffer.len_chars();
    harness.app.virtual_editor_state.set_cursor(0, len);
    let start_word = harness
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::DeleteForward { word: true }]);
    assert!(start_word.changed);
    assert_eq!(harness.app.virtual_editor_buffer.to_string(), "bar");

    harness.app.reset_virtual_editor("foo bar");
    harness
        .app
        .virtual_layout
        .rebuild(&harness.app.virtual_editor_buffer, 200.0, 1.0, 1.0);
    let len = harness.app.virtual_editor_buffer.len_chars();
    harness.app.virtual_editor_state.set_cursor(3, len);
    let separator = harness
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::DeleteForward { word: true }]);
    assert!(separator.changed);
    assert_eq!(harness.app.virtual_editor_buffer.to_string(), "foobar");
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
    let mut command_modifiers = primary_command_modifiers();
    command_modifiers.shift = false;
    let events = vec![
        egui::Event::Text("X".to_string()),
        egui::Event::Paste("ZZ".to_string()),
        egui::Event::Copy,
        egui::Event::Cut,
        egui::Event::Ime(egui::ImeEvent::Enabled),
        egui::Event::Ime(egui::ImeEvent::Preedit("Z".to_string())),
        egui::Event::Ime(egui::ImeEvent::Commit("Z".to_string())),
        egui::Event::Ime(egui::ImeEvent::Disabled),
        key_event(egui::Key::Delete, egui::Modifiers::default()),
        key_event(egui::Key::A, command_modifiers),
    ];

    assert!(commands_from_events(&events, false).is_empty());
}

#[test]
fn virtual_click_counter_promotes_to_triple_and_resets_on_timeout_or_distance() {
    let now = Instant::now();
    let p = egui::pos2(100.0, 200.0);
    let c1 = next_virtual_click_count(None, None, 0, p, now);
    assert_eq!(c1, 1);
    let c2 = next_virtual_click_count(Some(now), Some(p), c1, p, now);
    assert_eq!(c2, 2);
    let c3 = next_virtual_click_count(Some(now), Some(p), c2, p, now);
    assert_eq!(c3, 3);

    let expired = next_virtual_click_count(
        Some(now),
        Some(p),
        c3,
        p,
        now + EDITOR_DOUBLE_CLICK_WINDOW + Duration::from_millis(1),
    );
    assert_eq!(expired, 1);

    let far = egui::pos2(100.0 + EDITOR_DOUBLE_CLICK_DISTANCE + 1.0, 200.0);
    let distant = next_virtual_click_count(Some(now), Some(p), c3, far, now);
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
