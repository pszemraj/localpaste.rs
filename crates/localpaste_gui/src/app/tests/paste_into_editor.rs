//! `Ctrl/Cmd+Shift+V` inserts clipboard text into the open paste from any focus.

use super::*;

fn command_shift() -> egui::Modifiers {
    egui::Modifiers {
        shift: true,
        ..primary_command_modifiers()
    }
}

/// Sends egui-winit's native shape: Paste only, with the chord in frame modifiers.
fn press_paste_into_editor(harness: &mut TestHarness, ctx: &egui::Context, text: &str) {
    run_discovery_frame_with_modifiers(
        &mut harness.app,
        ctx,
        vec![egui::Event::Paste(text.into())],
        command_shift(),
    );
    run_discovery_frame_once(&mut harness.app, ctx, Vec::new());
}

fn drain_cmds(harness: &TestHarness) -> Vec<CoreCmd> {
    harness.cmd_rx.try_iter().collect()
}

#[test]
fn command_shift_v_uses_caret_only_in_focused_editor_and_appends_elsewhere() {
    for focus in ["editor", "deactivated", "search"] {
        let (mut harness, _events) = make_app_with_event_tx();
        let ctx = egui::Context::default();
        harness.app.reset_virtual_editor("hello world");
        harness.app.focus_editor_next = true;
        run_discovery_frame_once(&mut harness.app, &ctx, Vec::new());
        harness
            .app
            .virtual_editor_state
            .restore_selection(5, None, "hello world".chars().count());
        match focus {
            "deactivated" => {
                run_discovery_frame_once(
                    &mut harness.app,
                    &ctx,
                    vec![egui::Event::WindowFocused(false)],
                );
                run_discovery_frame_once(
                    &mut harness.app,
                    &ctx,
                    vec![egui::Event::WindowFocused(true)],
                );
                assert!(!harness.app.virtual_editor_state.has_focus);
            }
            "search" => {
                harness.app.search_focus_requested = true;
                run_discovery_frame_once(&mut harness.app, &ctx, Vec::new());
                run_discovery_frame_once(&mut harness.app, &ctx, Vec::new());
                assert!(ctx.egui_wants_keyboard_input());
                assert!(!harness.app.virtual_editor_state.has_focus);
            }
            _ => assert!(harness.app.virtual_editor_state.has_focus),
        }
        drain_cmds(&harness);

        press_paste_into_editor(&mut harness, &ctx, ",");

        let expected = match focus {
            "editor" => "hello, world",
            _ => "hello world\n,",
        };
        assert_eq!(harness.app.active_snapshot(), expected, "{focus}");
        assert_eq!(
            harness.app.virtual_editor_state.cursor(),
            expected.find(',').unwrap() + 1,
            "{focus}"
        );
        assert_eq!(harness.app.search_query, "", "{focus}");
        assert!(harness.app.virtual_editor_state.has_focus, "{focus}");
        assert!(
            !drain_cmds(&harness)
                .iter()
                .any(|cmd| matches!(cmd, CoreCmd::CreatePaste { .. })),
            "{focus}"
        );
    }
}

#[test]
fn command_shift_v_without_open_paste_appends_to_top_paste_after_load() {
    for existing in ["line one", "line one\n", ""] {
        let (mut harness, _events) = make_app_with_event_tx();
        let ctx = egui::Context::default();
        harness.app.selected_id = None;
        harness.app.selected_paste = None;
        harness.app.reset_virtual_editor("");

        press_paste_into_editor(&mut harness, &ctx, "clip");

        let cmds = drain_cmds(&harness);
        assert!(cmds
            .iter()
            .any(|cmd| matches!(cmd, CoreCmd::GetPaste { id, .. } if id == "alpha")));
        assert!(!cmds
            .iter()
            .any(|cmd| matches!(cmd, CoreCmd::CreatePaste { .. })));
        let mut loaded = Paste::new(existing.into(), "Alpha".into());
        loaded.id = "alpha".into();
        harness.app.apply_event(CoreEvent::PasteLoaded {
            paste: loaded,
            selection_epoch: harness.app.active_buffer_epoch,
        });
        run_discovery_frame_once(&mut harness.app, &ctx, Vec::new());

        let expected = match existing {
            "" => "clip".to_string(),
            "line one" => "line one\nclip".to_string(),
            _ => format!("{existing}clip"),
        };
        assert_eq!(harness.app.active_snapshot(), expected);
        assert!(harness.app.pending_editor_paste.is_none());
        assert!(harness.app.virtual_editor_state.has_focus);
        assert_eq!(harness.app.save_status, SaveStatus::Dirty);
    }
}

