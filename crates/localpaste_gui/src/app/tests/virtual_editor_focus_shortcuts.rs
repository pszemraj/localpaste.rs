//! Virtual editor focus shortcut ownership tests.

use super::virtual_editor_focus_support::*;
use super::*;
use crate::app::virtual_editor::PlatformFlavor;

#[test]
fn ctrl_home_after_clicking_into_editor_stays_in_editor() {
    let mut harness = make_app();
    harness
        .app
        .reset_virtual_editor("alpha\nbeta gamma\ndelta\n");

    let ctx = egui::Context::default();
    configure_virtual_editor_test_ctx(&ctx);
    let selected_before = harness.app.selected_id.clone();

    let _ = run_full_update_with_input(
        &mut harness.app,
        &ctx,
        egui::RawInput {
            screen_rect: Some(screen_rect()),
            ..Default::default()
        },
    );
    let _ = run_full_update_with_input(
        &mut harness.app,
        &ctx,
        egui::RawInput {
            screen_rect: Some(screen_rect()),
            events: primary_click_events(editor_body_click_pos()),
            ..Default::default()
        },
    );
    assert_editor_focus(&ctx);

    set_virtual_cursor_at(&mut harness.app, 1, 5);
    let modifiers = primary_command_modifiers();
    let _ = run_full_update_with_input(
        &mut harness.app,
        &ctx,
        egui::RawInput {
            screen_rect: Some(screen_rect()),
            events: vec![key_event(egui::Key::Home, modifiers)],
            modifiers,
            ..Default::default()
        },
    );

    assert_editor_focus(&ctx);
    assert_no_chrome_focus(&ctx, "ctrl-home after click");
    assert_eq!(harness.app.selected_id, selected_before);
    assert_eq!(harness.app.virtual_editor_state.cursor(), 0);
    assert!(harness.app.virtual_editor_state.selection_range().is_none());
}

#[test]
fn ctrl_home_after_search_focus_and_editor_click_stays_in_editor() {
    let mut harness = make_app();
    harness
        .app
        .reset_virtual_editor("alpha\nbeta gamma\ndelta\n");

    let ctx = egui::Context::default();
    configure_virtual_editor_test_ctx(&ctx);
    let selected_before = harness.app.selected_id.clone();

    harness.app.search_focus_requested = true;
    let _ = run_full_update_with_input(
        &mut harness.app,
        &ctx,
        egui::RawInput {
            screen_rect: Some(screen_rect()),
            ..Default::default()
        },
    );
    assert!(ctx.memory(|m| m.has_focus(egui::Id::new(SEARCH_INPUT_ID))));

    let _ = run_full_update_with_input(
        &mut harness.app,
        &ctx,
        egui::RawInput {
            screen_rect: Some(screen_rect()),
            events: primary_click_events(editor_body_click_pos()),
            ..Default::default()
        },
    );
    assert_editor_focus(&ctx);

    set_virtual_cursor_at(&mut harness.app, 1, 5);
    let modifiers = primary_command_modifiers();
    let _ = run_full_update_with_input(
        &mut harness.app,
        &ctx,
        egui::RawInput {
            screen_rect: Some(screen_rect()),
            events: vec![key_event(egui::Key::Home, modifiers)],
            modifiers,
            ..Default::default()
        },
    );

    assert_editor_focus(&ctx);
    assert_no_chrome_focus(&ctx, "ctrl-home after search refocus");
    assert_eq!(harness.app.selected_id, selected_before);
    assert_eq!(harness.app.virtual_editor_state.cursor(), 0);
    assert!(harness.app.virtual_editor_state.selection_range().is_none());
}

#[test]
fn ctrl_home_after_title_focus_and_editor_click_stays_in_editor() {
    let mut harness = make_app();
    harness
        .app
        .reset_virtual_editor("alpha\nbeta gamma\ndelta\n");

    let ctx = egui::Context::default();
    configure_virtual_editor_test_ctx(&ctx);
    let selected_before = harness.app.selected_id.clone();

    let _ = run_full_update_with_input(
        &mut harness.app,
        &ctx,
        egui::RawInput {
            screen_rect: Some(screen_rect()),
            ..Default::default()
        },
    );
    ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(TITLE_INPUT_ID)));
    let _ = run_full_update_with_input(
        &mut harness.app,
        &ctx,
        egui::RawInput {
            screen_rect: Some(screen_rect()),
            ..Default::default()
        },
    );
    assert!(ctx.memory(|m| m.has_focus(egui::Id::new(TITLE_INPUT_ID))));

    let _ = run_full_update_with_input(
        &mut harness.app,
        &ctx,
        egui::RawInput {
            screen_rect: Some(screen_rect()),
            events: primary_click_events(editor_body_click_pos()),
            ..Default::default()
        },
    );
    assert_editor_focus(&ctx);

    set_virtual_cursor_at(&mut harness.app, 1, 5);
    let modifiers = primary_command_modifiers();
    let _ = run_full_update_with_input(
        &mut harness.app,
        &ctx,
        egui::RawInput {
            screen_rect: Some(screen_rect()),
            events: vec![key_event(egui::Key::Home, modifiers)],
            modifiers,
            ..Default::default()
        },
    );

    assert_editor_focus(&ctx);
    assert_no_chrome_focus(&ctx, "ctrl-home after title refocus");
    assert_eq!(harness.app.selected_id, selected_before);
    assert_eq!(harness.app.virtual_editor_state.cursor(), 0);
    assert!(harness.app.virtual_editor_state.selection_range().is_none());
}

