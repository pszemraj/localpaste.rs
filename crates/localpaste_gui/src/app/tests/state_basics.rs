//! State/event flow tests for basic app selection, status, and toast behavior.

use super::*;

fn assert_delete_send_failure_keeps_lock_and_status(
    delete_action: impl FnOnce(&mut crate::app::LocalPasteApp),
) {
    let TestHarness {
        _dir: _guard,
        mut app,
        cmd_rx,
    } = make_app();
    app.locks
        .acquire("alpha", &app.lock_owner_id)
        .expect("acquire alpha lock");
    drop(cmd_rx);

    delete_action(&mut app);

    assert!(app.locks.is_locked("alpha").expect("is_locked"));
    assert_eq!(
        app.status.as_ref().map(|status| status.text.as_str()),
        Some("Delete failed: backend unavailable.")
    );
}

#[test]
fn paste_missing_updates_selection_and_list_matrix() {
    enum MissingCase {
        Selected,
        NonSelected,
    }

    for case in [MissingCase::Selected, MissingCase::NonSelected] {
        let mut harness = make_app();
        match case {
            MissingCase::Selected => {
                harness.app.apply_event(CoreEvent::PasteMissing {
                    id: "alpha".to_string(),
                });

                assert!(harness.app.pastes.is_empty());
                assert!(harness.app.selected_id.is_none());
                assert!(harness.app.selected_paste.is_none());
                assert_eq!(harness.app.active_text_chars(), 0);
                assert!(harness.app.status.is_some());
            }
            MissingCase::NonSelected => {
                harness
                    .app
                    .pastes
                    .push(test_summary("beta", "Beta", None, 4));

                harness.app.apply_event(CoreEvent::PasteMissing {
                    id: "beta".to_string(),
                });

                assert_eq!(harness.app.pastes.len(), 1);
                assert_eq!(harness.app.pastes[0].id, "alpha");
                assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));
                assert!(harness.app.selected_paste.is_some());
            }
        }
    }
}

#[test]
fn paste_load_failed_updates_lock_and_selection_matrix() {
    enum LoadFailedCase {
        Selected,
        Stale,
    }

    for case in [LoadFailedCase::Selected, LoadFailedCase::Stale] {
        let mut harness = make_app();
        match case {
            LoadFailedCase::Selected => {
                harness
                    .app
                    .locks
                    .acquire("alpha", &harness.app.lock_owner_id)
                    .expect("acquire alpha lock");
                harness.app.pending_copy_action = Some(PaletteCopyAction::Raw("alpha".to_string()));

                harness.app.apply_event(CoreEvent::PasteLoadFailed {
                    id: "alpha".to_string(),
                    message: "Get failed: injected".to_string(),
                });

                assert!(
                    !harness.app.locks.is_locked("alpha").expect("is_locked"),
                    "selected paste lock should be released on load failure"
                );
                assert!(harness.app.selected_id.is_none());
                assert!(harness.app.selected_paste.is_none());
                assert!(harness.app.pending_copy_action.is_none());
                assert_eq!(
                    harness
                        .app
                        .status
                        .as_ref()
                        .map(|status| status.text.as_str()),
                    Some("Get failed: injected")
                );
            }
            LoadFailedCase::Stale => {
                harness.app.selected_id = Some("beta".to_string());
                harness.app.selected_paste =
                    Some(Paste::new("beta".to_string(), "Beta".to_string()));
                harness
                    .app
                    .locks
                    .acquire("beta", &harness.app.lock_owner_id)
                    .expect("acquire beta lock");
                harness.app.pending_copy_action = Some(PaletteCopyAction::Raw("alpha".to_string()));

                harness.app.apply_event(CoreEvent::PasteLoadFailed {
                    id: "alpha".to_string(),
                    message: "Get failed: stale".to_string(),
                });

                assert!(
                    harness.app.locks.is_locked("beta").expect("is_locked"),
                    "stale load failure should not unlock current selection"
                );
                assert_eq!(harness.app.selected_id.as_deref(), Some("beta"));
                assert!(harness.app.pending_copy_action.is_none());
                assert_eq!(
                    harness
                        .app
                        .status
                        .as_ref()
                        .map(|status| status.text.as_str()),
                    Some("Get failed: stale")
                );
            }
        }
    }
}