#[test]
fn command_shift_v_appends_after_an_abandoned_ime_composition() {
    let (mut harness, _events) = make_app_with_event_tx();
    let ctx = egui::Context::default();
    harness.app.reset_virtual_editor("hello world");
    harness.app.focus_editor_next = true;
    run_discovery_frame_once(&mut harness.app, &ctx, Vec::new());
    harness.app.virtual_editor_state.set_cursor(5, 11);
    run_discovery_frame_once(
        &mut harness.app,
        &ctx,
        vec![egui::Event::Ime(egui::ImeEvent::Preedit {
            text: "に".into(),
            active_range_chars: None,
        })],
    );
    assert!(harness.app.virtual_editor_state.ime.preedit_range.is_some());
    harness.app.search_focus_requested = true;
    run_discovery_frame_once(&mut harness.app, &ctx, Vec::new());
    run_discovery_frame_once(&mut harness.app, &ctx, Vec::new());
    assert!(!harness.app.virtual_editor_state.has_focus);

    press_paste_into_editor(&mut harness, &ctx, "clip");

    let expected = "hello world\nclip";
    assert_eq!(harness.app.active_snapshot(), expected);
    assert_eq!(
        harness.app.virtual_editor_state.cursor(),
        expected.chars().count()
    );
    assert!(harness.app.virtual_editor_state.ime.preedit_range.is_none());
}

#[test]
fn command_shift_v_on_load_keeps_the_caret_after_the_text_with_find_open() {
    let (mut harness, _events) = make_app_with_event_tx();
    let ctx = egui::Context::default();
    harness.app.selected_id = None;
    harness.app.selected_paste = None;
    harness.app.reset_virtual_editor("");
    harness.app.open_editor_find();
    harness.app.set_editor_find_query("needle".into());
    run_discovery_frame_once(&mut harness.app, &ctx, Vec::new());

    press_paste_into_editor(&mut harness, &ctx, "clip");
    let mut loaded = Paste::new("needle one\nneedle two".into(), "Alpha".into());
    loaded.id = "alpha".into();
    harness.app.apply_event(CoreEvent::PasteLoaded {
        paste: loaded,
        selection_epoch: harness.app.active_buffer_epoch,
    });
    run_discovery_frame_once(&mut harness.app, &ctx, Vec::new());
    run_discovery_frame_once(&mut harness.app, &ctx, Vec::new());

    let expected = "needle one\nneedle two\nclip";
    assert_eq!(harness.app.active_snapshot(), expected);
    assert_eq!(
        harness.app.virtual_editor_state.cursor(),
        expected.chars().count()
    );
    assert!(harness.app.virtual_editor_state.selection_range().is_none());
    assert_eq!(harness.app.editor_find.matches.len(), 2);
}

#[test]
fn command_shift_v_repeated_presses_append_each_payload_in_order() {
    // Loaded or still loading, with presses in separate frames or in one native batch.
    for (loaded_first, same_batch) in [(false, false), (false, true), (true, false), (true, true)] {
        let case = format!("loaded_first={loaded_first} same_batch={same_batch}");
        let (mut harness, _events) = make_app_with_event_tx();
        let ctx = egui::Context::default();
        let load = |harness: &mut TestHarness| {
            let mut loaded = Paste::new("existing".into(), "Alpha".into());
            loaded.id = "alpha".into();
            harness.app.apply_event(CoreEvent::PasteLoaded {
                paste: loaded,
                selection_epoch: harness.app.active_buffer_epoch,
            });
        };
        harness.app.selected_id = None;
        harness.app.selected_paste = None;
        harness.app.reset_virtual_editor("");
        if loaded_first {
            assert!(harness.app.select_paste("alpha".into()));
            load(&mut harness);
            run_discovery_frame_once(&mut harness.app, &ctx, Vec::new());
        }

        if same_batch {
            run_discovery_frame_with_modifiers(
                &mut harness.app,
                &ctx,
                vec![
                    egui::Event::Paste("first".into()),
                    egui::Event::Paste("second".into()),
                ],
                command_shift(),
            );
            run_discovery_frame_once(&mut harness.app, &ctx, Vec::new());
        } else {
            press_paste_into_editor(&mut harness, &ctx, "first");
            press_paste_into_editor(&mut harness, &ctx, "second");
        }
        if !loaded_first {
            load(&mut harness);
            run_discovery_frame_once(&mut harness.app, &ctx, Vec::new());
        }

        // Every case matches the paste loading before the first press.
        assert_eq!(
            harness.app.active_snapshot(),
            "existing\nfirstsecond",
            "{case}"
        );
        assert!(harness.app.pending_editor_paste.is_none(), "{case}");
        for expected in ["existing\nfirst", "existing"] {
            harness
                .app
                .apply_virtual_commands(&ctx, &[VirtualInputCommand::Undo]);
            assert_eq!(harness.app.active_snapshot(), expected, "{case}");
        }
    }
}