#[test]
fn ctrl_home_after_command_palette_close_and_editor_click_stays_in_editor() {
    let mut harness = make_app();
    harness
        .app
        .reset_virtual_editor("alpha\nbeta gamma\ndelta\n");

    let ctx = egui::Context::default();
    configure_virtual_editor_test_ctx(&ctx);
    let selected_before = harness.app.selected_id.clone();

    harness.app.command_palette_open = true;
    let _ = run_full_update_with_input(
        &mut harness.app,
        &ctx,
        egui::RawInput {
            screen_rect: Some(screen_rect()),
            ..Default::default()
        },
    );
    assert!(ctx.memory(|m| m.has_focus(egui::Id::new(COMMAND_PALETTE_INPUT_ID))));

    let _ = run_full_update_with_input(
        &mut harness.app,
        &ctx,
        egui::RawInput {
            screen_rect: Some(screen_rect()),
            events: vec![key_event(egui::Key::Escape, egui::Modifiers::default())],
            ..Default::default()
        },
    );
    assert!(!harness.app.command_palette_open);

    let _ = run_full_update_with_input(
        &mut harness.app,
        &ctx,
        egui::RawInput {
            screen_rect: Some(screen_rect()),
            events: primary_click_events(editor_body_click_pos()),
            ..Default::default()
        },
    );
    assert_editor_focus(&ctx);

    set_virtual_cursor_at(&mut harness.app, 1, 5);
    let modifiers = primary_command_modifiers();
    let _ = run_full_update_with_input(
        &mut harness.app,
        &ctx,
        egui::RawInput {
            screen_rect: Some(screen_rect()),
            events: vec![key_event(egui::Key::Home, modifiers)],
            modifiers,
            ..Default::default()
        },
    );

    assert_editor_focus(&ctx);
    assert_no_chrome_focus(&ctx, "ctrl-home after palette close");
    assert_eq!(harness.app.selected_id, selected_before);
    assert_eq!(harness.app.virtual_editor_state.cursor(), 0);
    assert!(harness.app.virtual_editor_state.selection_range().is_none());
}

#[test]
fn focused_editor_keeps_boundary_and_page_navigation_inside_chrome() {
    for platform in [PlatformFlavor::Other, PlatformFlavor::Mac] {
        with_platform(platform, || {
            let mut harness = make_app();
            harness
                .app
                .reset_virtual_editor("alpha beta\ngamma delta\nepsilon zeta\n");
            let ctx = egui::Context::default();
            configure_virtual_editor_test_ctx(&ctx);
            let selected_before = harness.app.selected_id.clone();
            ctx.memory_mut(|memory| {
                memory.request_focus(egui::Id::new(VIRTUAL_EDITOR_ID));
            });
            let _ = run_full_update_with_input(
                &mut harness.app,
                &ctx,
                egui::RawInput {
                    screen_rect: Some(screen_rect()),
                    ..Default::default()
                },
            );
            assert_editor_focus(&ctx);

            let plain = egui::Modifiers::default();
            let shift = egui::Modifiers {
                shift: true,
                ..Default::default()
            };
            let doc = platform_doc_modifiers(platform);
            let doc_shift = egui::Modifiers { shift: true, ..doc };
            let cases = [
                ("home", egui::Key::Home, plain, false),
                ("end", egui::Key::End, plain, false),
                ("shift-home", egui::Key::Home, shift, true),
                ("shift-end", egui::Key::End, shift, true),
                ("doc-home", egui::Key::Home, doc, false),
                ("doc-end", egui::Key::End, doc, false),
                ("doc-shift-home", egui::Key::Home, doc_shift, true),
                ("doc-shift-end", egui::Key::End, doc_shift, true),
                ("page-up", egui::Key::PageUp, plain, false),
                ("page-down", egui::Key::PageDown, plain, false),
                ("shift-page-up", egui::Key::PageUp, shift, true),
                ("shift-page-down", egui::Key::PageDown, shift, true),
            ];

            for (name, key, modifiers, should_select) in cases {
                set_virtual_cursor_at(&mut harness.app, 1, 5);
                harness.app.virtual_editor_state.clear_preferred_column();
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
                assert_editor_focus(&ctx);
                assert_no_chrome_focus(&ctx, name);
                assert_eq!(harness.app.selected_id, selected_before, "{name}");
                assert_eq!(
                    harness.app.virtual_editor_state.selection_range().is_some(),
                    should_select,
                    "{name} selection"
                );
            }
        });
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
