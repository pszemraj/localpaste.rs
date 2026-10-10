//! Rendered discovery-overlay keyboard ownership and recovery regressions.

use super::run_discovery_frame_once as frame;
use super::*;

fn linux_command(shift: bool) -> egui::Modifiers {
    egui::Modifiers {
        ctrl: true,
        command: true,
        shift,
        ..Default::default()
    }
}

#[test]
fn linux_discovery_chords_preserve_each_focused_text_field() {
    for input_id in [
        SEARCH_INPUT_ID,
        TITLE_INPUT_ID,
        PROPERTIES_NAME_INPUT_ID,
        PROPERTIES_TAGS_INPUT_ID,
        EDITOR_FIND_INPUT_ID,
    ] {
        for shift in [false, true] {
            let (mut harness, _events) = make_app_with_event_tx();
            let ctx = egui::Context::default();
            harness.app.properties_drawer_open = true;
            harness.app.edit_name = "abcdefgh".into();
            harness.app.edit_tags = "abcdefgh".into();
            harness.app.set_search_query("abcdefgh".into());
            harness.app.open_editor_find();
            harness.app.set_editor_find_query("abcdefgh".into());
            frame(&mut harness.app, &ctx, vec![]);
            let id = egui::Id::new(input_id);
            ctx.memory_mut(|memory| memory.request_focus(id));
            frame(&mut harness.app, &ctx, vec![]);
            let mut state =
                egui::text_edit::TextEditState::load(&ctx, id).expect("rendered text input");
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::one(
                    egui::text::CCursor::new(3),
                )));
            state.store(&ctx, id);
            assert!(ctx.memory(|memory| memory.has_focus(id)), "{input_id}");
            harness.app.metadata_dirty = false;
            frame(
                &mut harness.app,
                &ctx,
                vec![key_event(egui::Key::K, linux_command(shift))],
            );
            assert_eq!(
                harness.app.edit_name, "abcdefgh",
                "{input_id}, shift={shift}"
            );
            assert_eq!(
                harness.app.edit_tags, "abcdefgh",
                "{input_id}, shift={shift}"
            );
            assert_eq!(
                harness.app.search_query, "abcdefgh",
                "{input_id}, shift={shift}"
            );
            assert_eq!(
                harness.app.editor_find.query, "abcdefgh",
                "{input_id}, shift={shift}"
            );
            assert!(!harness.app.metadata_dirty);
            assert_eq!(harness.app.paste_picker_open, shift);
            assert_eq!(harness.app.command_palette_open, !shift);
        }
    }
}

#[test]
fn canceling_discovery_returns_focus_and_following_paste_to_editor() {
    for (shift, cancel_by_toggle, switch_to_help) in [
        (false, false, false),
        (true, false, false),
        (false, true, false),
        (true, true, false),
        (true, false, true),
    ] {
        let (mut harness, _events) = make_app_with_event_tx();
        let ctx = egui::Context::default();
        harness.app.focus_editor_next = true;
        frame(&mut harness.app, &ctx, vec![]);
        frame(
            &mut harness.app,
            &ctx,
            vec![key_event(egui::Key::K, linux_command(shift))],
        );
        if switch_to_help {
            frame(
                &mut harness.app,
                &ctx,
                vec![key_event(egui::Key::F1, egui::Modifiers::NONE)],
            );
        }
        let cancel = if cancel_by_toggle {
            key_event(egui::Key::K, linux_command(shift))
        } else {
            key_event(egui::Key::Escape, egui::Modifiers::NONE)
        };
        frame(&mut harness.app, &ctx, vec![cancel]);
        assert!(!harness.app.keyboard_overlay_open());
        assert!(ctx.memory(|memory| memory.has_focus(egui::Id::new(VIRTUAL_EDITOR_ID))));
        run_discovery_frame_with_modifiers(
            &mut harness.app,
            &ctx,
            vec![egui::Event::Paste("inserted".into())],
            linux_command(false),
        );
        assert!(harness.app.active_snapshot().contains("inserted"));
        assert!(!harness
            .cmd_rx
            .try_iter()
            .any(|cmd| matches!(cmd, CoreCmd::CreatePaste { .. })));
    }
}

