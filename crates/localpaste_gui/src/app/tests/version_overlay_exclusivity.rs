//! Regression coverage for mutually exclusive version-overlay ownership.

use super::*;
use crossbeam_channel::TryRecvError;

const REENTRY_STATUS: &str = "Close the open version window before opening another one.";

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
            harness.app.editor_mode = EditorMode::VirtualEditor;
            harness.app.reset_virtual_editor("abcdef");
            harness.app.virtual_editor_active = true;
            harness.app.virtual_editor_state.has_focus = true;

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
            CoreCmd::GetPaste { id } => assert_eq!(id, "beta"),
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
