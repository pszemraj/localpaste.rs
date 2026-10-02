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

#[test]
fn pre_boundary_editor_text_survives_native_deactivation() {
    let (mut harness, _events) = make_app_with_event_tx();
    let ctx = egui::Context::default();
    harness.app.focus_editor_next = true;
    frame(&mut harness.app, &ctx, vec![]);
    let end = harness.app.virtual_editor_buffer.len_chars();
    harness.app.virtual_editor_state.set_cursor(end, end);
    let _ = run_full_update_with_input(
        &mut harness.app,
        &ctx,
        egui::RawInput {
            screen_rect: Some(screen_rect()),
            focused: false,
            events: vec![
                egui::Event::Text("before".into()),
                discovery_chord(false),
                egui::Event::WindowFocused(false),
            ],
            ..Default::default()
        },
    );
    assert_eq!(harness.app.active_snapshot(), "contentbefore");
    frame(&mut harness.app, &ctx, vec![]);
    assert_eq!(harness.app.active_snapshot(), "contentbefore");
    assert!(harness.app.deferred_discovery_events.is_empty());
    assert!(!harness.app.virtual_editor_state.has_focus);
}

#[test]
fn ime_disabled_during_discovery_does_not_poison_editor_typing() {
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
            egui::Event::Ime(egui::ImeEvent::Enabled),
            egui::Event::Ime(egui::ImeEvent::Preedit("に".into())),
        ],
    );
    assert!(harness.app.virtual_editor_state.ime.preedit_range.is_some());
    frame(
        &mut harness.app,
        &ctx,
        vec![key_event(egui::Key::F1, egui::Modifiers::NONE)],
    );
    frame(
        &mut harness.app,
        &ctx,
        vec![egui::Event::Ime(egui::ImeEvent::Disabled)],
    );
    frame(
        &mut harness.app,
        &ctx,
        vec![key_event(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    frame(&mut harness.app, &ctx, vec![egui::Event::Text("x".into())]);
    assert!(
        harness.app.active_snapshot().ends_with('x'),
        "{}",
        harness.app.active_snapshot()
    );
}

#[test]
fn palette_enter_keeps_later_text_out_of_query() {
    let (mut harness, _events) = make_app_with_event_tx();
    let ctx = egui::Context::default();
    harness.app.focus_editor_next = true;
    frame(&mut harness.app, &ctx, vec![]);
    let end = harness.app.virtual_editor_buffer.len_chars();
    harness.app.virtual_editor_state.set_cursor(end, end);
    frame(&mut harness.app, &ctx, vec![discovery_chord(false)]);
    frame(
        &mut harness.app,
        &ctx,
        vec![
            egui::Event::Text("copy".into()),
            key_event(egui::Key::Enter, egui::Modifiers::NONE),
            egui::Event::Text("x".into()),
        ],
    );
    assert_eq!(harness.app.command_palette_query, "copy");
    assert!(!harness.app.command_palette_open);
    assert_eq!(harness.app.active_snapshot(), "contentx");
}

#[test]
fn ime_disabled_after_native_deactivation_does_not_poison_editor_typing() {
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
            egui::Event::Ime(egui::ImeEvent::Enabled),
            egui::Event::Ime(egui::ImeEvent::Preedit("に".into())),
        ],
    );
    assert!(harness.app.virtual_editor_state.ime.preedit_range.is_some());
    frame(
        &mut harness.app,
        &ctx,
        vec![
            egui::Event::WindowFocused(false),
            egui::Event::Ime(egui::ImeEvent::Disabled),
        ],
    );
    frame(
        &mut harness.app,
        &ctx,
        vec![egui::Event::WindowFocused(true)],
    );
    harness.app.focus_editor_next = true;
    frame(&mut harness.app, &ctx, vec![]);
    frame(&mut harness.app, &ctx, vec![egui::Event::Text("x".into())]);
    assert!(
        harness.app.active_snapshot().ends_with('x'),
        "{}",
        harness.app.active_snapshot()
    );
}

