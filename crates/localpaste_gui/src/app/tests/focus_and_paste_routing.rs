//! Focus and paste-routing regression tests for the virtual editor.

use super::*;

#[test]
fn click_outside_editor_viewport_blurs_focus() {
    let mut harness = make_app();
    harness.app.editor_mode = EditorMode::VirtualEditor;
    harness.app.reset_virtual_editor("line one\nline two\n");

    let ctx = egui::Context::default();
    configure_virtual_editor_test_ctx(&ctx);
    let screen_rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1200.0, 900.0));
    let editor_id = egui::Id::new(VIRTUAL_EDITOR_ID);

    let inside_click = egui::pos2(260.0, 720.0);
    run_editor_panel_once(
        &mut harness.app,
        &ctx,
        egui::RawInput {
            screen_rect: Some(screen_rect),
            events: vec![
                egui::Event::PointerMoved(inside_click),
                egui::Event::PointerButton {
                    pos: inside_click,
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

    let outside_click = egui::pos2(20.0, 20.0);
    run_editor_panel_once(
        &mut harness.app,
        &ctx,
        egui::RawInput {
            screen_rect: Some(screen_rect),
            events: vec![
                egui::Event::PointerMoved(outside_click),
                egui::Event::PointerButton {
                    pos: outside_click,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::default(),
                },
            ],
            ..Default::default()
        },
    );
    assert!(!ctx.memory(|m| m.has_focus(editor_id)));
    assert!(!harness.app.virtual_editor_state.has_focus);
}

#[test]
fn virtual_editor_window_blur_clears_focus_state() {
    let mut harness = make_app();
    harness.app.editor_mode = EditorMode::VirtualEditor;
    harness.app.reset_virtual_editor("line one\nline two\n");

    let ctx = egui::Context::default();
    configure_virtual_editor_test_ctx(&ctx);
    let screen_rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1200.0, 900.0));
    let editor_id = egui::Id::new(VIRTUAL_EDITOR_ID);

    harness.app.focus_editor_next = true;
    run_editor_panel_once(
        &mut harness.app,
        &ctx,
        egui::RawInput {
            screen_rect: Some(screen_rect),
            ..Default::default()
        },
    );
    assert!(ctx.memory(|m| m.has_focus(editor_id)));
    assert!(harness.app.virtual_editor_state.has_focus);

    run_editor_panel_once(
        &mut harness.app,
        &ctx,
        egui::RawInput {
            screen_rect: Some(screen_rect),
            focused: false,
            ..Default::default()
        },
    );
    assert!(!ctx.memory(|m| m.has_focus(editor_id)));
    assert!(!harness.app.virtual_editor_state.has_focus);
}

#[test]
fn explicit_paste_as_new_pending_ttl_and_consumption_matrix() {
    let mut harness = make_app();

    harness.app.arm_paste_as_new_intent();
    let mut missing_clipboard = None;
    assert!(!harness
        .app
        .maybe_consume_explicit_paste_as_new(&mut missing_clipboard));
    assert_eq!(
        harness.app.paste_as_new_pending_frames,
        PASTE_AS_NEW_PENDING_TTL_FRAMES.saturating_sub(1)
    );

    let mut clipboard = Some("from clipboard".to_string());
    assert!(harness
        .app
        .maybe_consume_explicit_paste_as_new(&mut clipboard));
    assert!(clipboard.is_none());
    assert_eq!(harness.app.paste_as_new_pending_frames, 0);
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::CreatePaste { content } => assert_eq!(content, "from clipboard"),
        other => panic!("unexpected command: {:?}", other),
    }
}

#[test]
fn explicit_paste_as_new_preserves_tabbed_trailing_line_exactly() {
    let mut harness = make_app();
    harness.app.arm_paste_as_new_intent();
    let payload = "def sample():\n\treturn foobar";
    let mut clipboard = Some(payload.to_string());

    assert!(harness
        .app
        .maybe_consume_explicit_paste_as_new(&mut clipboard));
    assert!(clipboard.is_none());
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::CreatePaste { content } => assert_eq!(content, payload),
        other => panic!("unexpected command: {:?}", other),
    }
}

