//! Regression coverage for mutually exclusive version-overlay ownership.

use super::*;
use crossbeam_channel::TryRecvError;

const REENTRY_STATUS: &str = "Close the open version window before opening another one.";
const CREATE_DEFERRED_STATUS: &str =
    "Created new paste; current selection stays pinned until the version workflow finishes.";
const RESET_STATUS: &str = "Reset in progress; editor is temporarily read-only.";

#[test]
fn history_modal_rejects_opening_diff_modal_while_it_owns_version_workflow() {
    let mut harness = make_app();
    harness.app.version_ui.history_modal_open = true;
    harness.app.version_ui.history_selected_index = 1;

    harness.app.open_diff_modal();

    assert!(harness.app.version_ui.history_modal_open);
    assert_eq!(harness.app.version_ui.history_selected_index, 1);
    assert!(
        !harness.app.version_ui.diff_modal_open,
        "history should remain the sole active version overlay"
    );
    assert_eq!(
        harness
            .app
            .status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some(REENTRY_STATUS)
    );
    assert!(matches!(
        harness.cmd_rx.try_recv(),
        Err(TryRecvError::Empty)
    ));
}

#[test]
fn version_overlays_block_virtual_editor_fallback_shortcuts() {
    enum OverlayCase {
        History,
        Diff,
    }

    enum ShortcutCase {
        Cut,
        Undo,
    }

    for overlay in [OverlayCase::History, OverlayCase::Diff] {
        for shortcut in [ShortcutCase::Cut, ShortcutCase::Undo] {
            let mut harness = make_app();
            let ctx = egui::Context::default();
            let editor_id = egui::Id::new(VIRTUAL_EDITOR_ID);
            harness.app.reset_virtual_editor("abcdef");
            ctx.memory_mut(|m| m.request_focus(editor_id));

            let before_text = match shortcut {
                ShortcutCase::Cut => {
                    let len = harness.app.virtual_editor_buffer.len_chars();
                    harness.app.virtual_editor_state.set_cursor(1, len);
                    harness.app.virtual_editor_state.move_cursor(4, len, true);
                    "abcdef".to_string()
                }
                ShortcutCase::Undo => {
                    let len = harness.app.virtual_editor_buffer.len_chars();
                    harness.app.virtual_editor_state.set_cursor(len, len);
                    let applied = harness.app.apply_virtual_commands(
                        &ctx,
                        &[VirtualInputCommand::InsertText("!".to_string())],
                    );
                    assert!(applied.changed);
                    "abcdef!".to_string()
                }
            };

            match overlay {
                OverlayCase::History => harness.app.version_ui.history_modal_open = true,
                OverlayCase::Diff => harness.app.version_ui.diff_modal_open = true,
            }

            let event = match shortcut {
                ShortcutCase::Cut => command_key_event(egui::Key::X),
                ShortcutCase::Undo => command_key_event(egui::Key::Z),
            };
            run_full_update(&mut harness.app, &ctx, vec![event]);

            assert_eq!(
                harness.app.virtual_editor_buffer.to_string(),
                before_text,
                "open version overlays must fence fallback editor shortcuts"
            );
        }
    }
}

#[test]
fn closing_version_overlays_reconciles_hidden_selection_back_to_visible_projection() {
    enum OverlayCase {
        History,
        Diff,
    }

    for overlay in [OverlayCase::History, OverlayCase::Diff] {
        let mut harness = make_app();
        harness.app.pastes = vec![test_summary("beta", "Beta", None, 4)];

        match overlay {
            OverlayCase::History => {
                harness.app.version_ui.history_modal_open = true;
                harness.app.close_history_modal();
            }
            OverlayCase::Diff => {
                harness.app.version_ui.diff_modal_open = true;
                harness.app.close_diff_modal();
            }
        }

        assert_eq!(
            harness.app.selected_id.as_deref(),
            Some("beta"),
            "closing a detached version workflow should restore a visible main-view selection"
        );
        match recv_cmd(&harness.cmd_rx) {
            CoreCmd::GetPaste { id, .. } => assert_eq!(id, "beta"),
            other => panic!("expected GetPaste command, got {:?}", other),
        }
    }
}

#[test]
fn diff_modal_rejects_opening_history_modal_while_it_owns_version_workflow() {
    let mut harness = make_app();
    harness.app.version_ui.diff_modal_open = true;
    harness.app.version_ui.diff_query = "beta".to_string();
    harness.app.version_ui.diff_target_id = Some("beta".to_string());

    harness.app.open_history_modal();

    assert!(harness.app.version_ui.diff_modal_open);
    assert_eq!(harness.app.version_ui.diff_query, "beta");
    assert_eq!(
        harness.app.version_ui.diff_target_id.as_deref(),
        Some("beta")
    );
    assert!(
        !harness.app.version_ui.history_modal_open,
        "diff should remain the sole active version overlay"
    );
    assert_eq!(
        harness
            .app
            .status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some(REENTRY_STATUS)
    );
    assert!(matches!(
        harness.cmd_rx.try_recv(),
        Err(TryRecvError::Empty)
    ));
}