#[test]
fn editor_paste_before_native_deactivation_is_applied_without_global_creation() {
    let (mut harness, _events) = make_app_with_event_tx();
    let ctx = egui::Context::default();
    harness.app.focus_editor_next = true;
    frame(&mut harness.app, &ctx, vec![]);
    let end = harness.app.virtual_editor_buffer.len_chars();
    harness.app.virtual_editor_state.set_cursor(end, end);
    while harness.cmd_rx.try_recv().is_ok() {}
    frame(
        &mut harness.app,
        &ctx,
        vec![
            egui::Event::Paste("before".into()),
            egui::Event::WindowFocused(false),
        ],
    );
    assert_eq!(harness.app.active_snapshot(), "contentbefore");
    assert!(!harness.app.virtual_editor_state.has_focus);
    assert!(!harness
        .cmd_rx
        .try_iter()
        .any(|cmd| matches!(cmd, CoreCmd::CreatePaste { .. })));
}

#[test]
fn unmatched_palette_enter_keeps_its_suffix_in_the_query() {
    let (mut harness, _events) = make_app_with_event_tx();
    let ctx = egui::Context::default();
    frame(&mut harness.app, &ctx, vec![discovery_chord(false)]);
    frame(
        &mut harness.app,
        &ctx,
        vec![egui::Event::Text("no_such_command".into())],
    );
    one_frame(
        &mut harness.app,
        &ctx,
        vec![
            key_event(egui::Key::Enter, egui::Modifiers::NONE),
            egui::Event::Text("x".into()),
        ],
    );
    assert!(harness.app.command_palette_open);
    assert_eq!(harness.app.command_palette_query, "no_such_commandx");
    assert!(harness.app.deferred_discovery_events.is_empty());
}

#[test]
fn correcting_an_unmatched_query_before_enter_accepts_the_corrected_command() {
    let (mut harness, _events) = make_app_with_event_tx();
    let ctx = egui::Context::default();
    harness.app.focus_editor_next = true;
    frame(&mut harness.app, &ctx, vec![]);
    let end = harness.app.virtual_editor_buffer.len_chars();
    harness.app.virtual_editor_state.set_cursor(end, end);
    frame(&mut harness.app, &ctx, vec![discovery_chord(false)]);
    frame(
        &mut harness.app,
        &ctx,
        vec![egui::Event::Text("no_such_command".into())],
    );
    frame(
        &mut harness.app,
        &ctx,
        vec![
            command_key_event(egui::Key::A),
            egui::Event::Text("copy".into()),
            key_event(egui::Key::Enter, egui::Modifiers::NONE),
            egui::Event::Text("x".into()),
        ],
    );
    assert_eq!(harness.app.command_palette_query, "copy");
    assert!(!harness.app.command_palette_open);
    assert_eq!(harness.app.active_snapshot(), "contentx");
}

#[test]
fn canceling_selected_text_ime_on_discovery_preserves_original_text() {
    for boundary in [
        key_event(egui::Key::F1, egui::Modifiers::NONE),
        discovery_chord(false),
        discovery_chord(true),
        egui::Event::WindowFocused(false),
    ] {
        let (mut harness, _events) = make_app_with_event_tx();
        let ctx = egui::Context::default();
        harness.app.focus_editor_next = true;
        frame(&mut harness.app, &ctx, vec![]);
        let original = harness.app.active_snapshot();
        let len = harness.app.virtual_editor_buffer.len_chars();
        harness.app.virtual_editor_state.select_all(len);
        frame(
            &mut harness.app,
            &ctx,
            vec![
                egui::Event::Ime(egui::ImeEvent::Enabled),
                egui::Event::Ime(egui::ImeEvent::Preedit("に".into())),
            ],
        );
        assert_eq!(harness.app.active_snapshot(), "に");
        frame(&mut harness.app, &ctx, vec![boundary]);
        assert_eq!(harness.app.active_snapshot(), original);
        assert_eq!(
            harness.app.virtual_editor_state.selection_range(),
            Some(0..len)
        );
        assert_eq!(harness.app.virtual_editor_history.perf_stats().undo_len, 0);
    }
}