fn pressed_key(key: eframe::egui::Key) -> eframe::egui::Event {
    eframe::egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: eframe::egui::Modifiers::NONE,
    }
}

fn output_has_visible_true(output: &eframe::egui::FullOutput) -> bool {
    output.viewport_output.values().any(|viewport| {
        viewport
            .commands
            .iter()
            .any(|command| matches!(command, eframe::egui::ViewportCommand::Visible(true)))
    })
}

#[test]
fn shortcut_help_closes_on_escape() {
    let mut harness = make_app();
    harness.app.shortcut_help_open = true;
    let ctx = eframe::egui::Context::default();

    let _ = ctx.run(
        eframe::egui::RawInput {
            events: vec![pressed_key(eframe::egui::Key::Escape)],
            ..Default::default()
        },
        |ctx| {
            harness.app.render_shortcut_help(ctx);
        },
    );

    assert!(
        !harness.app.shortcut_help_open,
        "shortcut help should dismiss on Escape"
    );
}

#[test]
fn first_full_update_reveals_hidden_window_once() {
    let mut harness = make_app();
    let ctx = eframe::egui::Context::default();
    harness.app.ensure_style(&ctx);

    let mut frame = eframe::Frame::_new_kittest();
    let first_output = ctx.run(eframe::egui::RawInput::default(), |ctx| {
        eframe::App::update(&mut harness.app, ctx, &mut frame);
    });

    assert!(harness.app.window_shown_once);
    assert!(
        output_has_visible_true(&first_output),
        "first update should reveal the initially hidden native window"
    );

    let second_output = ctx.run(eframe::egui::RawInput::default(), |ctx| {
        eframe::App::update(&mut harness.app, ctx, &mut frame);
    });

    assert!(
        !output_has_visible_true(&second_output),
        "subsequent updates should not keep emitting viewport reveal commands"
    );
}

#[test]
fn history_modal_closes_on_escape() {
    let mut harness = make_app();
    harness.app.version_ui.history_modal_open = true;
    let ctx = eframe::egui::Context::default();

    let _ = ctx.run(
        eframe::egui::RawInput {
            events: vec![pressed_key(eframe::egui::Key::Escape)],
            ..Default::default()
        },
        |ctx| {
            harness.app.render_history_modal(ctx);
        },
    );

    assert!(
        !harness.app.version_ui.history_modal_open,
        "history modal should dismiss on Escape"
    );
}

#[test]
fn history_modal_headless_render_handles_long_inline_snapshot_band() {
    let mut harness = make_app();
    let long_snapshot = (0..180)
        .map(|idx| format!("line {idx}: long inline history preview"))
        .collect::<Vec<_>>()
        .join("\n");
    let now = chrono::Utc::now();
    harness.app.version_ui.history_modal_open = true;
    harness.app.version_ui.history_versions = vec![localpaste_core::models::paste::VersionMeta {
        version_id_ms: 42,
        created_at: now,
        content_hash: "history-hash".to_string(),
        len: long_snapshot.len(),
        language: Some("text".to_string()),
        language_is_manual: false,
    }];
    harness.app.version_ui.history_selected_index = 1;
    harness.app.version_ui.history_snapshot =
        Some(localpaste_core::models::paste::VersionSnapshot {
            paste_id: "alpha".to_string(),
            version_id_ms: 42,
            created_at: now,
            content_hash: "history-hash".to_string(),
            len: long_snapshot.len(),
            language: Some("text".to_string()),
            language_is_manual: false,
            content: long_snapshot.clone(),
        });

    let ctx = eframe::egui::Context::default();
    let output = ctx.run(
        eframe::egui::RawInput {
            screen_rect: Some(eframe::egui::Rect::from_min_size(
                eframe::egui::Pos2::ZERO,
                eframe::egui::Vec2::new(640.0, 360.0),
            )),
            ..Default::default()
        },
        |ctx| {
            harness.app.render_history_modal(ctx);
        },
    );

    assert!(harness.app.version_ui.history_modal_open);
    assert_eq!(harness.app.version_ui.history_preview_text, long_snapshot);
    assert!(
        !output.shapes.is_empty(),
        "headless modal render should produce paint output for the long inline snapshot"
    );
}