#[test]
fn discovery_cancellation_returns_focus_to_metadata_input() {
    for shift in [false, true] {
        let (mut harness, _events) = make_app_with_event_tx();
        let ctx = egui::Context::default();
        frame(&mut harness.app, &ctx, vec![]);
        let title_id = egui::Id::new(TITLE_INPUT_ID);
        ctx.memory_mut(|memory| memory.request_focus(title_id));
        frame(&mut harness.app, &ctx, vec![]);
        frame(
            &mut harness.app,
            &ctx,
            vec![key_event(egui::Key::K, linux_command(shift))],
        );
        frame(
            &mut harness.app,
            &ctx,
            vec![key_event(egui::Key::Escape, egui::Modifiers::NONE)],
        );
        assert!(ctx.memory(|memory| memory.has_focus(title_id)));
        let before = harness.app.edit_name.clone();
        frame(&mut harness.app, &ctx, vec![egui::Event::Text("x".into())]);
        assert_ne!(harness.app.edit_name, before);
        assert_eq!(harness.app.active_snapshot(), "content");
    }
}

#[test]
fn discovery_overlays_block_background_mutation_and_properties_chords() {
    for overlay in ["palette", "picker", "help"] {
        for (key, shift, immediate_payload) in [
            (egui::Key::N, false, false),
            (egui::Key::S, false, false),
            (egui::Key::Delete, false, false),
            (egui::Key::I, false, false),
            (egui::Key::V, true, false),
            (egui::Key::V, true, true),
        ] {
            let (mut harness, _events) = make_app_with_event_tx();
            let ctx = egui::Context::default();
            frame(&mut harness.app, &ctx, vec![]);
            match overlay {
                "palette" => {
                    frame(
                        &mut harness.app,
                        &ctx,
                        vec![key_event(egui::Key::K, linux_command(false))],
                    );
                }
                "picker" => {
                    frame(
                        &mut harness.app,
                        &ctx,
                        vec![key_event(egui::Key::K, linux_command(true))],
                    );
                }
                _ => harness.app.open_shortcut_help(&ctx),
            }
            frame(&mut harness.app, &ctx, vec![]);
            if let Some(id) = ctx.memory(|memory| memory.focused()) {
                ctx.memory_mut(|memory| memory.surrender_focus(id));
            }
            harness.app.virtual_editor_state.has_focus = false;
            if key == egui::Key::V {
                harness.app.mark_dirty();
            }
            let selected = harness.app.selected_id.clone();
            let properties_open = harness.app.properties_drawer_open;
            while harness.cmd_rx.try_recv().is_ok() {}
            let events = if immediate_payload {
                vec![egui::Event::Paste("clipboard payload".into())]
            } else {
                vec![key_event(key, linux_command(shift))]
            };
            let output = run_discovery_frame_with_modifiers(
                &mut harness.app,
                &ctx,
                events,
                linux_command(shift),
            );
            assert_eq!(harness.app.selected_id, selected, "{overlay}: {key:?}");
            assert_eq!(harness.app.properties_drawer_open, properties_open);
            assert_eq!(harness.app.active_snapshot(), "content");
            assert!(harness.cmd_rx.try_recv().is_err(), "{overlay}: {key:?}");
            if key == egui::Key::V {
                assert_eq!(harness.app.save_status, SaveStatus::Dirty);
            }
            assert_eq!(harness.app.paste_as_new_pending_frames, 0);
            assert!(harness.app.paste_as_new_clipboard_requested_at.is_none());
            assert!(!output.viewport_output.values().any(|viewport| viewport
                .commands
                .iter()
                .any(|command| matches!(command, egui::ViewportCommand::RequestPaste))));
            if key == egui::Key::V && !immediate_payload {
                frame(
                    &mut harness.app,
                    &ctx,
                    vec![egui::Event::Paste("delayed clipboard payload".into())],
                );
                assert_eq!(
                    harness.app.selected_id, selected,
                    "{overlay}: delayed paste"
                );
                assert_eq!(harness.app.active_snapshot(), "content");
                assert!(!harness.cmd_rx.try_iter().any(|command| matches!(
                    command,
                    CoreCmd::CreatePaste { .. } | CoreCmd::UpdatePasteVirtual { .. }
                )));
            }
        }
    }
}

