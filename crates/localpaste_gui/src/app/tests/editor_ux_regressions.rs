//! Rendered geometry, focus ownership, and block-edit regressions.

use super::*;

fn render_frames(app: &mut LocalPasteApp, ctx: &egui::Context, width: f32) {
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

fn assert_visible(app: &LocalPasteApp) {
    assert!(
        app.virtual_viewport.caret_visible(),
        "caret {:?}, viewport {:?}, offset {}",
        app.virtual_viewport.caret,
        app.virtual_viewport.rect,
        app.virtual_viewport.offset_y
    );
}

#[test]
fn missing_named_editor_style_renders_with_resolved_fallback_font() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    assert!(!ctx
        .style()
        .text_styles
        .contains_key(&TextStyle::Name(EDITOR_TEXT_STYLE.into())));
    render_frames(&mut harness.app, &ctx, 1000.0);
    assert_visible(&harness.app);
    // A native theme replacement can discard the named style after normal startup too.
    harness.app.ensure_style(&ctx);
    ctx.set_style(egui::Style::default());
    render_frames(&mut harness.app, &ctx, 1000.0);
    assert_visible(&harness.app);
}

#[test]
fn loading_another_paste_resets_the_previous_scroll_position_without_focus() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    harness.app.reset_virtual_editor(&"long line\n".repeat(900));
    harness
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::MoveDocEnd { select: false }]);
    render_frames(&mut harness.app, &ctx, 1000.0);
    assert!(harness.app.virtual_viewport.offset_y > 1000.0);

    harness
        .app
        .reset_virtual_editor(&"short paste\n".repeat(18));
    render_frames(&mut harness.app, &ctx, 1000.0);
    assert!(!harness.app.virtual_editor_state.has_focus);
    assert_eq!(harness.app.virtual_editor_state.cursor(), 0);
    assert_eq!(harness.app.virtual_viewport.offset_y, 0.0);
    assert_visible(&harness.app);
}

#[test]
fn long_wrapped_document_reveals_jumps_eof_edits_and_distant_find_without_focus() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    let text = (0..904)
        .map(|i| {
            format!(
                "{i:04} {} café 🦀 {}\n",
                "text ".repeat(22),
                if i % 400 == 0 { "NEEDLE" } else { "paragraph" }
            )
        })
        .collect::<String>();
    assert!(text.chars().count() > 114_000);
    harness.app.reset_virtual_editor(&text);
    for width in [1200.0, 640.0, 950.0] {
        render_frames(&mut harness.app, &ctx, width);
        for command in [
            VirtualInputCommand::MoveDocEnd { select: false },
            VirtualInputCommand::MoveDocHome { select: false },
            VirtualInputCommand::MoveDocEnd { select: false },
        ] {
            harness.app.apply_virtual_commands(&ctx, &[command]);
            render_frames(&mut harness.app, &ctx, width);
            assert_visible(&harness.app);
        }
        for command in [
            VirtualInputCommand::InsertText("EOF λ".into()),
            VirtualInputCommand::Paste("\nnew tail\n".repeat(50)),
        ] {
            harness.app.apply_virtual_commands(&ctx, &[command]);
            render_frames(&mut harness.app, &ctx, width);
            assert_visible(&harness.app);
        }
    }
    // A deliberate scroll away from the caret stays put until another action.
    harness.app.virtual_pending_scroll_offset_y = Some(500.0);
    render_frames(&mut harness.app, &ctx, 950.0);
    assert!((harness.app.virtual_viewport.offset_y - 500.0).abs() < 1.0);
    assert!(!harness.app.virtual_viewport.caret_visible());
    harness
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::MoveDocEnd { select: false }]);
    render_frames(&mut harness.app, &ctx, 950.0);
    assert_visible(&harness.app);

    harness.app.open_editor_find();
    harness.app.set_editor_find_query("NEEDLE".into());
    for _ in 0..4 {
        harness.app.editor_find_next();
        render_frames(&mut harness.app, &ctx, 950.0);
        assert_visible(&harness.app);
        if harness.app.editor_find.active_match != Some(0) {
            let viewport = &harness.app.virtual_viewport;
            assert!(
                (viewport.caret.unwrap().center().y - viewport.rect.unwrap().center().y).abs()
                    < 2.0,
                "interior Find matches must be centered, not merely visible"
            );
        }
        assert!(ctx.memory(|m| m.has_focus(egui::Id::new(EDITOR_FIND_INPUT_ID))));
        assert!(!harness.app.virtual_editor_state.has_focus);
    }
    let active = harness.app.editor_find.active_match.unwrap();
    for (modifiers, expected) in [
        (egui::Modifiers::NONE, (active + 1) % 3),
        (egui::Modifiers::SHIFT, active),
    ] {
        run_editor_panel_once(
            &mut harness.app,
            &ctx,
            egui::RawInput {
                events: vec![key_event(egui::Key::Enter, modifiers)],
                ..Default::default()
            },
        );
        render_frames(&mut harness.app, &ctx, 950.0);
        assert_eq!(harness.app.editor_find.active_match, Some(expected));
        assert!(ctx.memory(|m| m.has_focus(egui::Id::new(EDITOR_FIND_INPUT_ID))));
        assert_visible(&harness.app);
    }
}