#[test]
fn explicit_paste_as_new_accepts_whitespace_only_payload() {
    let mut harness = make_app();
    harness.app.arm_paste_as_new_intent();
    let payload = " \t\n  ";
    let mut clipboard = Some(payload.to_string());

    assert!(harness
        .app
        .maybe_consume_explicit_paste_as_new(&mut clipboard));
    assert!(clipboard.is_none());
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::CreatePaste { content } => assert_eq!(content, payload),
        other => panic!("unexpected command: {:?}", other),
    }
}

#[test]
fn clipboard_create_policy_distinguishes_explicit_and_global_shortcuts() {
    let whitespace_only = " \t\n ";

    assert!(LocalPasteApp::should_create_paste_from_clipboard(
        whitespace_only,
        paste_intent::ClipboardCreatePolicy::ExplicitPasteAsNew,
    ));
    assert!(!LocalPasteApp::should_create_paste_from_clipboard(
        whitespace_only,
        paste_intent::ClipboardCreatePolicy::ImplicitGlobalShortcut,
    ));
}

#[test]
fn merge_pasted_text_prefers_fuller_duplicate_payload() {
    let full_payload = "def sample():\n\treturn foobar";
    let short_payload = "def sample():\n\treturn";

    let mut observed = Some(short_payload.to_string());
    LocalPasteApp::merge_pasted_text(&mut observed, full_payload);
    assert_eq!(observed.as_deref(), Some(full_payload));

    let mut observed = Some(full_payload.to_string());
    LocalPasteApp::merge_pasted_text(&mut observed, short_payload);
    assert_eq!(observed.as_deref(), Some(full_payload));
}

#[test]
fn explicit_paste_as_new_waits_for_delayed_clipboard_payload() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    harness.app.request_paste_as_new(&ctx);

    let mut missing_clipboard = None;
    for _ in 0..(PASTE_AS_NEW_PENDING_TTL_FRAMES + 2) {
        assert!(!harness
            .app
            .maybe_consume_explicit_paste_as_new(&mut missing_clipboard));
        assert_eq!(
            harness.app.paste_as_new_pending_frames,
            PASTE_AS_NEW_PENDING_TTL_FRAMES
        );
        assert!(harness.app.paste_as_new_clipboard_requested_at.is_some());
    }

    let mut clipboard = Some("from delayed clipboard".to_string());
    assert!(harness
        .app
        .maybe_consume_explicit_paste_as_new(&mut clipboard));
    assert_eq!(harness.app.paste_as_new_pending_frames, 0);
    assert!(harness.app.paste_as_new_clipboard_requested_at.is_none());
    assert!(clipboard.is_none());
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::CreatePaste { content } => assert_eq!(content, "from delayed clipboard"),
        other => panic!("unexpected command: {:?}", other),
    }
}

#[test]
fn plain_paste_shortcut_clears_stale_explicit_paste_as_new_intent() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    harness.app.request_paste_as_new(&ctx);
    assert_eq!(
        harness.app.paste_as_new_pending_frames,
        PASTE_AS_NEW_PENDING_TTL_FRAMES
    );
    assert!(harness.app.paste_as_new_clipboard_requested_at.is_some());

    harness.app.cancel_paste_as_new_intent();
    assert_eq!(harness.app.paste_as_new_pending_frames, 0);
    assert!(harness.app.paste_as_new_clipboard_requested_at.is_none());

    let mut delayed_clipboard = Some("from stale explicit request".to_string());
    assert!(!harness
        .app
        .maybe_consume_explicit_paste_as_new(&mut delayed_clipboard));
    assert_eq!(
        delayed_clipboard.as_deref(),
        Some("from stale explicit request")
    );
    assert!(harness.cmd_rx.try_recv().is_err());
}