#[test]
fn history_reset_confirm_rejects_opening_diff_modal() {
    let mut harness = make_app();
    harness.app.version_ui.history_modal_open = true;
    harness.app.version_ui.history_reset_confirm_open = true;
    harness.app.version_ui.history_reset_confirm_target = Some(42);

    harness.app.open_diff_modal();

    assert!(harness.app.version_ui.history_modal_open);
    assert!(harness.app.version_ui.history_reset_confirm_open);
    assert_eq!(
        harness.app.version_ui.history_reset_confirm_target,
        Some(42)
    );
    assert!(
        !harness.app.version_ui.diff_modal_open,
        "history reset confirm should keep exclusive modal ownership"
    );
    assert_eq!(
        harness
            .app
            .status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some(REENTRY_STATUS)
    );
    assert!(matches!(
        harness.cmd_rx.try_recv(),
        Err(TryRecvError::Empty)
    ));
}

#[test]
fn paste_created_during_version_overlay_defers_selection_until_overlay_closes() {
    let mut harness = make_app();
    harness.app.version_ui.history_modal_open = true;
    harness.app.version_ui.history_selected_index = 1;
    let initial_content = harness.app.active_snapshot();

    let mut created = Paste::new("new-content".to_string(), "new-note".to_string());
    created.id = "new-id".to_string();
    harness
        .app
        .apply_event(CoreEvent::PasteCreated { paste: created });

    assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));
    assert!(harness.app.version_ui.history_modal_open);
    assert_eq!(harness.app.pending_selection_id.as_deref(), Some("new-id"));
    assert_eq!(harness.app.active_snapshot(), initial_content);
    assert_eq!(
        harness
            .app
            .status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some(CREATE_DEFERRED_STATUS)
    );
    assert!(matches!(
        harness.cmd_rx.try_recv(),
        Err(TryRecvError::Empty)
    ));

    harness.app.close_history_modal();

    assert_eq!(harness.app.selected_id.as_deref(), Some("new-id"));
    assert!(harness.app.pending_selection_id.is_none());
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::GetPaste { id, .. } => assert_eq!(id, "new-id"),
        other => panic!("expected GetPaste command, got {:?}", other),
    }
}

#[test]
fn reset_in_flight_rejects_opening_version_overlays_after_history_closes() {
    let mut harness = make_app();
    harness.app.version_ui.history_modal_open = true;
    harness.app.version_ui.history_reset_in_flight_paste_id = Some("alpha".to_string());

    harness.app.close_history_modal();
    assert!(harness
        .app
        .version_ui
        .history_reset_in_flight_paste_id
        .is_some());

    harness.app.open_diff_modal();
    assert!(!harness.app.version_ui.diff_modal_open);
    assert_eq!(
        harness
            .app
            .status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some(RESET_STATUS)
    );

    harness.app.open_history_modal();
    assert!(!harness.app.version_ui.history_modal_open);
    assert_eq!(
        harness
            .app
            .status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some(RESET_STATUS)
    );
    assert!(matches!(
        harness.cmd_rx.try_recv(),
        Err(TryRecvError::Empty)
    ));
}

#[test]
fn toolbar_version_opens_close_discovery_and_allow_save_with_blocked_mutation_feedback() {
    for history in [false, true] {
        for discovery in 0..3 {
            let (mut harness, _event_tx) = make_app_with_event_tx();
            let ctx = egui::Context::default();
            harness.app.paste_picker_open = discovery == 0;
            harness.app.command_palette_open = discovery == 1;
            harness.app.shortcut_help_open = discovery == 2;
            if history {
                harness.app.open_history_modal();
            } else {
                harness.app.open_diff_modal();
            }
            assert!(
                !harness.app.paste_picker_open
                    && !harness.app.command_palette_open
                    && !harness.app.shortcut_help_open
            );
            harness.cmd_rx.try_iter().for_each(drop);
            set_active_content(&mut harness.app, "dirty before version window");
            harness.app.mark_dirty();
            run_full_update(
                &mut harness.app,
                &ctx,
                vec![command_key_event(egui::Key::S)],
            );
            assert!(harness
                .cmd_rx
                .try_iter()
                .any(|cmd| matches!(cmd, CoreCmd::UpdatePasteVirtual { .. })));
            for key in [egui::Key::N, egui::Key::Delete] {
                harness.app.status = None;
                if let Some(id) = ctx.memory(|memory| memory.focused()) {
                    ctx.memory_mut(|memory| memory.surrender_focus(id));
                }
                run_full_update(&mut harness.app, &ctx, vec![command_key_event(key)]);
                assert!(harness
                    .app
                    .status
                    .as_ref()
                    .unwrap()
                    .text
                    .contains("version"));
                assert!(!harness.cmd_rx.try_iter().any(|cmd| matches!(
                    cmd,
                    CoreCmd::CreatePaste { .. } | CoreCmd::DeletePaste { .. }
                )));
            }
            run_full_update(
                &mut harness.app,
                &ctx,
                vec![key_event(egui::Key::Escape, egui::Modifiers::NONE)],
            );
            assert!(!harness.app.version_overlay_open());
        }
    }
}

