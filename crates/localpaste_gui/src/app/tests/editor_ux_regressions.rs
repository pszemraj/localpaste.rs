//! Rendered geometry, focus ownership, and block-edit regressions.

use super::*;

#[test]
fn review_regression_line_break_deletion_joins_lines_atomically() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    for prefix in ["", "a", "é", "🦀"] {
        for separator in [
            "\n", "\r", "\u{000b}", "\u{000c}", "\u{0085}", "\u{2028}", "\u{2029}", "\r\n",
        ] {
            for suffix in ["", "b"] {
                for backward in [false, true] {
                    let before = format!("{prefix}{separator}{suffix}");
                    let expected = format!("{prefix}{suffix}");
                    harness.app.reset_virtual_editor(&before);
                    let boundary = prefix.chars().count();
                    let cursor = boundary
                        + if backward {
                            separator.chars().count()
                        } else {
                            0
                        };
                    let len = harness.app.virtual_editor_buffer.len_chars();
                    harness.app.virtual_editor_state.set_cursor(cursor, len);
                    let command = if backward {
                        VirtualInputCommand::Backspace { word: false }
                    } else {
                        VirtualInputCommand::DeleteForward { word: false }
                    };
                    assert!(harness.app.apply_virtual_commands(&ctx, &[command]).changed);
                    assert_eq!(
                        harness.app.active_snapshot(),
                        expected,
                        "{before:?}, backward={backward}"
                    );
                    assert_eq!(harness.app.virtual_editor_state.cursor(), boundary);
                    assert_eq!(harness.app.virtual_editor_buffer.line_count(), 1);
                    for (command, text, expected_cursor) in [
                        (VirtualInputCommand::Undo, before.as_str(), cursor),
                        (VirtualInputCommand::Redo, expected.as_str(), boundary),
                    ] {
                        assert!(harness.app.apply_virtual_commands(&ctx, &[command]).changed);
                        assert_eq!(harness.app.active_snapshot(), text);
                        assert_eq!(harness.app.virtual_editor_state.cursor(), expected_cursor);
                    }
                }
            }
        }
    }
}

mod interactions;

#[test]
fn markdown_url_paste_wraps_selection_and_is_one_reversible_edit() {
    for language in ["markdown", "MD"] {
        for reverse in [false, true] {
            for (label, clipboard, replacement) in [
                (
                    "docs",
                    "https://example.test/docs",
                    "[docs](https://example.test/docs)",
                ),
                (
                    "café 🦀",
                    "http://localhost:3055/a",
                    "[café 🦀](http://localhost:3055/a)",
                ),
                (
                    "**docs**",
                    " \r\nHTTPS://EXAMPLE.test/%2f\r\n",
                    "[**docs**](HTTPS://EXAMPLE.test/%2f)",
                ),
                (
                    "[docs] \\ path",
                    "https://example.test/a(b)?x=&copy;",
                    r"[\[docs\] \\ path](https://example.test/a\(b\)?x=\&copy;)",
                ),
                (
                    "line one\nline two",
                    "https://example.test",
                    "[line one&#10;line two](https://example.test)",
                ),
                (
                    "`[docs] \\ path`",
                    "https://example.test",
                    "[`[docs] \\ path`](https://example.test)",
                ),
                (
                    "``a`[b]\\c``",
                    "https://example.test",
                    "[``a`[b]\\c``](https://example.test)",
                ),
                (
                    "`unclosed [",
                    "https://example.test/`",
                    r"[\`unclosed \[](https://example.test/`)",
                ),
                (
                    "<https://other.test>",
                    "https://example.test",
                    r"[\<https://other.test\>](https://example.test)",
                ),
                (
                    "one\n\ntwo",
                    "https://example.test",
                    "[one&#10;&#10;two](https://example.test)",
                ),
                (
                    "one\r\n\r\ntwo\n> quote",
                    "https://example.test",
                    r"[one&#13;&#10;&#13;&#10;two&#10;\> quote](https://example.test)",
                ),
                (
                    "`one\r\ntwo`",
                    "https://example.test",
                    "[`one two`](https://example.test)",
                ),
                (
                    r"\`[docs]\`",
                    "https://example.test",
                    r"[\`\[docs\]\`](https://example.test)",
                ),
                (
                    r"\[docs\] \\ path \&copy;",
                    "https://example.test",
                    r"[\[docs\] \\ path \&copy;](https://example.test)",
                ),
                (
                    r"`docs\`",
                    "https://example.test",
                    r"[`docs\`](https://example.test)",
                ),
            ] {
                let mut harness = make_app();
                let ctx = egui::Context::default();
                let before = format!("prefix {label} suffix");
                harness.app.reset_virtual_editor(&before);
                harness.app.edit_language = Some(language.into());
                let start = "prefix ".chars().count();
                let end = start + label.chars().count();
                let (cursor, anchor) = if reverse { (start, end) } else { (end, start) };
                harness.app.virtual_editor_state.restore_selection(
                    cursor,
                    Some(anchor),
                    before.chars().count(),
                );
                let result = harness
                    .app
                    .apply_virtual_commands(&ctx, &[VirtualInputCommand::Paste(clipboard.into())]);
                let after = format!("prefix {replacement} suffix");
                assert!(result.changed && result.pasted);
                assert_eq!(harness.app.active_snapshot(), after);
                assert_eq!(
                    harness.app.virtual_editor_state.cursor(),
                    start + replacement.chars().count()
                );
                assert!(harness.app.virtual_editor_state.selection_range().is_none());
                harness
                    .app
                    .apply_virtual_commands(&ctx, &[VirtualInputCommand::Undo]);
                assert_eq!(harness.app.active_snapshot(), before);
                assert_eq!(harness.app.virtual_editor_state.cursor(), cursor);
                assert_eq!(harness.app.virtual_editor_state.anchor(), Some(anchor));
                assert_eq!(harness.app.virtual_editor_history.perf_stats().undo_len, 0);
                harness
                    .app
                    .apply_virtual_commands(&ctx, &[VirtualInputCommand::Redo]);
                assert_eq!(harness.app.active_snapshot(), after);
                assert!(harness.app.virtual_editor_state.selection_range().is_none());
            }
        }
    }
}

