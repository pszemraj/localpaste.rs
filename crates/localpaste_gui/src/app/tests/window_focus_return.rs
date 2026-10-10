//! Editor keyboard ownership across native window deactivation and reactivation.

use super::virtual_editor_focus_support::{editor_body_click_pos, primary_click_events};
use super::*;
use crate::app::virtual_editor::PlatformFlavor;

/// `Cmd` as egui-winit reports it on macOS.
fn mac_command(shift: bool) -> egui::Modifiers {
    egui::Modifiers {
        command: true,
        mac_cmd: true,
        shift,
        ..Default::default()
    }
}

/// Runs frames until input deferred around discovery focus changes is rendered.
fn frame(harness: &mut TestHarness, ctx: &egui::Context, events: Vec<egui::Event>) {
    run_discovery_frame_once(&mut harness.app, ctx, events);
    for _ in 0..8 {
        if harness.app.deferred_discovery_events.is_empty() {
            return;
        }
        run_discovery_frame_once(&mut harness.app, ctx, vec![]);
    }
    assert!(harness.app.deferred_discovery_events.is_empty());
}

/// Presses `Cmd+Backspace` with the modifier snapshot egui-winit delivers.
fn press_cmd_backspace(harness: &mut TestHarness, ctx: &egui::Context) {
    let modifiers = mac_command(false);
    run_discovery_frame_with_modifiers(
        &mut harness.app,
        ctx,
        vec![key_event(egui::Key::Backspace, modifiers)],
        modifiers,
    );
}

/// Opens `text` in a focused editor with the caret at `cursor`.
fn focused_editor(text: &str, cursor: usize) -> (TestHarness, egui::Context) {
    let (mut harness, _events) = make_app_with_event_tx();
    let ctx = egui::Context::default();
    harness.app.reset_virtual_editor(text);
    harness.app.focus_editor_next = true;
    frame(&mut harness, &ctx, vec![]);
    let len = harness.app.virtual_editor_buffer.len_chars();
    harness.app.virtual_editor_state.set_cursor(cursor, len);
    (harness, ctx)
}

/// Switches away from the app and back in separate frames, as `Cmd+Tab` does.
fn switch_away_and_back(harness: &mut TestHarness, ctx: &egui::Context) {
    frame(harness, ctx, vec![egui::Event::WindowFocused(false)]);
    assert!(!harness.app.virtual_editor_state.has_focus);
    frame(harness, ctx, vec![egui::Event::WindowFocused(true)]);
}

/// Whether the editor owns keyboard input in both app state and egui memory.
fn editor_has_keyboard(harness: &TestHarness, ctx: &egui::Context) -> bool {
    harness.app.virtual_editor_state.has_focus
        && ctx.memory(|memory| memory.has_focus(egui::Id::new(VIRTUAL_EDITOR_ID)))
}

/// Takes every backend command sent so far.
fn drain(harness: &TestHarness) -> Vec<CoreCmd> {
    harness.cmd_rx.try_iter().collect()
}

/// Whether any command deletes a paste.
fn deletes_paste(cmds: &[CoreCmd]) -> bool {
    cmds.iter()
        .any(|cmd| matches!(cmd, CoreCmd::DeletePaste { .. }))
}

/// Whether any command creates a paste.
fn creates_paste(cmds: &[CoreCmd]) -> bool {
    cmds.iter()
        .any(|cmd| matches!(cmd, CoreCmd::CreatePaste { .. }))
}

#[test]
fn cmd_backspace_after_switching_back_deletes_to_line_start_not_the_paste() {
    with_platform(PlatformFlavor::Mac, || {
        let (mut harness, ctx) = focused_editor("alpha beta\ngamma delta", 16);
        switch_away_and_back(&mut harness, &ctx);
        drain(&harness);

        press_cmd_backspace(&mut harness, &ctx);

        assert!(!deletes_paste(&drain(&harness)));
        assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));
        assert_eq!(harness.app.active_snapshot(), "alpha beta\n delta");
        assert_eq!(harness.app.virtual_editor_state.cursor(), 11);
    });
}

#[test]
fn switching_back_restores_the_caret_selection_and_typing() {
    let (mut harness, ctx) = focused_editor("alpha beta", 0);
    harness
        .app
        .virtual_editor_state
        .restore_selection(5, Some(1), 10);
    frame(&mut harness, &ctx, vec![egui::Event::WindowFocused(false)]);
    assert!(!editor_has_keyboard(&harness, &ctx));
    // No caret blink keeps repainting the inactive window.
    let inactive = run_discovery_frame_once(&mut harness.app, &ctx, vec![]);
    assert!(inactive.platform_output.ime.is_none());

    let output = run_discovery_frame_once(
        &mut harness.app,
        &ctx,
        vec![egui::Event::WindowFocused(true)],
    );

    assert!(editor_has_keyboard(&harness, &ctx));
    assert!(output.platform_output.ime.is_some(), "caret and IME resume");
    assert_eq!(
        harness.app.virtual_editor_state.selection_range(),
        Some(1..5)
    );
    frame(&mut harness, &ctx, vec![egui::Event::Text("X".into())]);
    assert_eq!(harness.app.active_snapshot(), "aX beta");
    assert_eq!(harness.app.virtual_editor_state.cursor(), 2);
}

