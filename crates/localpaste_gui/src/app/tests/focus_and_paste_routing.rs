//! Focus and paste-routing regression tests for the virtual editor.

use super::*;

fn configure_virtual_editor_test_ctx(ctx: &egui::Context) {
    ctx.set_fonts(egui::FontDefinitions::empty());
    let mut style = (*ctx.style()).clone();
    style.text_styles.insert(
        egui::TextStyle::Name(EDITOR_TEXT_STYLE.into()),
        egui::FontId::new(14.0, egui::FontFamily::Monospace),
    );
    ctx.set_style(style);
}

fn run_editor_panel_once(app: &mut LocalPasteApp, ctx: &egui::Context, input: egui::RawInput) {
    let _ = ctx.run(input, |ctx| {
        app.render_editor_panel(ctx);
    });
}

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
fn plain_paste_shortcut_routes_by_editor_focus_contract() {
    struct Case {
        mode: EditorMode,
        editor_focus_pre: bool,
        saw_virtual_paste: bool,
        expect_virtual_request: bool,
        expect_new_paste_request: bool,
    }

    let cases = [
        Case {
            mode: EditorMode::VirtualEditor,
            editor_focus_pre: true,
            saw_virtual_paste: false,
            expect_virtual_request: true,
            expect_new_paste_request: false,
        },
        Case {
            mode: EditorMode::VirtualEditor,
            editor_focus_pre: true,
            saw_virtual_paste: true,
            expect_virtual_request: false,
            expect_new_paste_request: false,
        },
        Case {
            mode: EditorMode::VirtualEditor,
            editor_focus_pre: false,
            saw_virtual_paste: false,
            expect_virtual_request: false,
            expect_new_paste_request: true,
        },
        Case {
            mode: EditorMode::TextEdit,
            editor_focus_pre: true,
            saw_virtual_paste: false,
            expect_virtual_request: false,
            expect_new_paste_request: false,
        },
        Case {
            mode: EditorMode::TextEdit,
            editor_focus_pre: false,
            saw_virtual_paste: false,
            expect_virtual_request: false,
            expect_new_paste_request: true,
        },
        Case {
            mode: EditorMode::VirtualPreview,
            editor_focus_pre: false,
            saw_virtual_paste: false,
            expect_virtual_request: false,
            expect_new_paste_request: true,
        },
    ];

    for case in cases {
        let mut harness = make_app();
        harness.app.editor_mode = case.mode;
        let (request_virtual, request_new) = harness
            .app
            .route_plain_paste_shortcut(case.editor_focus_pre, case.saw_virtual_paste);
        assert_eq!(request_virtual, case.expect_virtual_request);
        assert_eq!(request_new, case.expect_new_paste_request);
    }
}
