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
    for reverse in [false, true] {
        let mut harness = make_app();
        let ctx = egui::Context::default();
        let text = "  α\n\tβ\nuntouched\n";
        harness.app.reset_virtual_editor(text);
        let end = harness.app.virtual_editor_buffer.line_col_to_char(2, 0);
        let (anchor, cursor) = if reverse { (end, 0) } else { (0, end) };
        harness.app.virtual_editor_state.restore_selection(
            cursor,
            Some(anchor),
            text.chars().count(),
        );
        for (command, expected) in [
            (VirtualInputCommand::Unindent, "α\nβ\nuntouched\n"),
            (
                VirtualInputCommand::InsertTab,
                "      α\n    \tβ\nuntouched\n",
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
        vec![egui::Event::WindowFocused(false)],
    );
    assert!(!harness.app.virtual_editor_state.has_focus);
    assert!(!ctx.memory(|m| m.has_focus(egui::Id::new(VIRTUAL_EDITOR_ID))));
    assert_eq!(
        harness.app.virtual_editor_state.selection_range(),
        Some(1..4)
    );
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