#[test]
fn markdown_url_paste_leaves_other_paste_cases_literal() {
    for (language, selected, clipboard) in [
        (None, true, "https://example.test"),
        (Some("rust"), true, "https://example.test"),
        (Some("text"), true, "https://example.test"),
        (Some("markdown"), false, "https://example.test"),
        (Some("markdown"), true, "ordinary pasted text"),
        (Some("markdown"), true, "https://one.test\nhttps://two.test"),
        (Some("markdown"), true, "https://"),
        (Some("markdown"), true, "https:example.test"),
        (Some("markdown"), true, "https://[invalid]"),
        (Some("markdown"), true, "https://example.test/a b"),
        (Some("markdown"), true, "mailto:user@example.test"),
    ] {
        let mut harness = make_app();
        let ctx = egui::Context::default();
        harness.app.reset_virtual_editor("prefix selected suffix");
        harness.app.edit_language = language.map(str::to_owned);
        // A pending language change, rather than the last persisted label, controls editing.
        harness.app.selected_paste.as_mut().unwrap().language = Some("markdown".into());
        harness
            .app
            .virtual_editor_state
            .restore_selection(15, selected.then_some(7), 22);
        harness
            .app
            .apply_virtual_commands(&ctx, &[VirtualInputCommand::Paste(clipboard.into())]);
        let prefix = if selected {
            "prefix "
        } else {
            "prefix selected"
        };
        assert_eq!(
            harness.app.active_snapshot(),
            format!("{prefix}{clipboard} suffix")
        );
    }
}

#[test]
fn markdown_url_paste_history_stays_separate_from_surrounding_edits() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    harness.app.reset_virtual_editor("read docs.");
    harness.app.edit_language = Some("markdown".into());
    harness.app.virtual_editor_state.set_cursor(10, 10);
    harness
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::InsertText("!".into())]);
    harness
        .app
        .virtual_editor_state
        .restore_selection(5, Some(9), 11);
    // Two native paste events in one frame must be applied in order: the
    // second URL has no selection and remains literal after the first link.
    harness.app.apply_virtual_commands(
        &ctx,
        &[
            VirtualInputCommand::Paste("https://one.test".into()),
            VirtualInputCommand::Paste("https://two.test".into()),
            VirtualInputCommand::InsertText("?".into()),
        ],
    );
    let linked = "read [docs](https://one.test)";
    assert_eq!(
        harness.app.active_snapshot(),
        format!("{linked}https://two.test?.!")
    );
    for expected in [
        format!("{linked}https://two.test.!"),
        format!("{linked}.!"),
        "read docs.!".into(),
        "read docs.".into(),
    ] {
        harness
            .app
            .apply_virtual_commands(&ctx, &[VirtualInputCommand::Undo]);
        assert_eq!(harness.app.active_snapshot(), expected);
        if expected == "read docs.!" {
            assert_eq!(harness.app.virtual_editor_state.cursor(), 5);
            assert_eq!(harness.app.virtual_editor_state.anchor(), Some(9));
        }
    }
    for expected in [
        "read docs.!".into(),
        format!("{linked}.!"),
        format!("{linked}https://two.test.!"),
        format!("{linked}https://two.test?.!"),
    ] {
        harness
            .app
            .apply_virtual_commands(&ctx, &[VirtualInputCommand::Redo]);
        assert_eq!(harness.app.active_snapshot(), expected);
    }
    harness
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::Undo]);
    harness
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::InsertText("changed".into())]);
    let revised = harness.app.active_snapshot();
    harness
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::Redo]);
    assert_eq!(harness.app.active_snapshot(), revised);
}

