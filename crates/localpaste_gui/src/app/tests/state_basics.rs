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
                assert_eq!(harness.app.selected_content.len(), 0);
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

#[test]
fn set_status_pushes_toast_feedback() {
    let mut harness = make_app();
    harness.app.set_status("Saved metadata.");

    assert!(harness.app.status.is_some());
    assert_eq!(harness.app.toasts.len(), 1);
    assert_eq!(
        harness.app.toasts.back().map(|toast| toast.text.as_str()),
        Some("Saved metadata.")
    );
}

#[test]
fn toast_queue_dedupes_tail_and_caps_length() {
    let mut harness = make_app();

    harness.app.set_status("Repeated");
    harness.app.set_status("Repeated");
    assert_eq!(harness.app.toasts.len(), 1);

    for idx in 0..(TOAST_LIMIT + 2) {
        harness.app.set_status(format!("Toast {}", idx));
    }
    assert_eq!(harness.app.toasts.len(), TOAST_LIMIT);
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
fn editor_buffer_tracks_char_len() {
    let mut buffer = EditorBuffer::new("ab".to_string());
    assert_eq!(buffer.chars_len(), 2);

    buffer.insert_text("\u{00E9}", 1);
    assert_eq!(buffer.chars_len(), 3);

    buffer.delete_char_range(1..2);
    assert_eq!(buffer.chars_len(), 2);

    buffer.replace_with("xyz");
    assert_eq!(buffer.chars_len(), 3);

    buffer.clear();
    assert_eq!(buffer.chars_len(), 0);
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
    });

    assert!(harness.app.pending_copy_action.is_none());
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
        selected_content: &'static str,
        paste_language: Option<&'static str>,
        edit_language: Option<&'static str>,
        expected_clipboard: &'static str,
    }

    let cases = [
        PaletteCopyCase {
            fenced: false,
            saved_content: "content",
            selected_content: "content",
            paste_language: None,
            edit_language: None,
            expected_clipboard: "content",
        },
        PaletteCopyCase {
            fenced: true,
            saved_content: "content",
            selected_content: "content",
            paste_language: Some("rust"),
            edit_language: None,
            expected_clipboard: "```rust\ncontent\n```",
        },
        PaletteCopyCase {
            fenced: false,
            saved_content: "saved",
            selected_content: "unsaved",
            paste_language: Some("rust"),
            edit_language: None,
            expected_clipboard: "unsaved",
        },
        PaletteCopyCase {
            fenced: true,
            saved_content: "saved",
            selected_content: "unsaved",
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
        harness
            .app
            .selected_content
            .reset(case.selected_content.to_string());
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

#[test]
fn version_modal_failure_cleanup_is_scoped_to_the_matching_request() {
    let mut harness = make_app();
    harness.app.version_ui.diff_target_id = Some("beta".to_string());
    harness.app.version_ui.diff_loading_target = true;
    harness.app.version_ui.history_loading_snapshot_id = Some(42);
    harness.app.version_ui.history_snapshot =
        Some(localpaste_core::models::paste::VersionSnapshot {
            paste_id: "alpha".to_string(),
            version_id_ms: 42,
            created_at: chrono::Utc::now(),
            content_hash: "hash".to_string(),
            len: 4,
            language: None,
            language_is_manual: false,
            content: "text".to_string(),
        });
    harness.app.version_ui.history_reset_in_flight_paste_id = Some("alpha".to_string());

    harness.app.apply_event(CoreEvent::DiffTargetLoadFailed {
        id: "beta".to_string(),
        message: "Diff load failed: missing".to_string(),
    });
    assert!(!harness.app.version_ui.diff_loading_target);
    assert!(harness.app.version_ui.diff_target_id.is_none());
    assert_eq!(harness.app.version_ui.history_loading_snapshot_id, Some(42));
    assert!(harness.app.version_ui.history_snapshot.is_some());
    assert_eq!(
        harness
            .app
            .status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some("Diff load failed: missing")
    );

    harness.app.apply_event(CoreEvent::Error {
        source: crate::backend::CoreErrorSource::Other,
        message: "Search failed".to_string(),
    });
    assert_eq!(harness.app.version_ui.history_loading_snapshot_id, Some(42));
    assert!(harness.app.version_ui.history_snapshot.is_some());

    harness.app.apply_event(CoreEvent::PasteVersionLoadFailed {
        paste_id: "alpha".to_string(),
        version_id_ms: 42,
        message: "Get version failed".to_string(),
    });
    assert!(harness.app.version_ui.history_loading_snapshot_id.is_none());
    assert!(harness.app.version_ui.history_snapshot.is_none());
    assert_eq!(
        harness
            .app
            .status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some("Get version failed")
    );

    harness.app.apply_event(CoreEvent::Error {
        source: crate::backend::CoreErrorSource::SaveContent,
        message: "reset failed".to_string(),
    });
    assert!(harness
        .app
        .version_ui
        .history_reset_in_flight_paste_id
        .is_none());
}

#[test]
fn stale_history_load_failures_do_not_override_status_after_snapshot_request_changes() {
    let mut harness = make_app();
    harness.app.set_status("Ready");
    harness.app.version_ui.history_loading_snapshot_id = Some(84);
    harness.app.version_ui.history_snapshot =
        Some(localpaste_core::models::paste::VersionSnapshot {
            paste_id: "alpha".to_string(),
            version_id_ms: 84,
            created_at: chrono::Utc::now(),
            content_hash: "hash-84".to_string(),
            len: 4,
            language: None,
            language_is_manual: false,
            content: "next".to_string(),
        });

    harness.app.apply_event(CoreEvent::PasteVersionLoadFailed {
        paste_id: "alpha".to_string(),
        version_id_ms: 42,
        message: "stale version failure".to_string(),
    });

    assert_eq!(harness.app.version_ui.history_loading_snapshot_id, Some(84));
    assert_eq!(
        harness
            .app
            .version_ui
            .history_snapshot
            .as_ref()
            .map(|snapshot| snapshot.version_id_ms),
        Some(84)
    );
    assert_eq!(
        harness
            .app
            .status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some("Ready")
    );
}

#[test]
fn stale_diff_target_failures_do_not_override_status_after_target_changes() {
    let mut harness = make_app();
    harness.app.set_status("Ready");
    harness.app.version_ui.diff_target_id = Some("gamma".to_string());
    harness.app.version_ui.diff_loading_target = true;

    harness.app.apply_event(CoreEvent::DiffTargetLoadFailed {
        id: "beta".to_string(),
        message: "Diff load failed: stale".to_string(),
    });

    assert!(harness.app.version_ui.diff_loading_target);
    assert_eq!(
        harness.app.version_ui.diff_target_id.as_deref(),
        Some("gamma")
    );
    assert_eq!(
        harness
            .app
            .status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some("Ready")
    );
}

#[test]
fn stale_diff_target_missing_refreshes_lists_without_overwriting_status() {
    let mut harness = make_app();
    harness.app.set_status("Ready");
    harness
        .app
        .all_pastes
        .push(test_summary("beta", "Beta", None, 4));
    harness.app.pastes = harness.app.all_pastes.clone();

    harness.app.apply_event(CoreEvent::DiffTargetMissing {
        id: "beta".to_string(),
    });

    assert!(
        !harness.app.pastes.iter().any(|paste| paste.id == "beta"),
        "stale diff-target missing events should still keep list caches honest"
    );
    assert_eq!(
        harness
            .app
            .status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some("Ready")
    );
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::ListPastes { .. } => {}
        other => panic!("expected ListPastes refresh command, got {:?}", other),
    }
}

#[test]
fn request_diff_target_uses_detached_backend_command() {
    let mut harness = make_app();

    harness.app.request_diff_target("beta".to_string());

    assert_eq!(
        harness.app.version_ui.diff_target_id.as_deref(),
        Some("beta")
    );
    assert!(harness.app.version_ui.diff_loading_target);
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::GetDiffTargetPaste { id } => assert_eq!(id, "beta"),
        other => panic!("expected GetDiffTargetPaste command, got {:?}", other),
    }
}