#[test]
fn command_shift_v_after_switching_pastes_appends_whether_or_not_loaded() {
    for loaded_first in [false, true] {
        let (mut harness, _events) = make_app_with_event_tx();
        let ctx = egui::Context::default();
        harness
            .app
            .pastes
            .push(test_summary("beta", "Beta", None, 9));
        run_discovery_frame_once(&mut harness.app, &ctx, Vec::new());
        assert!(harness.app.select_paste("beta".into()));
        let load = |harness: &mut TestHarness| {
            let mut paste = Paste::new("beta body".into(), "Beta".into());
            paste.id = "beta".into();
            harness.app.apply_event(CoreEvent::PasteLoaded {
                paste,
                selection_epoch: harness.app.active_buffer_epoch,
            });
        };
        if loaded_first {
            load(&mut harness);
            run_discovery_frame_once(&mut harness.app, &ctx, Vec::new());
            assert!(!harness.app.virtual_editor_state.has_focus);
        }

        press_paste_into_editor(&mut harness, &ctx, "clip");
        if !loaded_first {
            load(&mut harness);
            run_discovery_frame_once(&mut harness.app, &ctx, Vec::new());
        }

        assert_eq!(
            harness.app.active_snapshot(),
            "beta body\nclip",
            "loaded_first={loaded_first}"
        );
        assert!(harness.app.virtual_editor_state.has_focus);
    }
}

#[test]
fn command_shift_v_during_a_switch_deferred_by_a_save_appends_to_the_chosen_paste() {
    let (mut harness, _events) = make_app_with_event_tx();
    let ctx = egui::Context::default();
    harness
        .app
        .all_pastes
        .push(test_summary("beta", "Beta", None, 9));
    harness.app.pastes = harness.app.all_pastes.clone().into();
    run_discovery_frame_once(&mut harness.app, &ctx, Vec::new());
    harness.app.reset_virtual_editor("alpha edited");
    harness.app.mark_dirty();
    assert!(harness.app.select_paste("beta".into()));
    assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));
    assert_eq!(harness.app.pending_selection_id.as_deref(), Some("beta"));
    drain_cmds(&harness);

    press_paste_into_editor(&mut harness, &ctx, "clip");

    assert_eq!(
        harness.app.active_snapshot(),
        "alpha edited",
        "the outgoing paste must not receive the text"
    );
    assert_eq!(harness.app.pending_selection_id.as_deref(), Some("beta"));
    let mut saved = Paste::new("alpha edited".into(), "Alpha".into());
    saved.id = "alpha".into();
    harness
        .app
        .apply_event(CoreEvent::PasteSaved { paste: saved });
    assert_eq!(harness.app.selected_id.as_deref(), Some("beta"));
    let mut beta = Paste::new("beta body".into(), "Beta".into());
    beta.id = "beta".into();
    harness.app.apply_event(CoreEvent::PasteLoaded {
        paste: beta,
        selection_epoch: harness.app.active_buffer_epoch,
    });
    run_discovery_frame_once(&mut harness.app, &ctx, Vec::new());

    assert_eq!(harness.app.active_snapshot(), "beta body\nclip");
    assert!(harness.app.pending_editor_paste.is_none());
    assert!(harness.app.virtual_editor_state.has_focus);
}