#[test]
fn native_markdown_url_paste_uses_editor_ownership_and_reveals_the_link() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    harness.app.reset_virtual_editor("read docs here");
    harness.app.edit_language = Some("markdown".into());
    harness.app.focus_editor_next = true;
    run_full_update(&mut harness.app, &ctx, vec![]);
    harness
        .app
        .virtual_editor_state
        .restore_selection(9, Some(5), 14);
    run_full_update_with_input(
        &mut harness.app,
        &ctx,
        egui::RawInput {
            modifiers: primary_command_modifiers(),
            events: vec![egui::Event::Paste("https://example.test/docs".into())],
            ..Default::default()
        },
    );
    render_editor_frames(&mut harness.app, &ctx, 1000.0);
    assert_eq!(
        harness.app.active_snapshot(),
        "read [docs](https://example.test/docs) here"
    );
    assert_caret_visible(&harness.app);
    assert!(harness.app.virtual_editor_state.has_focus);
    assert_ne!(
        harness.app.active_snapshot(),
        harness.app.selected_paste.as_ref().unwrap().content
    );
    assert!(!harness
        .cmd_rx
        .try_iter()
        .any(|cmd| matches!(cmd, CoreCmd::CreatePaste { .. })));
    let before = harness.app.active_snapshot();
    harness
        .app
        .virtual_editor_state
        .restore_selection(10, Some(6), before.chars().count());
    harness.app.request_paste_as_new(&ctx);
    run_full_update(
        &mut harness.app,
        &ctx,
        vec![egui::Event::Paste("https://new.test".into())],
    );
    assert_eq!(harness.app.active_snapshot(), before);
    assert!(harness.cmd_rx.try_iter().any(
        |cmd| matches!(cmd, CoreCmd::CreatePaste { content } if content == "https://new.test")
    ));
}

#[test]
fn missing_named_editor_style_renders_with_resolved_fallback_font() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    assert!(!ctx
        .style()
        .text_styles
        .contains_key(&TextStyle::Name(EDITOR_TEXT_STYLE.into())));
    render_editor_frames(&mut harness.app, &ctx, 1000.0);
    assert_caret_visible(&harness.app);
    // A native theme replacement can discard the named style after normal startup too.
    harness.app.ensure_style(&ctx);
    ctx.set_style(egui::Style::default());
    render_editor_frames(&mut harness.app, &ctx, 1000.0);
    assert_caret_visible(&harness.app);
}

#[test]
fn loading_another_paste_resets_the_previous_scroll_position_without_focus() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    harness.app.reset_virtual_editor(&"long line\n".repeat(900));
    harness
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::MoveDocEnd { select: false }]);
    render_editor_frames(&mut harness.app, &ctx, 1000.0);
    assert!(harness.app.virtual_viewport.offset_y > 1000.0);

    harness
        .app
        .reset_virtual_editor(&"short paste\n".repeat(18));
    render_editor_frames(&mut harness.app, &ctx, 1000.0);
    assert!(!harness.app.virtual_editor_state.has_focus);
    assert_eq!(harness.app.virtual_editor_state.cursor(), 0);
    assert_eq!(harness.app.virtual_viewport.offset_y, 0.0);
    assert_caret_visible(&harness.app);
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
        render_editor_frames(&mut harness.app, &ctx, width);
        for command in [
            VirtualInputCommand::MoveDocEnd { select: false },
            VirtualInputCommand::MoveDocHome { select: false },
            VirtualInputCommand::MoveDocEnd { select: false },
        ] {
            harness.app.apply_virtual_commands(&ctx, &[command]);
            render_editor_frames(&mut harness.app, &ctx, width);
            assert_caret_visible(&harness.app);
        }
        for command in [
            VirtualInputCommand::InsertText("EOF λ".into()),
            VirtualInputCommand::Paste("\nnew tail\n".repeat(50)),
        ] {
            harness.app.apply_virtual_commands(&ctx, &[command]);
            render_editor_frames(&mut harness.app, &ctx, width);
            assert_caret_visible(&harness.app);
        }
    }
    // A deliberate scroll away from the caret stays put until another action.
    harness.app.virtual_pending_scroll_offset_y = Some(500.0);
    render_editor_frames(&mut harness.app, &ctx, 950.0);
    assert!((harness.app.virtual_viewport.offset_y - 500.0).abs() < 1.0);
    assert!(!harness.app.virtual_viewport.caret_visible());
    harness
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::MoveDocEnd { select: false }]);
    render_editor_frames(&mut harness.app, &ctx, 950.0);
    assert_caret_visible(&harness.app);

    harness.app.open_editor_find();
    harness.app.set_editor_find_query("NEEDLE".into());
    for _ in 0..4 {
        harness.app.editor_find_next();
        render_editor_frames(&mut harness.app, &ctx, 950.0);
        assert_caret_visible(&harness.app);
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
        render_editor_frames(&mut harness.app, &ctx, 950.0);
        assert_eq!(harness.app.editor_find.active_match, Some(expected));
        assert!(ctx.memory(|m| m.has_focus(egui::Id::new(EDITOR_FIND_INPUT_ID))));
        assert_caret_visible(&harness.app);
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