#[test]
fn version_refresh_reloads_selected_snapshot_after_prior_load_failure() {
    let mut harness = make_app();
    harness.app.selected_id = Some("alpha".to_string());
    harness.app.version_ui.history_selected_index = 1;
    harness.app.version_ui.history_versions = vec![localpaste_core::models::paste::VersionMeta {
        version_id_ms: 42,
        created_at: chrono::Utc::now(),
        content_hash: "hash".to_string(),
        len: 4,
        language: None,
        language_is_manual: false,
    }];
    harness.app.version_ui.history_snapshot = None;
    harness.app.version_ui.history_loading_snapshot_id = None;

    let refreshed_items = vec![localpaste_core::models::paste::VersionMeta {
        version_id_ms: 42,
        created_at: chrono::Utc::now(),
        content_hash: "hash".to_string(),
        len: 4,
        language: None,
        language_is_manual: false,
    }];
    harness.app.apply_event(CoreEvent::PasteVersionsLoaded {
        id: "alpha".to_string(),
        items: refreshed_items,
    });

    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::GetPasteVersion { id, version_id_ms } => {
            assert_eq!(id, "alpha");
            assert_eq!(version_id_ms, 42);
        }
        other => panic!("expected GetPasteVersion command, got {:?}", other),
    }
}