#[test]
fn cmd_backspace_after_switching_back_from_the_list_still_deletes_the_paste() {
    with_platform(PlatformFlavor::Mac, || {
        let (mut harness, ctx) = focused_editor("alpha beta", 10);
        let output = run_discovery_frame_once(&mut harness.app, &ctx, vec![]);
        let row = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text)
                    if text.galley.job.text == "Alpha" && text.pos.x < 300.0 =>
                {
                    Some(text.pos + text.galley.size() / 2.0)
                }
                _ => None,
            })
            .expect("sidebar row");
        frame(&mut harness, &ctx, primary_click_events(row));
        frame(&mut harness, &ctx, primary_pointer_events(row, false));
        assert!(!editor_has_keyboard(&harness, &ctx));
        switch_away_and_back(&mut harness, &ctx);
        assert!(!editor_has_keyboard(&harness, &ctx));
        drain(&harness);

        press_cmd_backspace(&mut harness, &ctx);

        assert!(deletes_paste(&drain(&harness)));
        assert_eq!(harness.app.active_snapshot(), "alpha beta");
    });
}

#[test]
fn deactivation_cancels_ime_composition_and_reactivation_does_not_restore_it() {
    let (mut harness, ctx) = focused_editor("hello world", 5);
    frame(
        &mut harness,
        &ctx,
        vec![
            egui::Event::Ime(egui::ImeEvent::Enabled),
            egui::Event::Ime(egui::ImeEvent::Preedit("に".into())),
        ],
    );
    assert!(harness.app.virtual_editor_state.ime.preedit_range.is_some());
    assert_eq!(harness.app.active_snapshot(), "helloに world");

    frame(&mut harness, &ctx, vec![egui::Event::WindowFocused(false)]);
    assert!(harness.app.virtual_editor_state.ime.preedit_range.is_none());
    assert_eq!(harness.app.active_snapshot(), "hello world");

    frame(&mut harness, &ctx, vec![egui::Event::WindowFocused(true)]);
    assert!(editor_has_keyboard(&harness, &ctx));
    assert!(harness.app.virtual_editor_state.ime.preedit_range.is_none());
    assert_eq!(harness.app.active_snapshot(), "hello world");
    frame(&mut harness, &ctx, vec![egui::Event::Text(",".into())]);
    assert_eq!(harness.app.active_snapshot(), "hello, world");
}

#[test]
fn first_paste_after_switching_back_still_creates_a_new_paste() {
    for before_paste in [None, Some(egui::Event::Copy)] {
        let (mut harness, ctx) = focused_editor("alpha beta", 5);
        switch_away_and_back(&mut harness, &ctx);
        assert!(editor_has_keyboard(&harness, &ctx));
        if let Some(event) = before_paste.clone() {
            frame(&mut harness, &ctx, vec![event]);
        }
        drain(&harness);

        for _ in 0..2 {
            // egui-winit's native shape: Paste only, with Cmd in the frame modifiers.
            let output = run_discovery_frame_with_modifiers(
                &mut harness.app,
                &ctx,
                vec![egui::Event::Paste("clip".into())],
                primary_command_modifiers(),
            );

            let created: Vec<_> = drain(&harness)
                .into_iter()
                .filter(|cmd| matches!(cmd, CoreCmd::CreatePaste { content } if content == "clip"))
                .collect();
            assert_eq!(created.len(), 1, "{before_paste:?}");
            assert!(
                !output.viewport_output.values().any(|viewport| viewport
                    .commands
                    .contains(&egui::ViewportCommand::RequestPaste)),
                "{before_paste:?}"
            );
            assert_eq!(harness.app.active_snapshot(), "alpha beta");
            // As with nothing focused, the next paste creates another paste.
            assert!(!editor_has_keyboard(&harness, &ctx), "{before_paste:?}");
        }
    }
}

#[test]
fn key_only_paste_after_switching_back_requests_the_clipboard_for_a_new_paste() {
    let (mut harness, ctx) = focused_editor("alpha beta", 5);
    switch_away_and_back(&mut harness, &ctx);
    drain(&harness);

    // Integrations that deliver Key(V) without a payload need a clipboard request.
    let output = run_discovery_frame_once(
        &mut harness.app,
        &ctx,
        vec![command_key_event(egui::Key::V)],
    );
    assert!(output.viewport_output.values().any(|viewport| viewport
        .commands
        .contains(&egui::ViewportCommand::RequestPaste)));
    frame(&mut harness, &ctx, vec![egui::Event::Paste("clip".into())]);

    assert!(drain(&harness)
        .iter()
        .any(|cmd| matches!(cmd, CoreCmd::CreatePaste { content } if content == "clip")));
    assert_eq!(harness.app.active_snapshot(), "alpha beta");
}