#[test]
fn command_shift_v_pending_append_is_dropped_when_selection_moves_elsewhere() {
    let (mut harness, _events) = make_app_with_event_tx();
    let ctx = egui::Context::default();
    harness.app.selected_id = None;
    harness.app.selected_paste = None;
    harness.app.reset_virtual_editor("");
    harness
        .app
        .pastes
        .push(test_summary("beta", "Beta", None, 3));

    press_paste_into_editor(&mut harness, &ctx, "clip");
    assert!(harness.app.pending_editor_paste.is_some());
    harness.app.select_paste("beta".into());
    let mut loaded = Paste::new("beta body".into(), "Beta".into());
    loaded.id = "beta".into();
    harness.app.apply_event(CoreEvent::PasteLoaded {
        paste: loaded,
        selection_epoch: harness.app.active_buffer_epoch,
    });
    run_discovery_frame_once(&mut harness.app, &ctx, Vec::new());

    assert!(harness.app.pending_editor_paste.is_none());
    assert_eq!(harness.app.active_snapshot(), "beta body");
}

#[test]
fn command_shift_v_with_no_pastes_creates_one() {
    let (mut harness, _events) = make_app_with_event_tx();
    let ctx = egui::Context::default();
    harness.app.selected_id = None;
    harness.app.selected_paste = None;
    harness.app.pastes.clear();
    harness.app.all_pastes.clear();
    harness.app.reset_virtual_editor("");

    press_paste_into_editor(&mut harness, &ctx, "clip");

    assert!(drain_cmds(&harness)
        .iter()
        .any(|cmd| matches!(cmd, CoreCmd::CreatePaste { content, .. } if content == "clip")));
}

#[test]
fn command_shift_v_leaves_discovery_overlay_input_alone() {
    for overlay in ["palette", "picker", "help"] {
        let (mut harness, _events) = make_app_with_event_tx();
        let ctx = egui::Context::default();
        harness.app.reset_virtual_editor("original");
        match overlay {
            "palette" => harness.app.command_palette_open = true,
            "picker" => harness.app.open_paste_picker(),
            _ => harness.app.shortcut_help_open = true,
        }
        run_discovery_frame_once(&mut harness.app, &ctx, Vec::new());
        drain_cmds(&harness);

        press_paste_into_editor(&mut harness, &ctx, "clip");

        assert_eq!(harness.app.active_snapshot(), "original", "{overlay}");
        assert!(harness.app.pending_editor_paste.is_none(), "{overlay}");
        assert!(
            !drain_cmds(&harness)
                .iter()
                .any(|cmd| matches!(cmd, CoreCmd::CreatePaste { .. })),
            "{overlay}"
        );
    }
}

/// Delivers a native batch and drains discovery slices without changing its modifiers.
fn native_batch(app: &mut LocalPasteApp, ctx: &egui::Context, events: Vec<egui::Event>) {
    let passes = events.len() + app.deferred_discovery_events.len() + 2;
    run_full_update(app, ctx, events);
    for _ in 0..passes {
        if app.deferred_discovery_events.is_empty() {
            return;
        }
        run_full_update(app, ctx, vec![]);
    }
    assert!(app.deferred_discovery_events.is_empty());
}

#[test]
fn native_shift_paste_keeps_pressed_modifiers_after_same_batch_release() {
    for platform in [
        virtual_editor::PlatformFlavor::Mac,
        virtual_editor::PlatformFlavor::Other,
    ] {
        with_platform(platform, || {
            for owner in ["editor", "title", "search", "unfocused"] {
                let mut harness = make_app();
                let ctx = egui::Context::default();
                harness.app.reset_virtual_editor("café 🦀 original");
                harness.app.focus_editor_next = true;
                native_batch(&mut harness.app, &ctx, vec![]);
                harness
                    .app
                    .virtual_editor_state
                    .restore_selection(4, None, 15);
                if owner != "editor" {
                    harness.app.virtual_editor_state.has_focus = false;
                    ctx.memory_mut(|memory| {
                        memory.surrender_focus(egui::Id::new(VIRTUAL_EDITOR_ID));
                        if owner == "title" {
                            memory.request_focus(egui::Id::new(TITLE_INPUT_ID));
                        } else if owner == "search" {
                            memory.request_focus(egui::Id::new(SEARCH_INPUT_ID));
                        }
                    });
                }
                let name = harness.app.edit_name.clone();
                let tags = harness.app.edit_tags.clone();
                drain_cmds(&harness);
                native_batch(
                    &mut harness.app,
                    &ctx,
                    vec![
                        egui::Event::ModifiersChanged(command_shift()),
                        egui::Event::Paste("日本".into()),
                        egui::Event::ModifiersChanged(egui::Modifiers::NONE),
                    ],
                );
                let expected = if owner == "editor" {
                    "café日本 🦀 original"
                } else {
                    "café 🦀 original\n日本"
                };
                assert_eq!(
                    harness.app.active_snapshot(),
                    expected,
                    "{platform:?} {owner}"
                );
                assert_eq!(harness.app.edit_name, name);
                assert_eq!(harness.app.edit_tags, tags);
                assert_eq!(harness.app.search_query, "");
                assert!(!drain_cmds(&harness)
                    .iter()
                    .any(|cmd| matches!(cmd, CoreCmd::CreatePaste { .. })));
            }
        });
    }
}