#[test]
fn delete_actions_keep_lock_until_delete_event_matrix() {
    enum DeleteAction {
        Selected,
        Palette,
    }

    for action in [DeleteAction::Selected, DeleteAction::Palette] {
        let mut harness = make_app();
        harness
            .app
            .locks
            .acquire("alpha", &harness.app.lock_owner_id)
            .expect("acquire alpha lock");
        assert!(harness.app.locks.is_locked("alpha").expect("is_locked"));

        match action {
            DeleteAction::Selected => harness.app.delete_selected(),
            DeleteAction::Palette => harness.app.send_palette_delete("alpha".to_string()),
        }
        assert!(harness.app.locks.is_locked("alpha").expect("is_locked"));

        match recv_cmd(&harness.cmd_rx) {
            CoreCmd::DeletePaste { id } => assert_eq!(id, "alpha"),
            other => panic!("expected delete command, got {:?}", other),
        }

        harness.app.apply_event(CoreEvent::PasteDeleted {
            id: "alpha".to_string(),
            undo_token: Some("undo-alpha".to_string()),
        });
        assert!(!harness.app.locks.is_locked("alpha").expect("is_locked"));
    }
}

#[test]
fn paste_deleted_clears_pending_copy_action_for_deleted_id() {
    let mut harness = make_app();
    harness.app.pending_copy_action = Some(PaletteCopyAction::Raw("alpha".to_string()));

    harness.app.apply_event(CoreEvent::PasteDeleted {
        id: "alpha".to_string(),
        undo_token: Some("undo-alpha".to_string()),
    });

    assert!(harness.app.pending_copy_action.is_none());
}

#[test]
fn paste_restored_selects_restored_paste_and_refreshes_list() {
    let mut harness = make_app();
    let mut restored = Paste::new("restored content".to_string(), "Restored".to_string());
    restored.id = "restored-id".to_string();

    harness.app.apply_event(CoreEvent::PasteRestored {
        paste: restored,
        undo_token: "undo-restored".to_string(),
    });

    assert_eq!(harness.app.selected_id.as_deref(), Some("restored-id"));
    assert_eq!(
        harness
            .app
            .selected_paste
            .as_ref()
            .map(|paste| paste.content.as_str()),
        Some("restored content")
    );
    assert!(harness
        .app
        .all_pastes
        .iter()
        .any(|paste| paste.id == "restored-id"));
    assert_eq!(
        harness
            .app
            .status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some("Restored deleted paste.")
    );
    match harness
        .cmd_rx
        .recv_timeout(Duration::from_millis(200))
        .expect("expected refresh command")
    {
        CoreCmd::ListPastes { .. } => {}
        other => panic!("expected ListPastes refresh, got {:?}", other),
    }
}

#[test]
fn paste_deleted_selects_visible_neighbor_matrix() {
    struct Case {
        visible_ids: &'static [&'static str],
        expected_selected_id: &'static str,
    }

    let cases = [
        Case {
            visible_ids: &["a", "b", "c"],
            expected_selected_id: "c",
        },
        Case {
            visible_ids: &["a", "b"],
            expected_selected_id: "a",
        },
    ];

    for case in cases {
        let mut harness = make_app();
        harness.app.all_pastes = case
            .visible_ids
            .iter()
            .map(|id| test_summary(id, &id.to_ascii_uppercase(), None, 1))
            .collect();
        harness.app.pastes = harness.app.all_pastes.clone();
        harness.app.selected_id = Some("b".to_string());

        harness.app.apply_event(CoreEvent::PasteDeleted {
            id: "b".to_string(),
            undo_token: Some("undo-b".to_string()),
        });
        assert_eq!(
            harness.app.selected_id.as_deref(),
            Some(case.expected_selected_id),
            "visible ids: {:?}",
            case.visible_ids
        );
    }
}

#[test]
fn create_new_paste_send_failure_shows_error_status() {
    let TestHarness {
        _dir: _guard,
        mut app,
        cmd_rx,
    } = make_app();
    drop(cmd_rx);

    app.create_new_paste_with_content("hello".to_string());

    assert_eq!(
        app.status.as_ref().map(|status| status.text.as_str()),
        Some("Create failed: backend unavailable.")
    );
    assert!(app.all_pastes.len() == 1);
}

