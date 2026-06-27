//! Virtual editor focus, caret, and full-frame keyboard ownership tests.

use super::*;

fn screen_rect() -> egui::Rect {
    egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1200.0, 900.0))
}

fn primary_click_events(pos: egui::Pos2) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::default(),
        },
    ]
}

fn focused_word_select_modifiers() -> egui::Modifiers {
    #[cfg(target_os = "macos")]
    {
        egui::Modifiers {
            alt: true,
            shift: true,
            ..Default::default()
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        egui::Modifiers {
            ctrl: true,
            command: true,
            shift: true,
            ..Default::default()
        }
    }
}

fn assert_editor_focus(ctx: &egui::Context) {
    assert!(ctx.memory(|m| m.has_focus(egui::Id::new(VIRTUAL_EDITOR_ID))));
}

#[test]
fn click_in_editor_viewport_without_row_hit_places_cursor_at_eof() {
    let mut harness = make_app();
    harness.app.reset_virtual_editor("line one\n");

    let ctx = egui::Context::default();
    configure_virtual_editor_test_ctx(&ctx);
    let editor_id = egui::Id::new(VIRTUAL_EDITOR_ID);

    run_editor_panel_once(
        &mut harness.app,
        &ctx,
        egui::RawInput {
            screen_rect: Some(screen_rect()),
            ..Default::default()
        },
    );
    assert!(!ctx.memory(|m| m.has_focus(editor_id)));

    let output = run_editor_panel_once_output(
        &mut harness.app,
        &ctx,
        egui::RawInput {
            screen_rect: Some(screen_rect()),
            events: primary_click_events(egui::pos2(240.0, 700.0)),
            ..Default::default()
        },
    );

    assert_editor_focus(&ctx);
    assert_eq!(
        harness.app.virtual_editor_state.cursor(),
        harness.app.virtual_editor_buffer.len_chars()
    );
    let ime = output
        .platform_output
        .ime
        .expect("blank editor click should publish focused IME cursor output");
    assert!(ime.cursor_rect.height() > 0.0);
    assert!(
        output
            .viewport_output
            .values()
            .any(|viewport| viewport.repaint_delay == Duration::ZERO),
        "blank editor click must repaint the newly focused caret"
    );
}

#[test]
fn same_frame_blank_editor_click_and_arrow_moves_from_eof() {
    let mut harness = make_app();
    harness.app.reset_virtual_editor("alpha\n");

    let ctx = egui::Context::default();
    configure_virtual_editor_test_ctx(&ctx);
    run_editor_panel_once(
        &mut harness.app,
        &ctx,
        egui::RawInput {
            screen_rect: Some(screen_rect()),
            events: {
                let mut events = primary_click_events(egui::pos2(240.0, 700.0));
                events.push(key_event(egui::Key::ArrowLeft, egui::Modifiers::default()));
                events
            },
            ..Default::default()
        },
    );

    assert_editor_focus(&ctx);
    let expected = harness
        .app
        .virtual_editor_buffer
        .len_chars()
        .saturating_sub(1);
    assert_eq!(harness.app.virtual_editor_state.cursor(), expected);
    assert!(harness.app.virtual_editor_state.selection_range().is_none());
}

#[test]
fn same_frame_row_click_and_arrow_starts_from_clicked_cursor() {
    let mut candidate_points = [16.0, 40.0, 80.0, 140.0, 240.0]
        .into_iter()
        .flat_map(|x| (0..240).map(move |idx| egui::pos2(x, 20.0 + idx as f32)));
    let (click_pos, clicked_cursor, len) = candidate_points
        .find_map(|click_pos| {
            let mut click_only = make_app();
            click_only.app.reset_virtual_editor("abcdef\n");
            let click_ctx = egui::Context::default();
            configure_virtual_editor_test_ctx(&click_ctx);
            run_editor_panel_once(
                &mut click_only.app,
                &click_ctx,
                egui::RawInput {
                    screen_rect: Some(screen_rect()),
                    ..Default::default()
                },
            );
            run_editor_panel_once(
                &mut click_only.app,
                &click_ctx,
                egui::RawInput {
                    screen_rect: Some(screen_rect()),
                    events: primary_click_events(click_pos),
                    ..Default::default()
                },
            );
            let cursor = click_only.app.virtual_editor_state.cursor();
            let len = click_only.app.virtual_editor_buffer.len_chars();
            let focused = click_ctx.memory(|m| m.has_focus(egui::Id::new(VIRTUAL_EDITOR_ID)));
            (focused && cursor < len).then_some((click_pos, cursor, len))
        })
        .expect("candidate click should hit a rendered editor row");

    let mut same_frame = make_app();
    same_frame.app.reset_virtual_editor("abcdef\n");
    let same_frame_ctx = egui::Context::default();
    configure_virtual_editor_test_ctx(&same_frame_ctx);
    run_editor_panel_once(
        &mut same_frame.app,
        &same_frame_ctx,
        egui::RawInput {
            screen_rect: Some(screen_rect()),
            ..Default::default()
        },
    );
    run_editor_panel_once(
        &mut same_frame.app,
        &same_frame_ctx,
        egui::RawInput {
            screen_rect: Some(screen_rect()),
            events: {
                let mut events = primary_click_events(click_pos);
                events.push(key_event(egui::Key::ArrowRight, egui::Modifiers::default()));
                events
            },
            ..Default::default()
        },
    );

    assert_editor_focus(&same_frame_ctx);
    assert_eq!(
        same_frame.app.virtual_editor_state.cursor(),
        clicked_cursor.saturating_add(1).min(len)
    );
}

#[test]
fn settled_focus_plain_arrows_move_cursor_inside_editor_without_sidebar_selection() {
    let mut harness = make_app();
    harness.app.reset_virtual_editor("abcd\nefgh\n");
    set_virtual_cursor_at(&mut harness.app, 0, 1);

    let ctx = egui::Context::default();
    configure_virtual_editor_test_ctx(&ctx);
    let selected_before = harness.app.selected_id.clone();
    ctx.memory_mut(|m| m.request_focus(egui::Id::new(VIRTUAL_EDITOR_ID)));
    let _ = run_full_update_with_input(
        &mut harness.app,
        &ctx,
        egui::RawInput {
            screen_rect: Some(screen_rect()),
            ..Default::default()
        },
    );
    assert_editor_focus(&ctx);

    let cases = [
        (egui::Key::ArrowRight, (0, 2)),
        (egui::Key::ArrowDown, (1, 2)),
        (egui::Key::ArrowLeft, (1, 1)),
        (egui::Key::ArrowUp, (0, 1)),
    ];
    for (key, expected) in cases {
        let _ = run_full_update_with_input(
            &mut harness.app,
            &ctx,
            egui::RawInput {
                screen_rect: Some(screen_rect()),
                events: vec![key_event(key, egui::Modifiers::default())],
                ..Default::default()
            },
        );
        assert_editor_focus(&ctx);
        assert_eq!(harness.app.selected_id, selected_before);
        assert!(harness.app.virtual_editor_state.selection_range().is_none());
        let cursor = harness
            .app
            .virtual_editor_buffer
            .char_to_line_col(harness.app.virtual_editor_state.cursor());
        assert_eq!(cursor, expected);
    }
}

#[test]
fn stale_virtual_focus_does_not_steal_arrow_from_other_focus_owner() {
    let mut harness = make_app();
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

    assert!(!ctx.memory(|m| m.has_focus(egui::Id::new(VIRTUAL_EDITOR_ID))));
    assert_eq!(harness.app.virtual_editor_state.cursor(), 0);
}

#[test]
fn focused_virtual_editor_publishes_ime_cursor_rect() {
    let mut harness = make_app();
    harness.app.reset_virtual_editor("alpha\nbeta\n");
    set_virtual_cursor_at(&mut harness.app, 0, 2);

    let ctx = egui::Context::default();
    configure_virtual_editor_test_ctx(&ctx);
    ctx.memory_mut(|m| m.request_focus(egui::Id::new(VIRTUAL_EDITOR_ID)));

    let output = run_editor_panel_once_output(
        &mut harness.app,
        &ctx,
        egui::RawInput {
            screen_rect: Some(screen_rect()),
            ..Default::default()
        },
    );

    let ime = output
        .platform_output
        .ime
        .expect("focused virtual editor should publish IME output");
    assert_editor_focus(&ctx);
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
fn focused_virtual_editor_publishes_ime_cursor_rect_when_caret_is_offscreen() {
    let mut harness = make_app();
    let content = (0..180)
        .map(|idx| format!("line {idx}"))
        .collect::<Vec<_>>()
        .join("\n");
    harness.app.reset_virtual_editor(content.as_str());
    set_virtual_cursor_at(&mut harness.app, 150, 2);

    let ctx = egui::Context::default();
    configure_virtual_editor_test_ctx(&ctx);
    ctx.memory_mut(|m| m.request_focus(egui::Id::new(VIRTUAL_EDITOR_ID)));

    let output = run_editor_panel_once_output(
        &mut harness.app,
        &ctx,
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(900.0, 260.0),
            )),
            ..Default::default()
        },
    );

    let ime = output
        .platform_output
        .ime
        .expect("focused virtual editor should publish IME output even when caret row is hidden");
    assert_editor_focus(&ctx);
    assert!(ime.cursor_rect.min.y.is_finite());
    assert!(ime.cursor_rect.height() > 0.0);
    assert!(
        ime.rect.contains(ime.cursor_rect.center()),
        "offscreen caret fallback should stay clamped inside the editor"
    );
}