#[test]
fn content_save_refreshes_open_history_modal_for_active_paste() {
    let mut harness = make_app();
    harness.app.selected_id = Some("alpha".to_string());
    harness.app.version_ui.history_modal_open = true;

    let mut saved = Paste::new("updated".to_string(), "Alpha".to_string());
    saved.id = "alpha".to_string();

    harness
        .app
        .apply_event(CoreEvent::PasteSaved { paste: saved });

    match harness
        .cmd_rx
        .recv_timeout(Duration::from_millis(200))
        .expect("expected version refresh command")
    {
        CoreCmd::ListPasteVersions { id, limit } => {
            assert_eq!(id, "alpha");
            assert_eq!(limit, 200);
        }
        other => panic!("expected ListPasteVersions command, got {:?}", other),
    }
}

#[test]
fn reset_to_version_invalidates_active_search_dispatch_state() {
    let mut harness = make_app();
    harness.app.search_query = "alpha".to_string();
    harness.app.search_last_sent = "alpha".to_string();
    harness.app.search_last_input_at = None;
    harness.app.selected_id = Some("alpha".to_string());

    let mut reset_paste = Paste::new("reset content".to_string(), "Alpha".to_string());
    reset_paste.id = "alpha".to_string();

    harness
        .app
        .apply_event(CoreEvent::PasteResetToVersion { paste: reset_paste });

    assert!(
        harness.app.search_last_sent.is_empty(),
        "reset should force a new backend search when query text is unchanged"
    );
    assert!(
        harness.app.search_last_input_at.is_some(),
        "search dispatch timestamp should be rewound so maybe_dispatch_search sends immediately"
    );
}

#[test]
fn reset_to_version_reprojects_sidebar_filters_without_search_query() {
    let mut harness = make_app();
    harness.app.all_pastes = vec![
        test_summary("alpha", "Alpha", Some("rust"), 7),
        test_summary("beta", "Beta", Some("rust"), 5),
    ];
    harness.app.pastes = harness.app.all_pastes.clone();
    harness
        .app
        .set_active_language_filter(Some("rust".to_string()));
    harness.app.selected_id = Some("alpha".to_string());

    let mut reset_paste = Paste::new("reset content".to_string(), "Alpha".to_string());
    reset_paste.id = "alpha".to_string();
    reset_paste.language = Some("python".to_string());
    reset_paste.language_is_manual = true;

    harness
        .app
        .apply_event(CoreEvent::PasteResetToVersion { paste: reset_paste });

    assert_eq!(
        harness
            .app
            .pastes
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        vec!["beta"],
        "reset should immediately reproject active language-filter results"
    );
    assert_eq!(harness.app.selected_id.as_deref(), Some("beta"));
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::GetPaste { id } => assert_eq!(id, "beta"),
        other => panic!("expected GetPaste command, got {:?}", other),
    }
}

#[test]
fn history_reset_confirm_keeps_original_target_after_selection_changes() {
    let mut harness = make_app();
    let version = |version_id_ms| localpaste_core::models::paste::VersionMeta {
        version_id_ms,
        created_at: chrono::Utc::now(),
        content_hash: format!("hash-{version_id_ms}"),
        len: 4,
        language: None,
        language_is_manual: false,
    };
    harness.app.version_ui.history_versions = vec![version(41), version(42)];
    harness.app.version_ui.history_selected_index = 1;

    harness.app.open_history_reset_confirm();
    assert_eq!(
        harness.app.version_ui.history_reset_confirm_target,
        Some(41)
    );

    harness.app.version_ui.history_selected_index = 2;
    harness.app.reset_selected_history_version();

    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::ResetPasteHardToVersion { id, version_id_ms } => {
            assert_eq!(id, "alpha");
            assert_eq!(version_id_ms, 41);
        }
        other => panic!("expected ResetPasteHardToVersion command, got {:?}", other),
    }
    assert_eq!(
        harness
            .app
            .version_ui
            .history_reset_in_flight_paste_id
            .as_deref(),
        Some("alpha")
    );
    assert!(harness
        .app
        .version_ui
        .history_reset_confirm_target
        .is_none());
}