#[test]
fn delete_send_failure_keeps_lock_and_shows_error_status_matrix() {
    assert_delete_send_failure_keeps_lock_and_status(|app| app.delete_selected());
    assert_delete_send_failure_keeps_lock_and_status(|app| {
        app.send_palette_delete("alpha".to_string())
    });
}

#[test]
fn palette_open_failure_keeps_palette_open() {
    let TestHarness {
        _dir: _guard,
        mut app,
        cmd_rx,
    } = make_app();
    drop(cmd_rx);

    app.command_palette_open = true;
    app.open_palette_selection("beta".to_string());

    assert!(
        app.command_palette_open,
        "palette should stay open when open action fails"
    );
    assert_eq!(
        app.status.as_ref().map(|status| status.text.as_str()),
        Some("Get paste failed: backend unavailable.")
    );
}

#[test]
fn palette_copy_send_failure_when_selected_paste_missing_is_cleared() {
    let TestHarness {
        _dir: _guard,
        mut app,
        cmd_rx,
    } = make_app();
    app.selected_paste = None;
    app.pending_copy_action = None;
    drop(cmd_rx);

    app.queue_palette_copy("alpha".to_string(), false);

    assert_eq!(
        app.status.as_ref().map(|status| status.text.as_str()),
        Some("Load paste for copy failed: backend unavailable.")
    );
    assert!(app.pending_copy_action.is_none());
}

#[test]
fn palette_copy_success_matrix_uses_expected_content_and_language() {
    struct PaletteCopyCase {
        fenced: bool,
        saved_content: &'static str,
        active_content: &'static str,
        paste_language: Option<&'static str>,
        edit_language: Option<&'static str>,
        expected_clipboard: &'static str,
    }

    let cases = [
        PaletteCopyCase {
            fenced: false,
            saved_content: "content",
            active_content: "content",
            paste_language: None,
            edit_language: None,
            expected_clipboard: "content",
        },
        PaletteCopyCase {
            fenced: true,
            saved_content: "content",
            active_content: "content",
            paste_language: Some("rust"),
            edit_language: None,
            expected_clipboard: "```rust\ncontent\n```",
        },
        PaletteCopyCase {
            fenced: false,
            saved_content: "saved",
            active_content: "unsaved",
            paste_language: Some("rust"),
            edit_language: None,
            expected_clipboard: "unsaved",
        },
        PaletteCopyCase {
            fenced: true,
            saved_content: "saved",
            active_content: "unsaved",
            paste_language: Some("rust"),
            edit_language: Some("python"),
            expected_clipboard: "```python\nunsaved\n```",
        },
    ];

    for case in cases {
        let mut harness = make_app();
        if let Some(paste) = harness.app.selected_paste.as_mut() {
            paste.id = "alpha".to_string();
            paste.content = case.saved_content.to_string();
            paste.language = case.paste_language.map(str::to_string);
        }
        set_active_content(&mut harness.app, case.active_content);
        harness.app.edit_language = case.edit_language.map(str::to_string);
        harness.app.pending_copy_action = None;

        harness
            .app
            .queue_palette_copy("alpha".to_string(), case.fenced);

        assert_eq!(
            harness.app.clipboard_outgoing.as_deref(),
            Some(case.expected_clipboard)
        );
        assert!(harness.app.pending_copy_action.is_none());
    }
}

#[test]
fn palette_copy_send_failure_after_reselect_clears_copy_pending_action() {
    let TestHarness {
        _dir: _guard,
        mut app,
        cmd_rx,
    } = make_app();
    app.selected_id = None;
    app.selected_paste = None;
    app.pending_copy_action = None;
    drop(cmd_rx);

    app.queue_palette_copy("alpha".to_string(), true);

    assert_eq!(
        app.status.as_ref().map(|status| status.text.as_str()),
        Some("Get paste failed: backend unavailable.")
    );
    assert!(app.pending_copy_action.is_none());
    assert!(
        !app.locks.is_locked("alpha").expect("is_locked"),
        "failed reselect should not leak a stale lock"
    );
    assert!(
        app.selected_id.is_none(),
        "failed reselect should clear stale selection state"
    );
}