#[test]
fn indentation_preserves_direction_excludes_next_line_and_is_one_undo_step() {
    for (ending, reverse) in ["\n", "\r\n", "\r", "\u{2028}", "\u{85}"]
        .into_iter()
        .flat_map(|ending| [false, true].map(|reverse| (ending, reverse)))
    {
        let mut harness = make_app();
        let ctx = egui::Context::default();
        let text = format!("  α{ending}{ending}\tβ{ending}untouched{ending}");
        harness.app.reset_virtual_editor(&text);
        let end = harness.app.virtual_editor_buffer.line_col_to_char(3, 0);
        let (anchor, cursor) = if reverse { (end, 0) } else { (0, end) };
        harness.app.virtual_editor_state.restore_selection(
            cursor,
            Some(anchor),
            text.chars().count(),
        );
        for (command, expected) in [
            (
                VirtualInputCommand::Unindent,
                format!("α{ending}{ending}β{ending}untouched{ending}"),
            ),
            (
                VirtualInputCommand::InsertTab,
                format!("      α{ending}{ending}    \tβ{ending}untouched{ending}"),
            ),
        ] {
            harness.app.apply_virtual_commands(&ctx, &[command]);
            assert_eq!(harness.app.virtual_editor_buffer.to_string(), expected);
            let after = (
                harness.app.virtual_editor_state.cursor(),
                harness.app.virtual_editor_state.anchor(),
            );
            assert_eq!(after.0 < after.1.unwrap(), reverse);
            harness
                .app
                .apply_virtual_commands(&ctx, &[VirtualInputCommand::Undo]);
            assert_eq!(harness.app.virtual_editor_buffer.to_string(), text);
            assert_eq!(harness.app.virtual_editor_state.cursor(), cursor);
            assert_eq!(harness.app.virtual_editor_state.anchor(), Some(anchor));
            harness
                .app
                .apply_virtual_commands(&ctx, &[VirtualInputCommand::Redo]);
            assert_eq!(harness.app.virtual_editor_buffer.to_string(), expected);
            assert_eq!(
                (
                    harness.app.virtual_editor_state.cursor(),
                    harness.app.virtual_editor_state.anchor()
                ),
                after
            );
            harness
                .app
                .apply_virtual_commands(&ctx, &[VirtualInputCommand::Undo]);
        }
    }
}

#[test]
fn shift_tab_without_selection_removes_only_leading_indentation() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    harness.app.reset_virtual_editor("      α tail");
    let len = harness.app.virtual_editor_buffer.len_chars();
    harness.app.virtual_editor_state.set_cursor(len, len);
    let commands = commands_from_events(&[key_event(egui::Key::Tab, egui::Modifiers::SHIFT)], true);
    harness.app.apply_virtual_commands(&ctx, &commands);
    assert_eq!(harness.app.virtual_editor_buffer.to_string(), "  α tail");
    assert_eq!(harness.app.virtual_editor_state.cursor(), len - 4);
    harness
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::Undo]);
    assert_eq!(
        harness.app.virtual_editor_buffer.to_string(),
        "      α tail"
    );
}

#[test]
fn native_deactivation_preserves_selection_and_releases_editor_ownership() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    harness.app.focus_editor_next = true;
    run_full_update(&mut harness.app, &ctx, Vec::new());
    harness
        .app
        .virtual_editor_state
        .restore_selection(4, Some(1), 7);
    run_full_update(
        &mut harness.app,
        &ctx,
        vec![
            egui::Event::WindowFocused(false),
            egui::Event::WindowFocused(true),
        ],
    );
    assert!(harness.app.virtual_editor_state.has_focus);
    assert!(ctx.memory(|m| m.has_focus(egui::Id::new(VIRTUAL_EDITOR_ID))));
    run_full_update(
        &mut harness.app,
        &ctx,
        vec![
            egui::Event::WindowFocused(true),
            egui::Event::WindowFocused(false),
        ],
    );
    assert!(!harness.app.virtual_editor_state.has_focus);
    assert!(!ctx.memory(|m| m.has_focus(egui::Id::new(VIRTUAL_EDITOR_ID))));
    assert_eq!(
        harness.app.virtual_editor_state.selection_range(),
        Some(1..4)
    );
    let output = run_full_update_with_input(
        &mut harness.app,
        &ctx,
        egui::RawInput {
            events: vec![egui::Event::WindowFocused(true), egui::Event::Copy],
            ..Default::default()
        },
    );
    assert!(output
        .platform_output
        .commands
        .iter()
        .any(|command| matches!(command, egui::OutputCommand::CopyText(text) if text == "ont")));
    assert!(!harness.app.virtual_editor_state.has_focus);
    run_full_update(
        &mut harness.app,
        &ctx,
        vec![
            egui::Event::WindowFocused(true),
            egui::Event::Paste("new paste".into()),
        ],
    );
    assert_eq!(harness.app.virtual_editor_buffer.to_string(), "content");
    assert!(harness
        .cmd_rx
        .try_iter()
        .any(|cmd| matches!(cmd, CoreCmd::CreatePaste { .. })));
}