#[test]
fn history_reset_is_blocked_while_local_changes_are_unsaved_or_saving_matrix() {
    #[derive(Clone, Copy)]
    enum ResetBlockCase {
        ContentDirty,
        ContentSaving,
        MetadataDirty,
        MetadataSaving,
    }

    for case in [
        ResetBlockCase::ContentDirty,
        ResetBlockCase::ContentSaving,
        ResetBlockCase::MetadataDirty,
        ResetBlockCase::MetadataSaving,
    ] {
        let mut harness = make_app();
        harness.app.version_ui.history_versions =
            vec![localpaste_core::models::paste::VersionMeta {
                version_id_ms: 42,
                created_at: chrono::Utc::now(),
                content_hash: "hash".to_string(),
                len: 4,
                language: None,
                language_is_manual: false,
            }];
        harness.app.version_ui.history_selected_index = 1;
        harness.app.open_history_reset_confirm();

        match case {
            ResetBlockCase::ContentDirty => {
                harness.app.save_status = SaveStatus::Dirty;
            }
            ResetBlockCase::ContentSaving => {
                harness.app.save_status = SaveStatus::Saving;
                harness.app.save_in_flight = true;
            }
            ResetBlockCase::MetadataDirty => {
                harness.app.metadata_dirty = true;
            }
            ResetBlockCase::MetadataSaving => {
                harness.app.metadata_save_in_flight = true;
            }
        }

        harness.app.reset_selected_history_version();

        assert!(harness
            .app
            .version_ui
            .history_reset_in_flight_paste_id
            .is_none());
        assert_eq!(
            harness.app.version_ui.history_reset_confirm_target,
            Some(42)
        );
        assert_eq!(
            harness
                .app
                .status
                .as_ref()
                .map(|status| status.text.as_str()),
            Some("Reset is unavailable while local changes are unsaved or saving.")
        );
        assert!(matches!(
            harness.cmd_rx.try_recv(),
            Err(TryRecvError::Empty)
        ));
    }
}

#[test]
fn history_reset_in_flight_blocks_selection_switches_until_matching_ack() {
    let mut harness = make_app();
    harness.app.all_pastes = vec![
        test_summary("alpha", "Alpha", None, 7),
        test_summary("beta", "Beta", None, 5),
    ];
    harness.app.pastes = harness.app.all_pastes.clone();
    harness
        .app
        .locks
        .acquire("alpha", &harness.app.lock_owner_id)
        .expect("acquire alpha lock");
    harness.app.version_ui.history_reset_in_flight_paste_id = Some("alpha".to_string());

    assert!(harness.app.reset_transition_active());
    assert!(!harness.app.select_paste("beta".to_string()));
    assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));
    assert!(
        harness
            .app
            .locks
            .is_locked("alpha")
            .expect("alpha lock state"),
        "pending reset should keep the current paste locked until ack/error"
    );
    assert_eq!(
        harness
            .app
            .status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some("Reset in progress; editor is temporarily read-only.")
    );
    assert!(matches!(
        harness.cmd_rx.try_recv(),
        Err(TryRecvError::Empty)
    ));

    let mut reset_paste = Paste::new("reset content".to_string(), "Alpha".to_string());
    reset_paste.id = "alpha".to_string();
    harness
        .app
        .apply_event(CoreEvent::PasteResetToVersion { paste: reset_paste });

    assert!(harness
        .app
        .version_ui
        .history_reset_in_flight_paste_id
        .is_none());

    assert!(harness.app.select_paste("beta".to_string()));
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::GetPaste { id } => assert_eq!(id, "beta"),
        other => panic!("expected GetPaste command, got {:?}", other),
    }
}