#[test]
fn editing_after_switching_back_ends_the_new_paste_capture() {
    for (edit, expected) in [
        (vec![egui::Event::Text("X".into())], "alphaXclip beta"),
        (
            vec![key_event(egui::Key::ArrowRight, egui::Modifiers::NONE)],
            "alpha clipbeta",
        ),
        // A click below the text places the caret at the end.
        (
            primary_click_events(editor_body_click_pos()),
            "alpha betaclip",
        ),
    ] {
        for same_frame in [false, true] {
            let (mut harness, ctx) = focused_editor("alpha beta", 5);
            frame(&mut harness, &ctx, vec![egui::Event::WindowFocused(false)]);
            let mut events = vec![egui::Event::WindowFocused(true)];
            if same_frame {
                events.extend(edit.clone());
            } else {
                frame(&mut harness, &ctx, events);
                events = edit.clone();
            }
            frame(&mut harness, &ctx, events);
            frame(
                &mut harness,
                &ctx,
                primary_pointer_events(editor_body_click_pos(), false),
            );
            drain(&harness);

            frame(&mut harness, &ctx, vec![egui::Event::Paste("clip".into())]);

            let context = format!("{edit:?} same_frame={same_frame}");
            assert!(!creates_paste(&drain(&harness)), "{context}");
            assert_eq!(harness.app.active_snapshot(), expected, "{context}");
            assert!(editor_has_keyboard(&harness, &ctx), "{context}");
        }
    }
}

#[test]
fn switching_back_leaves_focus_alone_when_something_changed_meanwhile() {
    for change in ["selection", "overlay", "click", "deleted", "field"] {
        let (mut harness, ctx) = focused_editor("alpha beta", 5);
        frame(&mut harness, &ctx, vec![egui::Event::WindowFocused(false)]);
        let mut events = vec![egui::Event::WindowFocused(true)];
        match change {
            "selection" => assert!(harness.app.select_paste("other".into())),
            "overlay" => harness.app.version_ui.history_modal_open = true,
            "deleted" => harness.app.apply_paste_deleted("alpha".into(), None),
            "field" => {
                let title = egui::Id::new(TITLE_INPUT_ID);
                ctx.memory_mut(|memory| memory.request_focus(title));
            }
            _ => events.extend(primary_click_events(egui::pos2(150.0, 860.0))),
        }

        frame(&mut harness, &ctx, events);

        assert!(!editor_has_keyboard(&harness, &ctx), "{change}");
        if change == "field" {
            assert!(ctx.memory(|memory| memory.has_focus(egui::Id::new(TITLE_INPUT_ID))));
        }
    }
}

#[test]
fn cmd_backspace_while_a_picker_paste_loads_is_held_as_editor_input() {
    with_platform(PlatformFlavor::Mac, || {
        let (mut harness, _events) = make_app_with_event_tx();
        let ctx = egui::Context::default();
        harness
            .app
            .all_pastes
            .insert(0, test_summary("picked", "Picked", None, 7));
        harness.app.focus_editor_next = true;
        frame(&mut harness, &ctx, vec![]);
        frame(
            &mut harness,
            &ctx,
            vec![key_event(egui::Key::K, mac_command(true))],
        );
        assert!(harness.app.paste_picker_open);
        drain(&harness);

        frame(
            &mut harness,
            &ctx,
            vec![
                key_event(egui::Key::Enter, egui::Modifiers::NONE),
                egui::Event::Text("typed ".into()),
            ],
        );
        // A later frame, once the closed picker query no longer holds egui focus.
        frame(&mut harness, &ctx, vec![]);
        press_cmd_backspace(&mut harness, &ctx);
        let mut cmds = drain(&harness);
        assert_eq!(harness.app.selected_id.as_deref(), Some("picked"));
        assert!(harness.app.selected_paste.is_none(), "still loading");
        let mut picked = Paste::new("content".into(), "Picked".into());
        picked.id = "picked".into();
        harness.app.apply_event(CoreEvent::PasteLoaded {
            paste: picked,
            selection_epoch: harness.app.active_buffer_epoch,
        });
        frame(&mut harness, &ctx, vec![]);
        cmds.extend(drain(&harness));

        assert!(!deletes_paste(&cmds), "{cmds:?}");
        assert_eq!(harness.app.selected_id.as_deref(), Some("picked"));
        assert_eq!(harness.app.active_snapshot(), "content");
        assert!(editor_has_keyboard(&harness, &ctx));
    });
}