#[test]
fn global_shortcut_uses_modifiers_at_key_press_even_after_release_in_same_frame() {
    let ctx = egui::Context::default();
    let _ = ctx.run(
        egui::RawInput {
            modifiers: egui::Modifiers::NONE,
            events: vec![command_key_event(egui::Key::K)],
            ..Default::default()
        },
        |ctx| {
            let actions: Vec<_> = ctx.input(|input| pressed_runtime_shortcuts(input).collect());
            assert_eq!(
                actions,
                vec![RuntimeShortcutAction::ToggleCommandPaletteLegacy]
            );
        },
    );
}

#[test]
fn partial_line_indentation_and_caret_reveal_cover_boundary_branches() {
    for (text, start, end, expected) in [
        ("alpha beta", 2, 7, "    alpha beta"),
        ("alpha\nbeta\ngamma", 2, 8, "    alpha\n    beta\ngamma"),
    ] {
        for reverse in [false, true] {
            let mut harness = make_app();
            let ctx = egui::Context::default();
            harness.app.reset_virtual_editor(text);
            let (cursor, anchor) = if reverse { (start, end) } else { (end, start) };
            harness.app.virtual_editor_state.restore_selection(
                cursor,
                Some(anchor),
                text.chars().count(),
            );
            harness
                .app
                .apply_virtual_commands(&ctx, &[VirtualInputCommand::InsertTab]);
            assert_eq!(harness.app.active_snapshot(), expected);
            assert_eq!(
                harness.app.virtual_editor_state.cursor()
                    < harness.app.virtual_editor_state.anchor().unwrap(),
                reverse
            );
            harness
                .app
                .apply_virtual_commands(&ctx, &[VirtualInputCommand::Undo]);
            assert_eq!(harness.app.active_snapshot(), text);
            assert_eq!(
                (
                    harness.app.virtual_editor_state.cursor(),
                    harness.app.virtual_editor_state.anchor()
                ),
                (cursor, Some(anchor))
            );
        }
    }
    for (reveal, row, offset, height, expected) in [
        (CursorReveal::Minimal, 5, 100.0, 100.0, 30.0),
        (CursorReveal::Minimal, 19, 100.0, 100.0, 120.0),
        (CursorReveal::Minimal, 15, 100.0, 100.0, 100.0),
        (CursorReveal::Minimal, 0, 100.0, 5.0, 0.0),
        (CursorReveal::Center, 15, 0.0, 100.0, 105.0),
        (CursorReveal::Center, 0, 100.0, 100.0, 0.0),
    ] {
        assert_eq!(reveal.offset(row, offset, height, 10.0), expected);
    }
}

fn review_frame(
    app: &mut LocalPasteApp,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let modifiers = events
        .iter()
        .rev()
        .find_map(|event| match event {
            egui::Event::PointerButton { modifiers, .. } | egui::Event::Key { modifiers, .. } => {
                Some(*modifiers)
            }
            _ => None,
        })
        .unwrap_or_default();
    run_full_update_with_input(
        app,
        ctx,
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 900.0),
            )),
            events,
            modifiers,
            ..Default::default()
        },
    )
}

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

