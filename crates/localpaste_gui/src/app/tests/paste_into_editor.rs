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
fn command_shift_v_inserts_at_retained_caret_from_any_focus() {
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
                assert!(ctx.wants_keyboard_input());
                assert!(!harness.app.virtual_editor_state.has_focus);
            }
            _ => assert!(harness.app.virtual_editor_state.has_focus),
        }
        drain_cmds(&harness);

        press_paste_into_editor(&mut harness, &ctx, ",");

        assert_eq!(harness.app.active_snapshot(), "hello, world", "{focus}");
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
