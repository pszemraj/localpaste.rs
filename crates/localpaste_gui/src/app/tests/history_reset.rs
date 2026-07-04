//! History reset save-and-flush regression tests.

use super::*;

#[test]
fn queued_history_reset_redispatches_dirty_metadata_after_stale_ack() {
    let mut harness = make_app();
    harness.app.edit_name = "First name".to_string();
    harness.app.metadata_dirty = true;
    harness.app.save_metadata_now();
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::UpdatePasteMeta { id, name, .. } => {
            assert_eq!(id, "alpha");
            assert_eq!(name.as_deref(), Some("First name"));
        }
        other => panic!("expected initial metadata save, got {:?}", other),
    }
    assert!(harness.app.metadata_save_in_flight);

    harness.app.edit_name = "Second name".to_string();
    harness.app.metadata_dirty = true;
    harness.app.version_ui.history_reset_confirm_target = Some(42);
    harness.app.reset_selected_history_version();

    assert!(harness.app.history_reset_flush_active());
    assert!(matches!(
        harness.cmd_rx.try_recv(),
        Err(TryRecvError::Empty)
    ));

    let mut stale_ack = Paste::new("content".to_string(), "First name".to_string());
    stale_ack.id = "alpha".to_string();
    harness
        .app
        .apply_event(CoreEvent::PasteMetaSaved { paste: stale_ack });

    assert!(harness.app.history_reset_flush_active());
    assert!(harness.app.metadata_dirty);
    assert!(harness.app.metadata_save_in_flight);
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::UpdatePasteMeta { id, name, .. } => {
            assert_eq!(id, "alpha");
            assert_eq!(name.as_deref(), Some("Second name"));
        }
        other => panic!("expected redispatched metadata save, got {:?}", other),
    }

    let mut fresh_ack = Paste::new("content".to_string(), "Second name".to_string());
    fresh_ack.id = "alpha".to_string();
    harness
        .app
        .apply_event(CoreEvent::PasteMetaSaved { paste: fresh_ack });

    assert!(!harness.app.history_reset_flush_active());
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::ResetPasteHardToVersion {
            id,
            version_id_ms,
            preserve_current_head,
        } => {
            assert_eq!(id, "alpha");
            assert_eq!(version_id_ms, 42);
            assert!(preserve_current_head);
        }
        other => panic!("expected reset after fresh metadata ack, got {:?}", other),
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
    harness.app.version_history_limit = 250;

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
            assert_eq!(limit, 250);
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
        CoreCmd::ResetPasteHardToVersion {
            id,
            version_id_ms,
            preserve_current_head,
        } => {
            assert_eq!(id, "alpha");
            assert_eq!(version_id_ms, 41);
            assert!(preserve_current_head);
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
fn history_reset_flushes_local_changes_before_reset_matrix() {
    #[derive(Clone, Copy)]
    enum ResetBlockCase {
        ContentDirty,
        MetadataDirty,
        MetadataSaving,
    }

    for case in [
        ResetBlockCase::ContentDirty,
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
            ResetBlockCase::MetadataDirty => {
                harness.app.metadata_dirty = true;
            }
            ResetBlockCase::MetadataSaving => {
                harness.app.metadata_save_in_flight = true;
            }
        }

        harness.app.reset_selected_history_version();

        assert!(harness.app.history_reset_flush_active());
        assert!(harness
            .app
            .version_ui
            .history_reset_confirm_target
            .is_none());
        assert_eq!(
            harness
                .app
                .status
                .as_ref()
                .map(|status| status.text.as_str()),
            Some("Saving current paste before reset...")
        );

        match case {
            ResetBlockCase::ContentDirty => match recv_cmd(&harness.cmd_rx) {
                CoreCmd::UpdatePasteVirtual {
                    id,
                    content,
                    protected_version_id_ms,
                } => {
                    assert_eq!(id, "alpha");
                    assert_eq!(content.to_string(), "content");
                    assert_eq!(protected_version_id_ms, Some(42));
                }
                other => panic!("expected content save before reset, got {:?}", other),
            },
            ResetBlockCase::MetadataDirty => match recv_cmd(&harness.cmd_rx) {
                CoreCmd::UpdatePasteMeta { id, .. } => assert_eq!(id, "alpha"),
                other => panic!("expected metadata save before reset, got {:?}", other),
            },
            ResetBlockCase::MetadataSaving => {
                assert!(matches!(
                    harness.cmd_rx.try_recv(),
                    Err(TryRecvError::Empty)
                ));
            }
        }

        harness.app.save_status = SaveStatus::Saved;
        harness.app.save_in_flight = false;
        harness.app.metadata_dirty = false;
        harness.app.metadata_save_in_flight = false;
        harness.app.metadata_save_request = None;
        harness.app.maybe_continue_queued_history_reset();

        match recv_cmd(&harness.cmd_rx) {
            CoreCmd::ResetPasteHardToVersion {
                id,
                version_id_ms,
                preserve_current_head,
            } => {
                assert_eq!(id, "alpha");
                assert_eq!(version_id_ms, 42);
                assert!(preserve_current_head);
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
    }
}

#[test]
fn history_reset_rejects_preexisting_content_save_without_queue() {
    let mut harness = make_app();
    harness.app.version_ui.history_versions = vec![localpaste_core::models::paste::VersionMeta {
        version_id_ms: 42,
        created_at: chrono::Utc::now(),
        content_hash: "hash".to_string(),
        len: 4,
        language: None,
        language_is_manual: false,
    }];
    harness.app.version_ui.history_selected_index = 1;
    harness.app.open_history_reset_confirm();
    harness.app.save_status = SaveStatus::Saving;
    harness.app.save_in_flight = true;

    harness.app.reset_selected_history_version();

    assert!(!harness.app.history_reset_flush_active());
    assert!(harness
        .app
        .version_ui
        .history_reset_confirm_target
        .is_some());
    assert_eq!(
        harness
            .app
            .status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some("Wait for the current content save to finish before resetting history.")
    );
    assert!(matches!(
        harness.cmd_rx.try_recv(),
        Err(TryRecvError::Empty)
    ));
}

#[test]
fn history_reset_flush_dispatch_failure_cancels_queue_with_status() {
    let TestHarness {
        _dir: _guard,
        mut app,
        cmd_rx,
    } = make_app();
    app.version_ui.history_reset_confirm_target = Some(42);
    app.save_status = SaveStatus::Dirty;
    app.last_edit_at = Some(Instant::now());
    app.metadata_dirty = true;
    drop(cmd_rx);

    app.reset_selected_history_version();

    assert!(!app.history_reset_flush_active());
    assert_eq!(
        app.status.as_ref().map(|status| status.text.as_str()),
        Some("Reset cancelled because current paste could not be saved.")
    );
}

#[test]
fn queued_history_reset_cancels_when_selection_changes_before_flush_finishes() {
    let mut harness = make_app();
    harness.app.version_ui.history_reset_confirm_target = Some(42);
    harness.app.save_status = SaveStatus::Dirty;
    harness.app.last_edit_at = Some(Instant::now());

    harness.app.reset_selected_history_version();
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::UpdatePasteVirtual { id, .. } => {
            assert_eq!(id, "alpha")
        }
        other => panic!("expected content save before reset, got {:?}", other),
    }
    assert!(harness.app.history_reset_flush_active());

    harness.app.selected_id = Some("beta".to_string());
    harness.app.save_status = SaveStatus::Saved;
    harness.app.save_in_flight = false;
    harness.app.maybe_continue_queued_history_reset();

    assert!(!harness.app.history_reset_flush_active());
    assert_eq!(
        harness
            .app
            .status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some("Reset cancelled because the selected paste changed.")
    );
    assert!(matches!(
        harness.cmd_rx.try_recv(),
        Err(TryRecvError::Empty)
    ));
}

#[test]
fn deferred_save_rollback_restores_dirty_flags_after_partial_dispatch() {
    let mut harness = make_app();
    harness.app.save_in_flight = true;
    harness.app.save_status = SaveStatus::Saving;
    harness.app.save_request_revision = Some(7);
    harness.app.last_edit_at = None;
    harness.app.metadata_save_in_flight = true;
    harness.app.metadata_dirty = false;
    harness.app.metadata_save_request = Some(MetadataDraftSnapshot {
        name: "Alpha".to_string(),
        language: None,
        language_is_manual: false,
        tags_csv: String::new(),
    });

    crate::app::deferred_saves::rollback_deferred_save_dispatches(&mut harness.app, true, true);

    assert!(!harness.app.save_in_flight);
    assert!(matches!(harness.app.save_status, SaveStatus::Dirty));
    assert!(harness.app.save_request_revision.is_none());
    assert!(harness.app.last_edit_at.is_some());
    assert!(!harness.app.metadata_save_in_flight);
    assert!(harness.app.metadata_dirty);
    assert!(harness.app.metadata_save_request.is_none());
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

    set_active_content(&mut harness.app, "auto-save");
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