#[test]
fn explicit_paste_as_new_timeout_sets_status_and_clears_intent() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    harness.app.request_paste_as_new(&ctx);
    harness.app.paste_as_new_clipboard_requested_at =
        Some(Instant::now() - PASTE_AS_NEW_CLIPBOARD_WAIT_TIMEOUT - Duration::from_millis(1));

    let mut missing_clipboard = None;
    assert!(!harness
        .app
        .maybe_consume_explicit_paste_as_new(&mut missing_clipboard));
    assert_eq!(harness.app.paste_as_new_pending_frames, 0);
    assert!(harness.app.paste_as_new_clipboard_requested_at.is_none());
    assert_eq!(
        harness
            .app
            .status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some("Paste-as-new clipboard request timed out; try again.")
    );
}

#[test]
fn explicit_paste_as_new_skips_virtual_paste_commands() {
    let mut harness = make_app();
    harness.app.arm_paste_as_new_intent();
    assert!(harness
        .app
        .should_skip_virtual_command_for_paste_as_new(&VirtualInputCommand::Paste(
            "x".to_string()
        )));
    assert!(!harness.app.should_skip_virtual_command_for_paste_as_new(
        &VirtualInputCommand::InsertText("x".to_string())
    ));
}

#[test]
fn paste_as_new_viewport_request_only_when_clipboard_payload_missing() {
    struct Case {
        request_paste_as_new: bool,
        pasted_text: Option<&'static str>,
        expect_viewport_request: bool,
    }

    let cases = [
        Case {
            request_paste_as_new: false,
            pasted_text: None,
            expect_viewport_request: false,
        },
        Case {
            request_paste_as_new: true,
            pasted_text: None,
            expect_viewport_request: true,
        },
        Case {
            request_paste_as_new: true,
            pasted_text: Some("from clipboard"),
            expect_viewport_request: false,
        },
    ];

    for case in cases {
        let harness = make_app();
        let should_request = harness
            .app
            .should_request_viewport_paste_for_new(case.request_paste_as_new, case.pasted_text);
        assert_eq!(
            should_request,
            case.expect_viewport_request,
            "viewport request mismatch for case request={} pasted_text_present={}",
            case.request_paste_as_new,
            case.pasted_text.is_some()
        );
    }
}

#[test]
fn command_shift_v_arms_paste_as_new_before_virtual_routing() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    let modifiers = egui::Modifiers {
        command: true,
        shift: true,
        ..Default::default()
    };

    let _ = ctx.run(
        egui::RawInput {
            events: vec![
                egui::Event::Key {
                    key: egui::Key::V,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers,
                },
                egui::Event::Paste("from clipboard".to_string()),
            ],
            ..Default::default()
        },
        |ctx| {
            assert!(harness.app.maybe_arm_paste_as_new_shortcut_intent(ctx));
            let commands = ctx.input(|input| commands_from_events(&input.events, true));
            assert!(
                commands
                    .iter()
                    .any(|command| matches!(command, VirtualInputCommand::Paste(_))),
                "expected virtual paste command from same-frame paste event"
            );
            for command in &commands {
                if matches!(command, VirtualInputCommand::Paste(_)) {
                    assert!(harness
                        .app
                        .should_skip_virtual_command_for_paste_as_new(command));
                }
            }
        },
    );
    assert_eq!(
        harness.app.paste_as_new_pending_frames,
        PASTE_AS_NEW_PENDING_TTL_FRAMES
    );
}