#[test]
fn floating_query_focus_waits_until_its_accessibility_node_can_render() {
    for query_id in [
        DIFF_QUERY_INPUT_ID,
        COMMAND_PALETTE_INPUT_ID,
        PASTE_PICKER_INPUT_ID,
        "shortcut_help_query",
    ] {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let id = egui::Id::new(query_id);
        let mut requested = true;
        let mut query = String::new();
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.scope_builder(egui::UiBuilder::new().sizing_pass().invisible(), |ui| {
                    if requested && super::super::ui::focus_visible_query(ui, id) {
                        requested = false;
                    }
                    ui.add(egui::TextEdit::singleline(&mut query).id(id));
                });
            });
        });
        assert!(
            requested,
            "invisible sizing must retain the one-shot request"
        );
        assert!(!ctx.memory(|memory| memory.has_focus(id)));
        assert_accessible_focus(&output);
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                if requested && super::super::ui::focus_visible_query(ui, id) {
                    requested = false;
                }
                ui.add(egui::TextEdit::singleline(&mut query).id(id));
            });
        });
        assert!(!requested);
        assert!(ctx.memory(|memory| memory.has_focus(id)));
        assert_accessible_focus(&output);
    }
}

#[test]
fn diff_open_focuses_query_once_for_toolbar_and_palette() {
    for palette in [false, true] {
        let (mut harness, _event_tx) = make_app_with_event_tx();
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        harness.app.focus_editor_next = true;
        let output = run_full_update_with_input(
            &mut harness.app,
            &ctx,
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 900.0),
                )),
                ..Default::default()
            },
        );
        assert_accessible_focus(&output);
        if palette {
            harness.app.command_palette_open = true;
            harness.app.command_palette_query = "open diff".into();
            run_full_update(&mut harness.app, &ctx, vec![]);
            run_full_update(
                &mut harness.app,
                &ctx,
                vec![key_event(egui::Key::Enter, egui::Modifiers::NONE)],
            );
        } else {
            let target = output
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some("Diff"))
                .unwrap()
                .0;
            assert_accessible_focus(&run_full_update_with_input(
                &mut harness.app,
                &ctx,
                egui::RawInput {
                    events: vec![egui::Event::AccessKitActionRequest(
                        egui::accesskit::ActionRequest {
                            action: egui::accesskit::Action::Click,
                            target,
                            data: None,
                        },
                    )],
                    ..Default::default()
                },
            ));
        }
        for _ in 0..3 {
            assert_accessible_focus(&run_full_update_with_input(
                &mut harness.app,
                &ctx,
                egui::RawInput::default(),
            ));
        }
        assert!(ctx.memory(|memory| memory.has_focus(egui::Id::new(DIFF_QUERY_INPUT_ID))));
        run_full_update(
            &mut harness.app,
            &ctx,
            vec![egui::Event::Text("beta".into())],
        );
        assert_eq!(harness.app.version_ui.diff_query, "beta");
        assert_eq!(harness.app.active_snapshot(), "content");
        ctx.memory_mut(|memory| memory.surrender_focus(egui::Id::new(DIFF_QUERY_INPUT_ID)));
        run_full_update(&mut harness.app, &ctx, vec![]);
        assert!(!ctx.memory(|memory| memory.has_focus(egui::Id::new(DIFF_QUERY_INPUT_ID))));
    }
}

/// Assert that each accessibility update's focus is reachable from its published root.
///
/// # Panics
/// Panics if output omits the tree or points focus at a missing/hidden widget.
fn assert_accessible_focus(output: &egui::FullOutput) {
    let update = output.platform_output.accesskit_update.as_ref().unwrap();
    let mut pending = vec![update.tree.as_ref().unwrap().root];
    let mut visited = HashSet::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        let node = update
            .nodes
            .iter()
            .find(|(node_id, _)| *node_id == id)
            .unwrap_or_else(|| panic!("missing accessible node {id:?}"));
        pending.extend(node.1.children());
    }
    assert!(
        visited.contains(&update.focus),
        "unpublished focus {:?}",
        update.focus
    );
}