#[test]
fn selected_text_ime_commit_is_one_undoable_edit_after_preedit_updates() {
    for clear_preedit in [false, true] {
        for backwards in [false, true] {
            let (mut harness, _events) = make_app_with_event_tx();
            let ctx = egui::Context::default();
            harness.app.focus_editor_next = true;
            frame(&mut harness.app, &ctx, vec![]);
            let original = harness.app.active_snapshot();
            let len = harness.app.virtual_editor_buffer.len_chars();
            let (cursor, anchor) = if backwards { (0, len) } else { (len, 0) };
            harness
                .app
                .virtual_editor_state
                .restore_selection(cursor, Some(anchor), len);
            let mut events = vec![
                egui::Event::Ime(egui::ImeEvent::Enabled),
                egui::Event::Ime(egui::ImeEvent::Preedit("に".into())),
                egui::Event::Ime(egui::ImeEvent::Preedit("にほん".into())),
            ];
            if clear_preedit {
                events.push(egui::Event::Ime(egui::ImeEvent::Preedit(String::new())));
            }
            events.extend([
                egui::Event::Ime(egui::ImeEvent::Commit("日本".into())),
                egui::Event::Ime(egui::ImeEvent::Disabled),
            ]);
            frame(&mut harness.app, &ctx, events);
            assert_eq!(harness.app.active_snapshot(), "日本");
            assert_eq!(harness.app.virtual_editor_history.perf_stats().undo_len, 1);
            frame(
                &mut harness.app,
                &ctx,
                vec![command_key_event(egui::Key::Z)],
            );
            assert_eq!(harness.app.active_snapshot(), original);
            assert_eq!(harness.app.virtual_editor_state.cursor(), cursor);
            assert_eq!(harness.app.virtual_editor_state.anchor(), Some(anchor));
            let mut redo = primary_command_modifiers();
            redo.shift = true;
            frame(&mut harness.app, &ctx, vec![key_event(egui::Key::Z, redo)]);
            assert_eq!(harness.app.active_snapshot(), "日本");
            assert_eq!(harness.app.virtual_editor_state.selection_range(), None);
        }
    }
}

#[test]
fn picker_enter_suffix_waits_for_delayed_selection_load() {
    for (suffix, dirty, focus_tail) in [
        (egui::Event::Paste("pasted ".into()), false, vec![]),
        (egui::Event::Text("typed ".into()), false, vec![]),
        (egui::Event::Text("typed ".into()), true, vec![]),
        (egui::Event::Paste("pasted ".into()), true, vec![false]),
        (egui::Event::Text("typed ".into()), false, vec![false]),
        (egui::Event::Text("typed ".into()), false, vec![false, true]),
    ] {
        let (mut harness, _events) = make_app_with_event_tx();
        let ctx = egui::Context::default();
        harness
            .app
            .all_pastes
            .insert(0, test_summary("picked", "Picked", None, 7));
        harness.app.focus_editor_next = true;
        frame(&mut harness.app, &ctx, vec![]);
        frame(&mut harness.app, &ctx, vec![discovery_chord(true)]);
        if dirty {
            harness.app.save_status = SaveStatus::Dirty;
        }
        while harness.cmd_rx.try_recv().is_ok() {}
        let mut events = vec![
            key_event(egui::Key::Enter, egui::Modifiers::NONE),
            suffix.clone(),
        ];
        events.extend(focus_tail.iter().copied().map(egui::Event::WindowFocused));
        frame(&mut harness.app, &ctx, events);
        if focus_tail.is_empty() {
            frame(
                &mut harness.app,
                &ctx,
                vec![
                    key_event(egui::Key::End, egui::Modifiers::NONE),
                    egui::Event::Text("tail".into()),
                ],
            );
        }
        let mut dispatched: Vec<_> = harness.cmd_rx.try_iter().collect();
        if dirty {
            assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));
            assert_eq!(harness.app.active_snapshot(), "content");
            let mut saved = Paste::new("content".into(), "Alpha".into());
            saved.id = "alpha".into();
            harness
                .app
                .apply_event(CoreEvent::PasteSaved { paste: saved });
            dispatched.extend(harness.cmd_rx.try_iter());
        }
        assert_eq!(harness.app.selected_id.as_deref(), Some("picked"));
        assert!(harness.app.selected_paste.is_none());
        let mut picked = Paste::new("content".into(), "Picked".into());
        picked.id = "picked".into();
        harness.app.apply_event(CoreEvent::PasteLoaded {
            paste: picked,
            selection_epoch: harness.app.active_buffer_epoch,
        });
        frame(&mut harness.app, &ctx, vec![]);
        let prefix = match suffix {
            egui::Event::Text(_) => "typed content",
            _ => "pasted content",
        };
        let expected = if focus_tail.is_empty() {
            format!("{prefix}tail")
        } else {
            prefix.to_owned()
        };
        assert_eq!(
            harness.app.active_snapshot(),
            expected,
            "dispatched={dispatched:?}"
        );
        assert!(
            !dispatched
                .iter()
                .any(|cmd| matches!(cmd, CoreCmd::CreatePaste { .. })),
            "{dispatched:?}"
        );
        let expected_focus = focus_tail.last().copied().unwrap_or(true);
        assert_eq!(harness.app.virtual_editor_state.has_focus, expected_focus);
        assert_eq!(
            ctx.memory(|memory| memory.has_focus(egui::Id::new(VIRTUAL_EDITOR_ID))),
            expected_focus
        );
    }
}