fn review_click(app: &mut LocalPasteApp, ctx: &egui::Context, pos: egui::Pos2) {
    for pressed in [true, false] {
        review_frame(
            app,
            ctx,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
}

#[test]
fn find_mouse_navigation_retains_query_focus_for_enter_paste_and_escape() {
    let (mut harness, _event_tx) = make_app_with_event_tx();
    let ctx = egui::Context::default();
    harness.app.reset_virtual_editor("needle alpha needle beta");
    harness.app.open_editor_find();
    harness.app.set_editor_find_query("needle".into());
    review_frame(&mut harness.app, &ctx, vec![]);
    for (label, active) in [("Next", 1), ("Prev", 0)] {
        let output = review_frame(&mut harness.app, &ctx, vec![]);
        review_click(
            &mut harness.app,
            &ctx,
            rendered_label_center(&output, label),
        );
        assert_eq!(harness.app.editor_find.active_match, Some(active));
        assert!(ctx.memory(|memory| memory.has_focus(egui::Id::new(EDITOR_FIND_INPUT_ID))));
    }
    review_frame(
        &mut harness.app,
        &ctx,
        vec![key_event(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert_eq!(harness.app.editor_find.active_match, Some(1));
    review_frame(&mut harness.app, &ctx, vec![egui::Event::Paste("X".into())]);
    assert!(harness.app.editor_find.query.contains('X'));
    assert_eq!(harness.app.active_snapshot(), "needle alpha needle beta");
    assert!(!harness
        .cmd_rx
        .try_iter()
        .any(|cmd| matches!(cmd, CoreCmd::CreatePaste { .. })));
    review_frame(
        &mut harness.app,
        &ctx,
        vec![key_event(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert!(!harness.app.editor_find.open);
    assert!(ctx.memory(|memory| memory.has_focus(egui::Id::new(VIRTUAL_EDITOR_ID))));
}

#[test]
fn floating_help_buttons_preserve_the_underlying_caret_and_selection() {
    for button in ["Clear", "Close", "left edge", "right edge"] {
        let (mut harness, _event_tx) = make_app_with_event_tx();
        let ctx = egui::Context::default();
        let text = "editor text below help\n".repeat(80);
        harness.app.reset_virtual_editor(&text);
        harness
            .app
            .virtual_editor_state
            .restore_selection(300, Some(290), text.chars().count());
        harness.app.focus_editor_next = true;
        review_frame(&mut harness.app, &ctx, vec![]);
        harness.app.open_shortcut_help(&ctx);
        harness.app.shortcut_help_query = "undo".into();
        for _ in 0..3 {
            review_frame(&mut harness.app, &ctx, vec![]);
        }
        let output = review_frame(&mut harness.app, &ctx, vec![]);
        let pos = if button.ends_with("edge") {
            let window = ctx
                .memory(|memory| memory.area_rect(egui::Id::new("Keyboard Shortcuts")))
                .unwrap();
            egui::pos2(
                if button == "left edge" {
                    window.left() + 2.0
                } else {
                    window.right() - 2.0
                },
                window.center().y,
            )
        } else {
            rendered_label_center(&output, button)
        };
        review_click(&mut harness.app, &ctx, pos);
        assert_eq!(
            (
                harness.app.virtual_editor_state.cursor(),
                harness.app.virtual_editor_state.anchor()
            ),
            (300, Some(290)),
            "{button}"
        );
        if harness.app.shortcut_help_open {
            harness.app.close_shortcut_help(&ctx);
        }
        review_frame(&mut harness.app, &ctx, vec![egui::Event::Text("W".into())]);
        assert_eq!(harness.app.virtual_editor_buffer.slice_chars(290..291), "W");
    }
}

#[test]
fn shift_click_extends_editor_selection_from_its_existing_anchor() {
    for (cursor, anchor) in [(1, None), (8, Some(2)), (2, Some(8))] {
        for (drag, shift) in [(false, true), (true, true), (true, false)] {
            let (mut harness, _event_tx) = make_app_with_event_tx();
            let ctx = egui::Context::default();
            let text = "alpha beta gamma\nnext line\n";
            harness.app.reset_virtual_editor(text);
            for _ in 0..3 {
                review_frame(&mut harness.app, &ctx, vec![]);
            }
            let pos = harness.app.virtual_viewport.caret.unwrap().center() + egui::vec2(105.0, 0.0);
            let mut expected_anchor = anchor.unwrap_or(cursor);
            harness
                .app
                .virtual_editor_state
                .restore_selection(cursor, anchor, text.len());
            review_frame(
                &mut harness.app,
                &ctx,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: if shift {
                            egui::Modifiers::SHIFT
                        } else {
                            egui::Modifiers::NONE
                        },
                    },
                ],
            );
            if !shift {
                expected_anchor = harness.app.virtual_editor_state.cursor();
            }
            if drag {
                // Shift is released before egui recognizes the drag start.
                review_frame(
                    &mut harness.app,
                    &ctx,
                    vec![egui::Event::PointerMoved(pos + egui::vec2(25.0, 0.0))],
                );
            }
            let end = if drag {
                pos + egui::vec2(25.0, 0.0)
            } else {
                pos
            };
            review_frame(
                &mut harness.app,
                &ctx,
                vec![egui::Event::PointerButton {
                    pos: end,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            assert_eq!(
                harness.app.virtual_editor_state.anchor(),
                Some(expected_anchor),
                "cursor={cursor}, anchor={anchor:?}, drag={drag}"
            );
            assert!(harness.app.virtual_editor_state.cursor() > expected_anchor);
        }
    }
}