#[test]
fn focused_virtual_editor_owns_tab_without_focus_traversal() {
    let mut harness = make_app();
    harness.app.reset_virtual_editor("alpha");

    let ctx = egui::Context::default();
    configure_virtual_editor_test_ctx(&ctx);
    let editor_id = egui::Id::new(VIRTUAL_EDITOR_ID);
    let other_focus_id = egui::Id::new("tab_traversal_target");
    ctx.memory_mut(|m| m.request_focus(editor_id));

    let mut other_text = "other".to_string();
    let mut render_with_neighbor = |events| {
        ctx.run(
            egui::RawInput {
                events,
                screen_rect: Some(screen_rect()),
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
    assert_editor_focus(&ctx);

    let _ = render_with_neighbor(vec![key_event(egui::Key::Tab, egui::Modifiers::default())]);

    assert_editor_focus(&ctx);
    assert!(!ctx.memory(|m| m.has_focus(other_focus_id)));
    assert_eq!(harness.app.virtual_editor_buffer.to_string(), "    alpha");
    assert_eq!(other_text, "other");
}

#[test]
fn focused_editor_keeps_platform_mod_arrow_focus_inside_real_app_chrome() {
    let mut harness = make_app();
    harness
        .app
        .reset_virtual_editor("alpha\nbeta gamma\ndelta\n");

    let ctx = egui::Context::default();
    configure_virtual_editor_test_ctx(&ctx);
    let editor_id = egui::Id::new(VIRTUAL_EDITOR_ID);
    let search_id = egui::Id::new(SEARCH_INPUT_ID);
    let selected_before = harness.app.selected_id.clone();
    ctx.memory_mut(|m| m.request_focus(editor_id));
    let _ = run_full_update_with_input(
        &mut harness.app,
        &ctx,
        egui::RawInput {
            screen_rect: Some(screen_rect()),
            ..Default::default()
        },
    );
    assert_editor_focus(&ctx);

    let command_only = primary_command_modifiers();
    let command_shift = egui::Modifiers {
        shift: true,
        ..primary_command_modifiers()
    };
    let cases = [
        ("mod-left", egui::Key::ArrowLeft, command_only, false),
        ("mod-up", egui::Key::ArrowUp, command_only, false),
        ("mod-right", egui::Key::ArrowRight, command_only, false),
        ("mod-down", egui::Key::ArrowDown, command_only, false),
        ("mod-shift-left", egui::Key::ArrowLeft, command_shift, true),
        ("mod-shift-up", egui::Key::ArrowUp, command_shift, true),
        (
            "mod-shift-right",
            egui::Key::ArrowRight,
            command_shift,
            true,
        ),
        ("mod-shift-down", egui::Key::ArrowDown, command_shift, true),
    ];

    for (name, key, modifiers, should_select) in cases {
        set_virtual_cursor_at(&mut harness.app, 1, 4);
        harness.app.virtual_editor_state.clear_preferred_column();
        let start_cursor = harness.app.virtual_editor_state.cursor();
        let _ = run_full_update_with_input(
            &mut harness.app,
            &ctx,
            egui::RawInput {
                screen_rect: Some(screen_rect()),
                events: vec![key_event(key, modifiers)],
                modifiers,
                ..Default::default()
            },
        );

        let len = harness.app.virtual_editor_buffer.len_chars();
        let cursor = harness.app.virtual_editor_state.cursor();
        assert_editor_focus(&ctx);
        assert!(
            !ctx.memory(|m| m.has_focus(search_id)),
            "{name} should not move focus to sidebar search"
        );
        assert_eq!(harness.app.selected_id, selected_before, "{name}");
        assert_ne!(cursor, start_cursor, "{name} should move the editor cursor");
        assert!(
            cursor <= len,
            "{name} left cursor {cursor} outside buffer length {len}"
        );
        assert_eq!(
            harness.app.virtual_editor_state.selection_range().is_some(),
            should_select,
            "{name} selection expectation"
        );
    }
}

#[test]
fn word_selection_delete_keys_stay_in_focused_editor() {
    let cases = [
        (egui::Key::Backspace, "beta gamma"),
        (egui::Key::Delete, "beta gamma"),
    ];

    for (delete_key, expected_text) in cases {
        let mut harness = make_app();
        harness.app.reset_virtual_editor("alpha beta gamma");
        let ctx = egui::Context::default();
        configure_virtual_editor_test_ctx(&ctx);
        ctx.memory_mut(|m| m.request_focus(egui::Id::new(VIRTUAL_EDITOR_ID)));
        let _ = run_full_update_with_input(
            &mut harness.app,
            &ctx,
            egui::RawInput {
                screen_rect: Some(screen_rect()),
                ..Default::default()
            },
        );
        assert_editor_focus(&ctx);

        let word_select = focused_word_select_modifiers();
        let _ = run_full_update_with_input(
            &mut harness.app,
            &ctx,
            egui::RawInput {
                screen_rect: Some(screen_rect()),
                events: vec![key_event(egui::Key::ArrowRight, word_select)],
                modifiers: word_select,
                ..Default::default()
            },
        );
        assert_eq!(
            harness.app.virtual_editor_state.selection_range(),
            Some(0..6)
        );

        let _ = run_full_update_with_input(
            &mut harness.app,
            &ctx,
            egui::RawInput {
                screen_rect: Some(screen_rect()),
                events: vec![key_event(delete_key, egui::Modifiers::default())],
                ..Default::default()
            },
        );
        assert_editor_focus(&ctx);
        assert_eq!(harness.app.virtual_editor_buffer.to_string(), expected_text);
        assert!(harness.app.virtual_editor_state.selection_range().is_none());
        assert!(matches!(
            harness.cmd_rx.try_recv(),
            Err(TryRecvError::Empty)
        ));
    }
}
