//! Rendered discovery transition and same-frame input ownership regressions.

use super::virtual_editor_focus_support::screen_rect;
use super::*;

/// Runs one rendered full app frame with a fixed viewport.
fn frame(app: &mut LocalPasteApp, ctx: &egui::Context, events: Vec<egui::Event>) {
    let remaining_frames = app.deferred_discovery_events.len() + events.len() + 1;
    one_frame(app, ctx, events);
    for _ in 0..remaining_frames {
        if app.deferred_discovery_events.is_empty() {
            return;
        }
        one_frame(app, ctx, vec![]);
    }
    assert!(
        app.deferred_discovery_events.is_empty(),
        "input slices must make progress"
    );
}

/// Runs exactly one rendered frame without draining scheduled input slices.
fn one_frame(app: &mut LocalPasteApp, ctx: &egui::Context, events: Vec<egui::Event>) {
    let _ = run_full_update_with_input(
        app,
        ctx,
        egui::RawInput {
            screen_rect: Some(screen_rect()),
            events,
            ..Default::default()
        },
    );
}

/// Returns the command chord for opening one discovery surface.
fn discovery_chord(shift: bool) -> egui::Event {
    let mut modifiers = primary_command_modifiers();
    modifiers.shift = shift;
    key_event(egui::Key::K, modifiers)
}

/// Verifies title protection and the query destination for an opening chord.
fn opening_then_typing_preserves_title(shift: bool) {
    let (mut harness, _events) = make_app_with_event_tx();
    let ctx = egui::Context::default();
    frame(&mut harness.app, &ctx, vec![]);
    ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(TITLE_INPUT_ID)));
    frame(&mut harness.app, &ctx, vec![]);
    frame(
        &mut harness.app,
        &ctx,
        vec![discovery_chord(shift), egui::Event::Text("copy".into())],
    );
    assert_eq!(
        harness.app.edit_name, "Alpha",
        "shift={shift}; palette={}, picker={}",
        harness.app.command_palette_query, harness.app.paste_picker_query
    );
    let query = if shift {
        &harness.app.paste_picker_query
    } else {
        &harness.app.command_palette_query
    };
    assert_eq!(query, "copy");
}

#[test]
fn opening_palette_then_typing_same_frame_preserves_title() {
    opening_then_typing_preserves_title(false);
}

#[test]
fn opening_picker_then_typing_same_frame_preserves_title() {
    opening_then_typing_preserves_title(true);
}

/// Verifies that a post-Escape paste reaches the opener's editor.
fn canceling_then_pasting_inserts_into_editor(shift: bool) {
    let (mut harness, _events) = make_app_with_event_tx();
    let ctx = egui::Context::default();
    harness.app.focus_editor_next = true;
    frame(&mut harness.app, &ctx, vec![]);
    frame(&mut harness.app, &ctx, vec![discovery_chord(shift)]);
    frame(&mut harness.app, &ctx, vec![]);
    let end = harness.app.virtual_editor_buffer.len_chars();
    harness.app.virtual_editor_state.set_cursor(end, end);
    frame(
        &mut harness.app,
        &ctx,
        vec![
            key_event(egui::Key::Escape, egui::Modifiers::NONE),
            egui::Event::Paste("inserted".into()),
        ],
    );
    assert_eq!(
        harness.app.active_snapshot(),
        "contentinserted",
        "shift={shift}; palette={}, picker={}",
        harness.app.command_palette_query,
        harness.app.paste_picker_query
    );
}

#[test]
fn canceling_palette_then_pasting_same_frame_inserts_into_editor() {
    canceling_then_pasting_inserts_into_editor(false);
}

#[test]
fn canceling_picker_then_pasting_same_frame_inserts_into_editor() {
    canceling_then_pasting_inserts_into_editor(true);
}

#[test]
fn opening_discovery_then_new_chord_same_frame_blocks_background_creation() {
    let (mut harness, _events) = make_app_with_event_tx();
    let ctx = egui::Context::default();
    frame(&mut harness.app, &ctx, vec![]);
    while harness.cmd_rx.try_recv().is_ok() {}
    frame(
        &mut harness.app,
        &ctx,
        vec![discovery_chord(false), command_key_event(egui::Key::N)],
    );
    let cmds: Vec<_> = harness.cmd_rx.try_iter().collect();
    assert!(
        !cmds
            .iter()
            .any(|cmd| matches!(cmd, CoreCmd::CreatePaste { .. })),
        "background commands: {cmds:?}"
    );
}