#[test]
fn native_modifier_state_survives_batches_and_resets_before_plain_metadata_paste() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    harness.app.reset_virtual_editor("body");
    native_batch(
        &mut harness.app,
        &ctx,
        vec![egui::Event::ModifiersChanged(command_shift())],
    );
    native_batch(
        &mut harness.app,
        &ctx,
        vec![
            egui::Event::Paste("one".into()),
            egui::Event::Paste("two".into()),
            egui::Event::ModifiersChanged(egui::Modifiers::NONE),
        ],
    );
    assert_eq!(harness.app.active_snapshot(), "body\nonetwo");
    harness.app.virtual_editor_state.has_focus = false;
    ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(TITLE_INPUT_ID)));
    let name = harness.app.edit_name.clone();
    native_batch(
        &mut harness.app,
        &ctx,
        vec![
            egui::Event::ModifiersChanged(primary_command_modifiers()),
            egui::Event::Paste("title".into()),
            egui::Event::ModifiersChanged(egui::Modifiers::NONE),
        ],
    );
    assert_eq!(harness.app.active_snapshot(), "body\nonetwo");
    assert_eq!(harness.app.edit_name, format!("{name}title"));
}

#[test]
fn native_paste_marker_keeps_chord_across_each_discovery_dismissal() {
    for overlay in ["palette", "picker", "help"] {
        let mut harness = make_app();
        let ctx = egui::Context::default();
        harness.app.reset_virtual_editor("body");
        native_batch(&mut harness.app, &ctx, vec![]);
        match overlay {
            "palette" => harness.app.command_palette_open = true,
            "picker" => harness.app.paste_picker_open = true,
            _ => harness.app.shortcut_help_open = true,
        }
        harness.app.focus_discovery_input(&ctx);
        native_batch(&mut harness.app, &ctx, vec![]);
        native_batch(
            &mut harness.app,
            &ctx,
            vec![
                key_event(egui::Key::Escape, egui::Modifiers::NONE),
                egui::Event::ModifiersChanged(command_shift()),
                egui::Event::Paste("clip".into()),
                egui::Event::ModifiersChanged(egui::Modifiers::NONE),
            ],
        );
        assert_eq!(harness.app.active_snapshot(), "body\nclip", "{overlay}");
        assert_eq!(harness.app.command_palette_query, "");
        assert_eq!(harness.app.paste_picker_query, "");
        assert_eq!(harness.app.shortcut_help_query, "");
    }
}

#[test]
fn mixed_native_paste_chords_in_one_batch_keep_separate_owners() {
    for shifted_first in [false, true] {
        let mut harness = make_app();
        let ctx = egui::Context::default();
        harness.app.reset_virtual_editor("body");
        native_batch(&mut harness.app, &ctx, vec![]);
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(TITLE_INPUT_ID)));
        let name = harness.app.edit_name.clone();
        let shift = command_shift();
        let plain = primary_command_modifiers();
        native_batch(
            &mut harness.app,
            &ctx,
            vec![
                egui::Event::ModifiersChanged(if shifted_first { shift } else { plain }),
                egui::Event::Paste("first".into()),
                egui::Event::ModifiersChanged(if shifted_first { plain } else { shift }),
                egui::Event::Paste("second".into()),
                egui::Event::ModifiersChanged(egui::Modifiers::NONE),
            ],
        );
        if shifted_first {
            // The append focuses the editor, so the following plain paste follows it.
            assert_eq!(harness.app.active_snapshot(), "body\nfirstsecond");
            assert_eq!(harness.app.edit_name, name);
        } else {
            assert_eq!(harness.app.active_snapshot(), "body\nsecond");
            assert_eq!(harness.app.edit_name, format!("{name}first"));
        }
        assert_eq!(harness.app.edit_tags, "");
        assert!(!drain_cmds(&harness)
            .iter()
            .any(|cmd| matches!(cmd, CoreCmd::CreatePaste { .. })));
    }
}