#[test]
fn version_dialogs_refuse_palette_and_picker_chords() {
    for history in [false, true] {
        for shift in [false, true] {
            let (mut harness, _events) = make_app_with_event_tx();
            let ctx = egui::Context::default();
            harness.app.version_ui.history_modal_open = history;
            harness.app.version_ui.diff_modal_open = !history;
            frame(&mut harness.app, &ctx, vec![]);
            frame(
                &mut harness.app,
                &ctx,
                vec![key_event(egui::Key::K, linux_command(shift))],
            );
            assert!(!harness.app.command_palette_open);
            assert!(!harness.app.paste_picker_open);
            assert!(harness.app.version_overlay_open());
        }
    }
}

#[test]
fn palette_actions_keep_their_focus_destination_and_picker_cancel_returns_to_opener() {
    for (query, destination) in [
        ("Find in paste", EDITOR_FIND_INPUT_ID),
        ("Focus sidebar search", SEARCH_INPUT_ID),
        ("Open paste picker", PASTE_PICKER_INPUT_ID),
    ] {
        let (mut harness, _events) = make_app_with_event_tx();
        let ctx = egui::Context::default();
        harness.app.focus_editor_next = true;
        frame(&mut harness.app, &ctx, vec![]);
        frame(
            &mut harness.app,
            &ctx,
            vec![key_event(egui::Key::K, linux_command(false))],
        );
        harness.app.command_palette_query = query.into();
        frame(
            &mut harness.app,
            &ctx,
            vec![key_event(egui::Key::Enter, egui::Modifiers::NONE)],
        );
        frame(&mut harness.app, &ctx, vec![]);
        assert!(
            ctx.memory(|memory| memory.has_focus(egui::Id::new(destination))),
            "{query}"
        );
        if destination == PASTE_PICKER_INPUT_ID {
            frame(
                &mut harness.app,
                &ctx,
                vec![key_event(egui::Key::Escape, egui::Modifiers::NONE)],
            );
            assert!(ctx.memory(|memory| memory.has_focus(egui::Id::new(VIRTUAL_EDITOR_ID))));
        } else {
            assert!(harness.app.discovery_return_focus.is_none());
        }
    }
}

#[test]
fn failed_picker_search_stays_visible_until_explicit_retry() {
    let (mut harness, _events) = make_app_with_event_tx();
    let ctx = egui::Context::default();
    harness.app.open_paste_picker();
    harness.app.set_paste_picker_query("needle".into());
    harness.app.palette_search_last_input_at = Some(Instant::now() - SEARCH_DEBOUNCE);
    harness.app.maybe_dispatch_palette_search();
    assert!(matches!(
        recv_cmd(&harness.cmd_rx),
        CoreCmd::SearchPalette { .. }
    ));
    harness.app.apply_event(CoreEvent::PaletteSearchFailed {
        query: "needle".into(),
        scope: SearchScope::All,
        message: "Search failed: disk unavailable".into(),
    });
    frame(&mut harness.app, &ctx, vec![]);
    let output = frame(&mut harness.app, &ctx, vec![]);
    let texts: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
            _ => None,
        })
        .collect();
    assert!(texts
        .iter()
        .any(|text| text.contains("Search failed: disk unavailable")));
    assert!(texts.iter().any(|text| text.contains("Retry")));
    assert!(!texts.iter().any(|text| text.contains("No matching pastes")));
    assert!(
        harness.cmd_rx.try_recv().is_err(),
        "failure must wait for an explicit retry"
    );
    assert!(harness.app.palette_search_last_input_at.is_none());
    let retry = rendered_label_center(&output, "Retry");
    for pressed in [true, false] {
        frame(
            &mut harness.app,
            &ctx,
            primary_pointer_events(retry, pressed),
        );
    }
    harness.app.maybe_dispatch_palette_search();
    assert!(
        matches!(recv_cmd(&harness.cmd_rx), CoreCmd::SearchPalette { query, .. } if query == "needle")
    );
    harness.app.apply_event(CoreEvent::PaletteSearchResults {
        query: "needle".into(),
        scope: SearchScope::All,
        items: vec![test_summary("found", "Found", None, 1)],
    });
    assert!(harness.app.palette_search_error.is_none());
    assert_eq!(harness.app.palette_search_results[0].id, "found");
}