#[test]
fn opening_keeps_input_before_and_after_the_chord_in_their_original_fields() {
    for shift in [false, true] {
        let (mut harness, _events) = make_app_with_event_tx();
        let ctx = egui::Context::default();
        frame(&mut harness.app, &ctx, vec![]);
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(TITLE_INPUT_ID)));
        frame(&mut harness.app, &ctx, vec![]);
        frame(
            &mut harness.app,
            &ctx,
            vec![
                egui::Event::Text("before".into()),
                discovery_chord(shift),
                egui::Event::Paste("after".into()),
            ],
        );
        assert_eq!(harness.app.edit_name, "Alphabefore");
        let query = if shift {
            &harness.app.paste_picker_query
        } else {
            &harness.app.command_palette_query
        };
        assert_eq!(query, "after");
    }
}

#[test]
fn dismissal_keeps_earlier_paste_in_query_and_later_paste_in_editor() {
    for shift in [false, true] {
        for escape in [false, true] {
            let (mut harness, _events) = make_app_with_event_tx();
            let ctx = egui::Context::default();
            harness.app.focus_editor_next = true;
            frame(&mut harness.app, &ctx, vec![]);
            let end = harness.app.virtual_editor_buffer.len_chars();
            harness.app.virtual_editor_state.set_cursor(end, end);
            frame(&mut harness.app, &ctx, vec![discovery_chord(shift)]);
            let dismiss = if escape {
                key_event(egui::Key::Escape, egui::Modifiers::NONE)
            } else {
                discovery_chord(shift)
            };
            frame(
                &mut harness.app,
                &ctx,
                vec![
                    egui::Event::Paste("before".into()),
                    dismiss,
                    egui::Event::Paste("after".into()),
                ],
            );
            assert_eq!(harness.app.active_snapshot(), "contentafter");
            let query = if shift {
                &harness.app.paste_picker_query
            } else {
                &harness.app.command_palette_query
            };
            assert_eq!(query, "before", "shift={shift}, escape={escape}");
            assert!(!harness.app.keyboard_overlay_open());
        }
    }
}

#[test]
fn switching_discovery_preserves_each_query_and_original_return_focus() {
    let (mut harness, _events) = make_app_with_event_tx();
    let ctx = egui::Context::default();
    harness.app.focus_editor_next = true;
    frame(&mut harness.app, &ctx, vec![]);
    let end = harness.app.virtual_editor_buffer.len_chars();
    harness.app.virtual_editor_state.set_cursor(end, end);
    frame(
        &mut harness.app,
        &ctx,
        vec![
            discovery_chord(false),
            egui::Event::Text("palette".into()),
            discovery_chord(true),
            egui::Event::Text("picker".into()),
            key_event(egui::Key::F1, egui::Modifiers::NONE),
            egui::Event::Text("help".into()),
            key_event(egui::Key::Escape, egui::Modifiers::NONE),
            egui::Event::Text("editor".into()),
        ],
    );
    assert_eq!(harness.app.command_palette_query, "palette");
    assert_eq!(harness.app.paste_picker_query, "picker");
    assert_eq!(harness.app.shortcut_help_query, "help");
    assert_eq!(harness.app.active_snapshot(), "contenteditor");
    assert!(ctx.memory(|memory| memory.has_focus(egui::Id::new(VIRTUAL_EDITOR_ID))));
}

#[test]
fn deferred_input_precedes_new_native_events() {
    let (mut harness, _events) = make_app_with_event_tx();
    let ctx = egui::Context::default();
    frame(&mut harness.app, &ctx, vec![]);
    one_frame(
        &mut harness.app,
        &ctx,
        vec![discovery_chord(true), egui::Event::Text("older".into())],
    );
    assert!(!harness.app.deferred_discovery_events.is_empty());
    frame(
        &mut harness.app,
        &ctx,
        vec![egui::Event::Text("newer".into())],
    );
    assert_eq!(harness.app.paste_picker_query, "oldernewer");
}

#[test]
fn multipass_render_does_not_replay_discovery_chords_or_text() {
    let (mut harness, _events) = make_app_with_event_tx();
    let ctx = egui::Context::default();
    frame(&mut harness.app, &ctx, vec![]);
    let mut passes = 0;
    let mut app_frame = eframe::Frame::_new_kittest();
    let mut input = egui::RawInput {
        screen_rect: Some(screen_rect()),
        events: vec![discovery_chord(true), egui::Event::Text("once".into())],
        ..Default::default()
    };
    harness.app.raw_input_hook(&ctx, &mut input);
    let _ = ctx.run(input, |ctx| {
        harness.app.update(ctx, &mut app_frame);
        passes += 1;
        if ctx.current_pass_index() == 0 {
            ctx.request_discard("test repeated native batch");
        }
    });
    assert!(passes >= 2);
    assert!(harness.app.paste_picker_open);
    frame(&mut harness.app, &ctx, vec![]);
    assert_eq!(harness.app.paste_picker_query, "once");
    assert!(harness.app.deferred_discovery_events.is_empty());
}

