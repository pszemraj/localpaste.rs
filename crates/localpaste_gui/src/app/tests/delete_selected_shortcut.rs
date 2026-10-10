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
