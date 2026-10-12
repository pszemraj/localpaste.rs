//! Platform routing for the delete-selected-paste chord and its macOS alias.

use super::*;
use crate::app::virtual_editor::PlatformFlavor;

const PLATFORMS: [PlatformFlavor; 2] = [PlatformFlavor::Mac, PlatformFlavor::Other];

/// Builds the platform's primary command modifiers as egui-winit reports them.
fn command(platform: PlatformFlavor, shift: bool) -> egui::Modifiers {
    egui::Modifiers {
        command: true,
        mac_cmd: platform == PlatformFlavor::Mac,
        ctrl: platform == PlatformFlavor::Other,
        shift,
        ..Default::default()
    }
}

fn press(app: &mut LocalPasteApp, ctx: &egui::Context, key: egui::Key, modifiers: egui::Modifiers) {
    run_discovery_frame_with_modifiers(app, ctx, vec![key_event(key, modifiers)], modifiers);
}

fn sent_delete(harness: &TestHarness) -> bool {
    harness
        .cmd_rx
        .try_iter()
        .any(|command| matches!(command, CoreCmd::DeletePaste { id } if id == "alpha"))
}

#[test]
fn unfocused_delete_selected_accepts_cmd_backspace_only_on_macos() {
    for platform in PLATFORMS {
        with_platform(platform, || {
            for (key, expected) in [
                (egui::Key::Delete, true),
                (egui::Key::Backspace, platform == PlatformFlavor::Mac),
            ] {
                let (mut harness, _events) = make_app_with_event_tx();
                let ctx = egui::Context::default();
                run_discovery_frame_once(&mut harness.app, &ctx, vec![]);
                press(&mut harness.app, &ctx, key, command(platform, false));
                assert_eq!(sent_delete(&harness), expected, "{platform:?} {key:?}");
            }
        });
    }
}

#[test]
fn focused_text_inputs_keep_command_backspace_and_delete_as_text_editing() {
    for platform in PLATFORMS {
        with_platform(platform, || {
            for key in [egui::Key::Backspace, egui::Key::Delete] {
                for owner in [
                    VIRTUAL_EDITOR_ID,
                    TITLE_INPUT_ID,
                    SEARCH_INPUT_ID,
                    PROPERTIES_NAME_INPUT_ID,
                    PROPERTIES_TAGS_INPUT_ID,
                    EDITOR_FIND_INPUT_ID,
                    PASTE_PICKER_INPUT_ID,
                    COMMAND_PALETTE_INPUT_ID,
                ] {
                    let (mut harness, _events) = make_app_with_event_tx();
                    let ctx = egui::Context::default();
                    harness.app.reset_virtual_editor("alpha beta");
                    harness.app.properties_drawer_open = true;
                    harness.app.edit_name = "abcdefgh".into();
                    harness.app.edit_tags = "abcdefgh".into();
                    harness.app.set_search_query("abcdefgh".into());
                    harness.app.open_editor_find();
                    harness.app.set_editor_find_query("abcdefgh".into());
                    run_discovery_frame_once(&mut harness.app, &ctx, vec![]);
                    match owner {
                        VIRTUAL_EDITOR_ID => harness.app.focus_editor_next = true,
                        PASTE_PICKER_INPUT_ID | COMMAND_PALETTE_INPUT_ID => {
                            let picker = owner == PASTE_PICKER_INPUT_ID;
                            press(
                                &mut harness.app,
                                &ctx,
                                egui::Key::K,
                                command(platform, picker),
                            );
                        }
                        _ => ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(owner))),
                    }
                    run_discovery_frame_once(&mut harness.app, &ctx, vec![]);
                    assert!(
                        ctx.memory(|memory| memory.has_focus(egui::Id::new(owner))),
                        "{platform:?} {owner}"
                    );
                    harness.app.virtual_editor_state.set_cursor(10, 10);
                    let _ = harness.cmd_rx.try_iter().count();

                    press(&mut harness.app, &ctx, key, command(platform, false));

                    assert!(!sent_delete(&harness), "{platform:?} {key:?} {owner}");
                    assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));
                    if owner == VIRTUAL_EDITOR_ID {
                        // Cmd+Backspace deletes to line start on macOS; Ctrl+Backspace
                        // deletes the previous word elsewhere. Forward Delete at EOF
                        // leaves the text alone on both.
                        let expected = match (key, platform) {
                            (egui::Key::Backspace, PlatformFlavor::Mac) => "",
                            (egui::Key::Backspace, PlatformFlavor::Other) => "alpha ",
                            _ => "alpha beta",
                        };
                        assert_eq!(
                            harness.app.virtual_editor_buffer.to_string(),
                            expected,
                            "{platform:?} {key:?}"
                        );
                    } else {
                        assert_eq!(
                            harness.app.virtual_editor_buffer.to_string(),
                            "alpha beta",
                            "{platform:?} {key:?} {owner}"
                        );
                    }
                }
            }
        });
    }
}

/// Runs a frame, then empty frames until input deferred around a discovery focus change is rendered.
fn settle(app: &mut LocalPasteApp, ctx: &egui::Context, events: Vec<egui::Event>) {
    run_discovery_frame_once(app, ctx, events);
    for _ in 0..8 {
        if app.deferred_discovery_events.is_empty() {
            return;
        }
        run_discovery_frame_once(app, ctx, vec![]);
    }
    assert!(app.deferred_discovery_events.is_empty());
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
        settle(&mut harness.app, &ctx, vec![]);
        let picker_chord = command(PlatformFlavor::Mac, true);
        settle(
            &mut harness.app,
            &ctx,
            vec![key_event(egui::Key::K, picker_chord)],
        );
        assert!(harness.app.paste_picker_open);
        let _ = harness.cmd_rx.try_iter().count();

        settle(
            &mut harness.app,
            &ctx,
            vec![
                key_event(egui::Key::Enter, egui::Modifiers::NONE),
                egui::Event::Text("typed ".into()),
            ],
        );
        // A later frame, once the closed picker query no longer holds egui focus.
        settle(&mut harness.app, &ctx, vec![]);
        press(
            &mut harness.app,
            &ctx,
            egui::Key::Backspace,
            command(PlatformFlavor::Mac, false),
        );
        let mut cmds = harness.cmd_rx.try_iter().collect::<Vec<_>>();
        assert_eq!(harness.app.selected_id.as_deref(), Some("picked"));
        assert!(harness.app.selected_paste.is_none(), "still loading");
        let mut picked = Paste::new("content".into(), "Picked".into());
        picked.id = "picked".into();
        harness.app.apply_event(CoreEvent::PasteLoaded {
            paste: picked,
            selection_epoch: harness.app.active_buffer_epoch,
        });
        settle(&mut harness.app, &ctx, vec![]);
        cmds.extend(harness.cmd_rx.try_iter());

        // The held typing and `Cmd+Backspace` replay as one line edit in the loaded paste.
        assert!(
            !cmds
                .iter()
                .any(|cmd| matches!(cmd, CoreCmd::DeletePaste { .. })),
            "{cmds:?}"
        );
        assert_eq!(harness.app.selected_id.as_deref(), Some("picked"));
        assert_eq!(harness.app.active_snapshot(), "content");
        assert!(harness.app.virtual_editor_state.has_focus);
    });
}