#[test]
fn sidebar_arrow_target_is_empty_list_safe() {
    let mut harness = make_app();
    harness.app.selected_id = Some("alpha".to_string());
    harness.app.pastes.clear();

    assert_eq!(harness.app.sidebar_arrow_target_id(1), None);
    assert_eq!(harness.app.sidebar_arrow_target_id(-1), None);

    harness.app.pastes = vec![
        test_summary("alpha", "Alpha", None, 1),
        test_summary("beta", "Beta", None, 1),
    ];
    assert_eq!(
        harness.app.sidebar_arrow_target_id(1),
        Some("beta".to_string())
    );
    for selected in [None, Some("hidden".to_string())] {
        harness.app.selected_id = selected;
        for direction in [-1, 1] {
            assert_eq!(
                harness.app.sidebar_arrow_target_id(direction),
                Some("alpha".into())
            );
        }
    }
    harness.app.selected_id = Some("beta".into());
    assert_eq!(harness.app.sidebar_arrow_target_id(1), None);
    assert_eq!(
        harness.app.sidebar_arrow_target_id(-1),
        Some("alpha".into())
    );
}

#[test]
fn delete_shortcut_guard_preserves_editor_delete_ownership_and_global_unfocused_behavior() {
    struct Case {
        name: &'static str,
        wants_keyboard_input: bool,
        virtual_editor_focus_active: bool,
        expected: bool,
    }

    let cases = [
        Case {
            name: "text input owns keyboard",
            wants_keyboard_input: true,
            virtual_editor_focus_active: false,
            expected: false,
        },
        Case {
            name: "virtual editor focused",
            wants_keyboard_input: false,
            virtual_editor_focus_active: true,
            expected: false,
        },
        Case {
            name: "non editor context",
            wants_keyboard_input: false,
            virtual_editor_focus_active: false,
            expected: true,
        },
    ];

    for case in cases {
        let harness = make_app();
        let focus_state = LocalPasteApp::keyboard_focus_state(
            case.virtual_editor_focus_active,
            case.wants_keyboard_input,
        );
        let actual = harness
            .app
            .should_route_delete_selected_shortcut(focus_state);
        assert_eq!(actual, case.expected, "case '{}'", case.name);
    }
    for focused in [false, true] {
        let (mut harness, _event_tx) = make_app_with_event_tx();
        let ctx = egui::Context::default();
        harness.app.version_ui.history_reset_in_flight_paste_id = Some("alpha".into());
        run_full_update(&mut harness.app, &ctx, vec![]);
        if focused {
            ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(SEARCH_INPUT_ID)));
            run_full_update(&mut harness.app, &ctx, vec![]);
        }
        harness.app.status = None;
        run_full_update(
            &mut harness.app,
            &ctx,
            vec![command_key_event(egui::Key::Delete)],
        );
        assert_eq!(harness.app.status.is_some(), !focused);
        assert!(!harness
            .cmd_rx
            .try_iter()
            .any(|cmd| matches!(cmd, CoreCmd::DeletePaste { .. })));
    }
}