#[test]
fn title_click_before_discovery_chord_establishes_the_return_focus() {
    for shift in [false, true] {
        let (mut harness, _events) = make_app_with_event_tx();
        let ctx = egui::Context::default();
        frame(&mut harness.app, &ctx, vec![]);
        frame(&mut harness.app, &ctx, vec![]);
        let title_id = egui::Id::new(TITLE_INPUT_ID);
        let pos = ctx
            .read_response(title_id)
            .expect("rendered title input")
            .rect
            .center();
        let mut events = super::virtual_editor_focus_support::primary_click_events(pos);
        events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });
        events.extend([discovery_chord(shift), egui::Event::Paste("needle".into())]);
        frame(&mut harness.app, &ctx, events);
        assert_eq!(harness.app.edit_name, "Alpha");
        let query = if shift {
            &harness.app.paste_picker_query
        } else {
            &harness.app.command_palette_query
        };
        assert_eq!(query, "needle");
        frame(
            &mut harness.app,
            &ctx,
            vec![key_event(egui::Key::Escape, egui::Modifiers::NONE)],
        );
        assert!(ctx.memory(|memory| memory.has_focus(title_id)));
    }
}

#[test]
fn new_chord_before_discovery_open_keeps_its_authorized_action() {
    let (mut harness, _events) = make_app_with_event_tx();
    let ctx = egui::Context::default();
    frame(&mut harness.app, &ctx, vec![]);
    while harness.cmd_rx.try_recv().is_ok() {}
    frame(
        &mut harness.app,
        &ctx,
        vec![command_key_event(egui::Key::N), discovery_chord(false)],
    );
    let creates = harness
        .cmd_rx
        .try_iter()
        .filter(|cmd| matches!(cmd, CoreCmd::CreatePaste { .. }))
        .count();
    assert_eq!(creates, 1);
    assert!(harness.app.command_palette_open);
}

#[test]
fn queued_dismissal_preserves_paste_before_native_deactivation_and_finishes_blurred() {
    let (mut harness, _events) = make_app_with_event_tx();
    let ctx = egui::Context::default();
    harness.app.focus_editor_next = true;
    frame(&mut harness.app, &ctx, vec![]);
    let end = harness.app.virtual_editor_buffer.len_chars();
    harness.app.virtual_editor_state.set_cursor(end, end);
    frame(&mut harness.app, &ctx, vec![discovery_chord(false)]);
    while harness.cmd_rx.try_recv().is_ok() {}
    let _ = run_full_update_with_input(
        &mut harness.app,
        &ctx,
        egui::RawInput {
            screen_rect: Some(screen_rect()),
            focused: false,
            events: vec![
                egui::Event::Paste("query prefix".into()),
                key_event(egui::Key::Escape, egui::Modifiers::NONE),
                egui::Event::Paste("after dismissal".into()),
                egui::Event::WindowFocused(false),
            ],
            ..Default::default()
        },
    );
    assert!(!harness.app.virtual_editor_state.has_focus);
    for _ in 0..8 {
        if harness.app.deferred_discovery_events.is_empty() {
            break;
        }
        let _ = run_full_update_with_input(
            &mut harness.app,
            &ctx,
            egui::RawInput {
                screen_rect: Some(screen_rect()),
                focused: false,
                ..Default::default()
            },
        );
    }
    assert!(harness.app.deferred_discovery_events.is_empty());
    assert_eq!(harness.app.command_palette_query, "query prefix");
    assert_eq!(harness.app.active_snapshot(), "contentafter dismissal");
    let cmds: Vec<_> = harness.cmd_rx.try_iter().collect();
    assert!(
        !cmds
            .iter()
            .any(|cmd| matches!(cmd, CoreCmd::CreatePaste { .. })),
        "deferred editor paste must retain its action: {cmds:?}"
    );
    assert!(!harness.app.virtual_editor_state.has_focus);
    frame(
        &mut harness.app,
        &ctx,
        vec![egui::Event::WindowFocused(true)],
    );
    assert!(!harness.app.virtual_editor_state.has_focus);
    assert!(!ctx.memory(|memory| memory.has_focus(egui::Id::new(VIRTUAL_EDITOR_ID))));
}