#[test]
fn pending_picker_input_is_discarded_on_failure_reselection_or_workflow_cancel() {
    for outcome in [
        "failed",
        "missing",
        "save failed",
        "reselected",
        "escape",
        "discovery",
    ] {
        let (mut harness, _events) = make_app_with_event_tx();
        let ctx = egui::Context::default();
        frame(&mut harness.app, &ctx, vec![discovery_chord(true)]);
        if outcome == "save failed" {
            harness.app.save_status = SaveStatus::Dirty;
        }
        harness.app.open_palette_selection("picked".into());
        frame(
            &mut harness.app,
            &ctx,
            vec![egui::Event::Paste("discard me".into())],
        );
        assert!(!harness
            .app
            .pending_picker_open
            .as_ref()
            .unwrap()
            .input_events
            .is_empty());
        match outcome {
            "failed" => harness.app.apply_event(CoreEvent::PasteLoadFailed {
                id: "picked".into(),
                selection_epoch: harness.app.active_buffer_epoch,
                message: "Load failed".into(),
            }),
            "missing" => harness.app.apply_event(CoreEvent::PasteSelectionMissing {
                id: "picked".into(),
                selection_epoch: harness.app.active_buffer_epoch,
            }),
            "save failed" => harness.app.apply_event(CoreEvent::Error {
                source: crate::backend::CoreErrorSource::SaveContent,
                message: "Save failed".into(),
            }),
            "reselected" => {
                harness.app.select_paste("other".into());
            }
            "escape" => frame(
                &mut harness.app,
                &ctx,
                vec![key_event(egui::Key::Escape, egui::Modifiers::NONE)],
            ),
            _ => frame(&mut harness.app, &ctx, vec![discovery_chord(false)]),
        }
        assert!(harness.app.pending_picker_open.is_none(), "{outcome}");
        if outcome == "save failed" {
            let mut saved = Paste::new("content".into(), "Alpha".into());
            saved.id = "alpha".into();
            harness
                .app
                .apply_event(CoreEvent::PasteSaved { paste: saved });
        }
        if harness.app.selected_id.as_deref() != Some("other") {
            harness.app.select_paste("other".into());
        }
        let mut paste = Paste::new("other content".into(), "Other".into());
        paste.id = "other".into();
        harness.app.apply_event(CoreEvent::PasteLoaded {
            paste,
            selection_epoch: harness.app.active_buffer_epoch,
        });
        frame(&mut harness.app, &ctx, vec![]);
        assert_eq!(harness.app.active_snapshot(), "other content", "{outcome}");
        assert!(!harness.app.command_palette_query.contains("discard me"));
        assert!(
            !harness
                .cmd_rx
                .try_iter()
                .any(|cmd| matches!(cmd, CoreCmd::CreatePaste { .. })),
            "{outcome}"
        );
    }
}