#[test]
fn canceled_clipboard_reply_after_discovery_dismissal_does_not_edit_current_paste() {
    let (mut harness, _event_tx) = make_app_with_event_tx();
    let ctx = egui::Context::default();
    harness.app.reset_virtual_editor("original");
    harness.app.focus_editor_next = true;
    run_discovery_frame_once(&mut harness.app, &ctx, vec![]);
    harness.app.request_paste_as_new(&ctx);
    run_discovery_frame_once(
        &mut harness.app,
        &ctx,
        vec![key_event(egui::Key::F1, egui::Modifiers::NONE)],
    );
    assert_eq!(harness.app.paste_as_new_pending_frames, 0);
    run_discovery_frame_once(
        &mut harness.app,
        &ctx,
        vec![key_event(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    run_discovery_frame_once(
        &mut harness.app,
        &ctx,
        vec![egui::Event::Paste("late clipboard".into())],
    );
    assert_eq!(harness.app.active_snapshot(), "original");
    assert!(!harness
        .cmd_rx
        .try_iter()
        .any(|cmd| matches!(cmd, CoreCmd::CreatePaste { .. })));
    // Once the outstanding reply is discarded, independent native paste works.
    run_discovery_frame_once(
        &mut harness.app,
        &ctx,
        vec![egui::Event::Paste("fresh".into())],
    );
    assert_eq!(harness.app.active_snapshot(), "freshoriginal");
}

#[test]
fn native_paste_after_same_frame_discovery_dismissal_uses_returned_focus() {
    for explicit in [false, true] {
        let (mut harness, _events) = make_app_with_event_tx();
        let ctx = egui::Context::default();
        harness.app.focus_editor_next = true;
        frame(&mut harness.app, &ctx, vec![]);
        harness.app.open_shortcut_help(&ctx);
        frame(&mut harness.app, &ctx, vec![]);
        run_discovery_frame_with_modifiers(
            &mut harness.app,
            &ctx,
            vec![
                key_event(egui::Key::Escape, egui::Modifiers::NONE),
                egui::Event::Paste("fresh".into()),
            ],
            linux_command(explicit),
        );
        assert!(!harness.app.keyboard_overlay_open());
        assert_eq!(harness.app.active_snapshot(), "freshcontent", "{explicit}");
        assert!(!harness
            .cmd_rx
            .try_iter()
            .any(|command| matches!(command, CoreCmd::CreatePaste { .. })));
    }
}

#[test]
fn deferred_native_paste_keeps_its_modifiers_after_release() {
    for explicit in [false, true] {
        let (mut harness, _events) = make_app_with_event_tx();
        let ctx = egui::Context::default();
        harness.app.focus_editor_next = true;
        frame(&mut harness.app, &ctx, vec![]);
        run_discovery_frame_with_modifiers(
            &mut harness.app,
            &ctx,
            vec![
                key_event(egui::Key::F1, egui::Modifiers::NONE),
                key_event(egui::Key::Escape, egui::Modifiers::NONE),
                egui::Event::Paste("fresh".into()),
            ],
            linux_command(explicit),
        );
        assert!(harness.app.shortcut_help_open);
        frame(&mut harness.app, &ctx, vec![]);
        assert!(!harness.app.keyboard_overlay_open());
        assert_eq!(harness.app.active_snapshot(), "freshcontent", "{explicit}");
        assert!(!harness
            .cmd_rx
            .try_iter()
            .any(|command| matches!(command, CoreCmd::CreatePaste { .. })));
    }
}

#[test]
fn newer_plain_paste_supersedes_canceled_request_with_same_or_later_payload() {
    for delayed in [false, true] {
        let (mut harness, _event_tx) = make_app_with_event_tx();
        let ctx = egui::Context::default();
        harness.app.reset_virtual_editor("original");
        harness.app.focus_editor_next = true;
        run_discovery_frame_once(&mut harness.app, &ctx, vec![]);
        harness.app.request_paste_as_new(&ctx);
        harness.app.cancel_paste_as_new_intent();
        let mut events = vec![command_key_event(egui::Key::V)];
        if !delayed {
            events.push(egui::Event::Paste("fresh".into()));
        }
        run_discovery_frame_once(&mut harness.app, &ctx, events);
        if delayed {
            run_discovery_frame_once(
                &mut harness.app,
                &ctx,
                vec![egui::Event::Paste("fresh".into())],
            );
        }
        assert_eq!(harness.app.active_snapshot(), "freshoriginal");
    }
}

#[test]
fn canceled_reply_preceding_new_paste_shortcut_is_discarded_in_event_order() {
    for explicit in [false, true] {
        for delayed in [false, true] {
            let (mut harness, _event_tx) = make_app_with_event_tx();
            let ctx = egui::Context::default();
            harness.app.reset_virtual_editor("original");
            harness.app.focus_editor_next = true;
            run_discovery_frame_once(&mut harness.app, &ctx, vec![]);
            harness.app.request_paste_as_new(&ctx);
            harness.app.cancel_paste_as_new_intent();
            let modifiers = primary_command_modifiers()
                | if explicit {
                    egui::Modifiers::SHIFT
                } else {
                    egui::Modifiers::NONE
                };
            let mut events = vec![
                egui::Event::Paste("stale payload".into()),
                key_event(egui::Key::V, modifiers),
            ];
            if !delayed {
                events.push(egui::Event::Paste("fresh".into()));
            }
            run_discovery_frame_once(&mut harness.app, &ctx, events);
            if delayed {
                run_discovery_frame_once(
                    &mut harness.app,
                    &ctx,
                    vec![egui::Event::Paste("fresh".into())],
                );
            }
            // Both chords target the focused editor; only the fresh payload lands.
            assert_eq!(
                harness.app.active_snapshot(),
                "freshoriginal",
                "shift={explicit} delayed={delayed}"
            );
            assert!(!harness
                .cmd_rx
                .try_iter()
                .any(|cmd| matches!(cmd, CoreCmd::CreatePaste { .. })));
        }
    }
}