#[test]
fn plain_paste_shortcut_routes_by_editor_focus_contract() {
    struct Case {
        name: &'static str,
        editor_focus_pre: bool,
        saw_virtual_paste: bool,
        wants_keyboard_input_before: bool,
        expect_virtual_request: bool,
        expect_new_paste_request: bool,
    }

    let cases = [
        Case {
            name: "virtual focused requests viewport paste",
            editor_focus_pre: true,
            saw_virtual_paste: false,
            wants_keyboard_input_before: true,
            expect_virtual_request: true,
            expect_new_paste_request: false,
        },
        Case {
            name: "virtual focused suppresses duplicate viewport paste",
            editor_focus_pre: true,
            saw_virtual_paste: true,
            wants_keyboard_input_before: true,
            expect_virtual_request: false,
            expect_new_paste_request: false,
        },
        Case {
            name: "virtual unfocused with free keyboard creates new paste",
            editor_focus_pre: false,
            saw_virtual_paste: false,
            wants_keyboard_input_before: false,
            expect_virtual_request: false,
            expect_new_paste_request: true,
        },
        Case {
            name: "unfocused with focused non-editor input does not create new paste",
            editor_focus_pre: false,
            saw_virtual_paste: false,
            wants_keyboard_input_before: true,
            expect_virtual_request: false,
            expect_new_paste_request: false,
        },
    ];

    for case in cases {
        let harness = make_app();
        let focus_state = LocalPasteApp::plain_paste_focus_state(
            case.editor_focus_pre,
            case.wants_keyboard_input_before,
        );
        let (request_virtual, request_new) = harness
            .app
            .route_plain_paste_shortcut(focus_state, case.saw_virtual_paste);
        assert_eq!(
            request_virtual, case.expect_virtual_request,
            "virtual routing mismatch for case '{}'",
            case.name
        );
        assert_eq!(
            request_new, case.expect_new_paste_request,
            "new-paste routing mismatch for case '{}'",
            case.name
        );
    }
}

#[test]
fn plain_paste_shortcut_resolution_uses_post_layout_focus_state() {
    let mut harness = make_app();
    harness.app.editor_mode = EditorMode::VirtualEditor;

    // Regression guard: when focus is acquired in the same frame as Ctrl/Cmd+V,
    // plain paste should stay in the editor instead of creating a new paste.
    let (request_virtual, request_new) = harness.app.resolve_plain_paste_shortcut_request(
        true,
        LocalPasteApp::plain_paste_focus_state(true, true),
        false,
    );
    assert!(request_virtual);
    assert!(!request_new);

    let (request_virtual, request_new) = harness.app.resolve_plain_paste_shortcut_request(
        false,
        LocalPasteApp::plain_paste_focus_state(false, false),
        false,
    );
    assert!(!request_virtual);
    assert!(!request_new);
}

#[test]
fn delete_shortcut_guard_blocks_when_text_input_virtual_focus_or_focus_promotion_active() {
    struct Case {
        name: &'static str,
        wants_keyboard_input: bool,
        virtual_editor_focus_active: bool,
        focus_promotion_requested: bool,
        expected: bool,
    }

    let cases = [
        Case {
            name: "text input owns keyboard",
            wants_keyboard_input: true,
            virtual_editor_focus_active: false,
            focus_promotion_requested: false,
            expected: false,
        },
        Case {
            name: "virtual editor focused",
            wants_keyboard_input: false,
            virtual_editor_focus_active: true,
            focus_promotion_requested: false,
            expected: false,
        },
        Case {
            name: "virtual editor focus promotion pending",
            wants_keyboard_input: false,
            virtual_editor_focus_active: false,
            focus_promotion_requested: true,
            expected: false,
        },
        Case {
            name: "non editor context",
            wants_keyboard_input: false,
            virtual_editor_focus_active: false,
            focus_promotion_requested: false,
            expected: true,
        },
    ];

    for case in cases {
        let harness = make_app();
        let focus_state = LocalPasteApp::delete_shortcut_focus_state(
            case.wants_keyboard_input,
            case.virtual_editor_focus_active,
            case.focus_promotion_requested,
        );
        let actual = harness
            .app
            .should_route_delete_selected_shortcut(focus_state);
        assert_eq!(actual, case.expected, "case '{}'", case.name);
    }
}