#[test]
fn history_reset_in_flight_preserves_selection_across_list_reprojection() {
    let mut harness = make_app();
    harness.app.all_pastes = vec![
        test_summary("alpha", "Alpha", None, 7),
        test_summary("beta", "Beta", None, 5),
    ];
    harness.app.pastes = vec![test_summary("beta", "Beta", None, 5)];
    harness
        .app
        .locks
        .acquire("alpha", &harness.app.lock_owner_id)
        .expect("acquire alpha lock");
    harness.app.version_ui.history_reset_in_flight_paste_id = Some("alpha".to_string());

    harness.app.ensure_selection_after_list_update();

    assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));
    assert!(
        harness
            .app
            .locks
            .is_locked("alpha")
            .expect("alpha lock state"),
        "filter/search reprojection should not release the selected paste lock during reset"
    );
    assert!(matches!(
        harness.cmd_rx.try_recv(),
        Err(TryRecvError::Empty)
    ));
}

#[test]
fn reset_transition_is_inactive_without_selected_or_pending_reset_paste() {
    let mut harness = make_app();
    harness.app.selected_id = None;
    harness.app.selected_paste = None;
    harness.app.version_ui.history_reset_in_flight_paste_id = None;

    assert!(
        !harness.app.reset_transition_active(),
        "lack of selection must not be mistaken for a pending reset fence"
    );
}

#[test]
fn content_save_error_does_not_clear_reset_pending_for_other_paste() {
    let mut harness = make_app();
    harness.app.selected_id = Some("beta".to_string());
    harness.app.save_in_flight = true;
    harness.app.save_status = SaveStatus::Saving;
    harness.app.version_ui.history_reset_in_flight_paste_id = Some("alpha".to_string());

    harness.app.apply_event(CoreEvent::Error {
        source: crate::backend::CoreErrorSource::SaveContent,
        message: "Update failed: backend unavailable.".to_string(),
    });

    assert_eq!(
        harness
            .app
            .version_ui
            .history_reset_in_flight_paste_id
            .as_deref(),
        Some("alpha")
    );
}

#[test]
fn history_reset_in_flight_blocks_create_delete_and_paste_as_new_requests() {
    let mut harness = make_app();
    let ctx = eframe::egui::Context::default();
    harness.app.version_ui.history_reset_in_flight_paste_id = Some("alpha".to_string());

    harness
        .app
        .create_new_paste_with_content("hello".to_string());
    harness.app.delete_selected();
    harness.app.send_palette_delete("alpha".to_string());
    harness.app.request_paste_as_new(&ctx);

    assert_eq!(harness.app.paste_as_new_pending_frames, 0);
    assert!(harness.app.paste_as_new_clipboard_requested_at.is_none());
    assert_eq!(
        harness
            .app
            .status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some("Reset in progress; editor is temporarily read-only.")
    );
    assert!(matches!(
        harness.cmd_rx.try_recv(),
        Err(TryRecvError::Empty)
    ));
}

#[test]
fn history_reset_in_flight_blocks_dirtying_and_save_dispatches() {
    let mut harness = make_app();
    harness.app.version_ui.history_reset_in_flight_paste_id = Some("alpha".to_string());

    harness.app.mark_dirty();
    assert!(matches!(harness.app.save_status, SaveStatus::Saved));
    assert!(harness.app.last_edit_at.is_none());

    harness.app.selected_content.reset("auto-save".to_string());
    harness.app.save_status = SaveStatus::Dirty;
    harness.app.last_edit_at =
        Some(Instant::now() - harness.app.autosave_delay - Duration::from_millis(5));
    harness.app.maybe_autosave();
    assert!(!harness.app.save_in_flight);
    assert!(matches!(
        harness.cmd_rx.try_recv(),
        Err(TryRecvError::Empty)
    ));

    harness.app.save_now();
    assert!(!harness.app.save_in_flight);
    assert!(matches!(harness.app.save_status, SaveStatus::Dirty));

    harness.app.metadata_dirty = true;
    harness.app.save_metadata_now();
    assert!(!harness.app.metadata_save_in_flight);
    assert!(harness.app.metadata_dirty);
    assert_eq!(
        harness
            .app
            .status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some("Reset in progress; editor is temporarily read-only.")
    );
    assert!(matches!(
        harness.cmd_rx.try_recv(),
        Err(TryRecvError::Empty)
    ));
}
