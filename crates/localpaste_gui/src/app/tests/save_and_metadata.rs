//! Save/autosave and metadata command emission tests.

use super::*;
use crate::backend::CoreErrorSource;

#[test]
fn paste_meta_saved_refilters_when_selected_paste_leaves_active_scope() {
    let mut harness = make_app();
    let now = Utc::now();
    harness.app.apply_event(CoreEvent::PasteList {
        items: vec![
            PasteSummary {
                id: "alpha".to_string(),
                name: "Alpha".to_string(),
                language: Some("rust".to_string()),
                content_len: 7,
                updated_at: now,
                folder_id: None,
                tags: Vec::new(),
            },
            PasteSummary {
                id: "beta".to_string(),
                name: "Beta".to_string(),
                language: Some("rust".to_string()),
                content_len: 7,
                updated_at: now,
                folder_id: None,
                tags: Vec::new(),
            },
        ],
    });
    harness
        .app
        .set_active_collection(SidebarCollection::Unfiled);
    harness.app.select_paste("alpha".to_string());

    let mut moved = Paste::new("moved".to_string(), "Alpha".to_string());
    moved.id = "alpha".to_string();
    moved.folder_id = Some("folder-z".to_string());
    moved.language = Some("rust".to_string());
    harness
        .app
        .apply_event(CoreEvent::PasteMetaSaved { paste: moved });

    assert_eq!(harness.app.pastes.len(), 1);
    assert_eq!(harness.app.pastes[0].id, "beta");
    assert_eq!(harness.app.selected_id.as_deref(), Some("beta"));
}

#[test]
fn save_metadata_now_sends_manual_language_and_normalized_tags() {
    let mut harness = make_app();
    harness.app.metadata_dirty = true;
    harness.app.edit_name = "Script One".to_string();
    harness.app.edit_language = Some("python".to_string());
    harness.app.edit_language_is_manual = true;
    harness.app.edit_tags = "rust, CLI, rust, cli, ".to_string();

    harness.app.save_metadata_now();
    assert!(harness.app.metadata_dirty);
    assert!(harness.app.metadata_save_in_flight);

    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::UpdatePasteMeta {
            id,
            name,
            language,
            language_is_manual,
            folder_id,
            tags,
        } => {
            assert_eq!(id, "alpha");
            assert_eq!(name.as_deref(), Some("Script One"));
            assert_eq!(language.as_deref(), Some("python"));
            assert_eq!(language_is_manual, Some(true));
            assert!(folder_id.is_none());
            assert_eq!(
                tags.expect("tags"),
                vec!["rust".to_string(), "CLI".to_string()]
            );
        }
        other => panic!("unexpected command: {:?}", other),
    }
}

#[test]
fn save_metadata_now_auto_language_clears_override_without_folder_edits() {
    let mut harness = make_app();
    harness.app.metadata_dirty = true;
    harness.app.edit_name = "Auto Language".to_string();
    harness.app.edit_language = Some("python".to_string());
    harness.app.edit_language_is_manual = false;
    harness.app.edit_tags = String::new();

    harness.app.save_metadata_now();
    assert!(harness.app.metadata_dirty);
    assert!(harness.app.metadata_save_in_flight);

    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::UpdatePasteMeta {
            language,
            language_is_manual,
            folder_id,
            ..
        } => {
            assert!(language.is_none());
            assert_eq!(language_is_manual, Some(false));
            assert!(folder_id.is_none());
        }
        other => panic!("unexpected command: {:?}", other),
    }
}

#[test]
fn metadata_save_success_clears_dirty_state_on_ack() {
    let mut harness = make_app();
    harness.app.metadata_dirty = true;
    harness.app.edit_name = "Acked".to_string();
    harness.app.save_metadata_now();
    let _ = recv_cmd(&harness.cmd_rx);

    let mut paste = Paste::new("content".to_string(), "Acked".to_string());
    paste.id = "alpha".to_string();
    harness.app.apply_event(CoreEvent::PasteMetaSaved {
        paste: paste.clone(),
    });

    assert!(!harness.app.metadata_dirty);
    assert!(!harness.app.metadata_save_in_flight);
    assert_eq!(harness.app.edit_name, "Acked");
}

#[test]
fn metadata_save_during_active_search_forces_fresh_backend_search() {
    let mut harness = make_app();
    harness.app.search_query = "alpha".to_string();
    harness.app.search_last_sent = "alpha".to_string();

    let mut paste = Paste::new("content".to_string(), "Alpha-renamed".to_string());
    paste.id = "alpha".to_string();
    harness.app.apply_event(CoreEvent::PasteMetaSaved { paste });

    assert!(
        harness.app.search_last_sent.is_empty(),
        "metadata save should invalidate cached search query"
    );

    harness.app.maybe_dispatch_search();
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::SearchPastes { query, .. } => assert_eq!(query, "alpha"),
        other => panic!("unexpected command: {:?}", other),
    }
}

#[test]
fn metadata_save_error_preserves_dirty_state_and_clears_in_flight() {
    let mut harness = make_app();
    harness.app.metadata_dirty = true;
    harness.app.edit_name = "Will Retry".to_string();
    harness.app.save_metadata_now();
    let _ = recv_cmd(&harness.cmd_rx);

    harness.app.apply_event(CoreEvent::Error {
        source: CoreErrorSource::SaveMetadata,
        message: "disk full".to_string(),
    });

    assert!(harness.app.metadata_dirty);
    assert!(!harness.app.metadata_save_in_flight);
    assert_eq!(
        harness
            .app
            .status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some("Metadata save failed: disk full")
    );
}

#[test]
fn content_save_error_does_not_clear_metadata_in_flight() {
    let mut harness = make_app();
    harness.app.metadata_dirty = true;
    harness.app.edit_name = "Will Persist".to_string();
    harness.app.save_metadata_now();
    let _ = recv_cmd(&harness.cmd_rx);

    harness
        .app
        .selected_content
        .reset("edited-content".to_string());
    harness.app.save_status = SaveStatus::Dirty;
    harness.app.last_edit_at =
        Some(Instant::now() - harness.app.autosave_delay - Duration::from_millis(5));
    harness.app.maybe_autosave();
    let _ = recv_cmd(&harness.cmd_rx);
    assert!(harness.app.metadata_save_in_flight);
    assert!(harness.app.save_in_flight);

    harness.app.apply_event(CoreEvent::Error {
        source: CoreErrorSource::SaveContent,
        message: "Update failed: disk full".to_string(),
    });

    assert!(harness.app.metadata_save_in_flight);
    assert!(harness.app.metadata_dirty);
    assert!(matches!(harness.app.save_status, SaveStatus::Dirty));
    assert!(!harness.app.save_in_flight);
    assert_eq!(
        harness
            .app
            .status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some("Update failed: disk full")
    );
}

#[test]
fn save_and_autosave_emit_update_commands_at_expected_times() {
    let mut harness = make_app();
    harness
        .app
        .selected_content
        .reset("manual-save".to_string());
    harness.app.save_status = SaveStatus::Dirty;
    harness.app.save_in_flight = false;
    harness.app.save_now();

    assert!(matches!(harness.app.save_status, SaveStatus::Saving));
    assert!(harness.app.save_in_flight);
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::UpdatePaste { id, content } => {
            assert_eq!(id, "alpha");
            assert_eq!(content, "manual-save");
        }
        other => panic!("unexpected command: {:?}", other),
    }

    harness.app.save_in_flight = false;
    harness.app.save_status = SaveStatus::Dirty;
    harness.app.selected_content.reset("auto-save".to_string());
    harness.app.last_edit_at = Some(Instant::now());
    harness.app.maybe_autosave();
    assert!(matches!(
        harness.cmd_rx.try_recv(),
        Err(TryRecvError::Empty)
    ));

    harness.app.last_edit_at =
        Some(Instant::now() - harness.app.autosave_delay - Duration::from_millis(5));
    harness.app.maybe_autosave();
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::UpdatePaste { id, content } => {
            assert_eq!(id, "alpha");
            assert_eq!(content, "auto-save");
        }
        other => panic!("unexpected command: {:?}", other),
    }

    harness
        .app
        .selected_content
        .reset("edited-during-save".to_string());
    harness.app.mark_dirty();
    assert!(matches!(harness.app.save_status, SaveStatus::Dirty));
    assert!(harness.app.last_edit_at.is_some());

    let mut saved = Paste::new("auto-save".to_string(), "Alpha".to_string());
    saved.id = "alpha".to_string();
    harness
        .app
        .apply_event(CoreEvent::PasteSaved { paste: saved });

    assert!(matches!(harness.app.save_status, SaveStatus::Dirty));
    assert!(!harness.app.save_in_flight);
    assert!(harness.app.last_edit_at.is_some());
    assert_eq!(harness.app.selected_content.as_str(), "edited-during-save");
}

#[test]
fn virtual_editor_autosave_dispatches_rope_snapshot_command() {
    let mut harness = make_app();
    harness.app.editor_mode = EditorMode::VirtualEditor;
    harness.app.virtual_editor_buffer.reset("virtual-content");
    harness.app.save_status = SaveStatus::Dirty;
    harness.app.save_in_flight = false;
    harness.app.last_edit_at =
        Some(Instant::now() - harness.app.autosave_delay - Duration::from_millis(5));

    harness.app.maybe_autosave();
    assert!(matches!(harness.app.save_status, SaveStatus::Saving));
    assert!(harness.app.save_in_flight);
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::UpdatePasteVirtual { id, content } => {
            assert_eq!(id, "alpha");
            assert_eq!(content.to_string(), "virtual-content");
        }
        other => panic!("unexpected command: {:?}", other),
    }
}

#[test]
fn virtual_save_ack_without_revision_keeps_dirty_state_when_buffer_diverged() {
    let mut harness = make_app();
    harness.app.editor_mode = EditorMode::VirtualEditor;
    harness.app.virtual_editor_buffer.reset("local-newer");
    harness.app.save_status = SaveStatus::Saving;
    harness.app.save_in_flight = true;
    harness.app.save_request_revision = None;
    harness.app.last_edit_at = Some(Instant::now());

    let mut saved = Paste::new("server-older".to_string(), "Alpha".to_string());
    saved.id = "alpha".to_string();
    harness
        .app
        .apply_event(CoreEvent::PasteSaved { paste: saved });

    assert!(matches!(harness.app.save_status, SaveStatus::Dirty));
    assert!(!harness.app.save_in_flight);
    assert!(harness.app.last_edit_at.is_some());
    assert_eq!(harness.app.virtual_editor_buffer.to_string(), "local-newer");
}

#[test]
fn real_backend_virtual_save_error_updates_ui_state() {
    let mut harness = make_app();
    let dir = TempDir::new().expect("temp dir");
    let db_path = dir.path().join("db");
    let db = Database::new(db_path.to_str().expect("db path")).expect("db");
    harness.app.backend = crate::backend::spawn_backend(db, 8);

    harness.app.editor_mode = EditorMode::VirtualEditor;
    harness.app.virtual_editor_buffer.reset("123456789");
    harness.app.save_status = SaveStatus::Dirty;
    harness.app.save_in_flight = false;
    harness.app.last_edit_at =
        Some(Instant::now() - harness.app.autosave_delay - Duration::from_millis(5));

    harness.app.maybe_autosave();
    assert!(harness.app.save_in_flight);
    assert!(matches!(harness.app.save_status, SaveStatus::Saving));

    let event = harness
        .app
        .backend
        .evt_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("expected backend error event");
    match &event {
        CoreEvent::Error { source, message } => {
            assert_eq!(*source, CoreErrorSource::SaveContent);
            assert!(message.contains("maximum of 8 bytes"));
        }
        other => panic!("unexpected event: {:?}", other),
    }

    harness.app.apply_event(event);
    assert!(matches!(harness.app.save_status, SaveStatus::Dirty));
    assert!(!harness.app.save_in_flight);
    assert!(harness.app.save_request_revision.is_none());
    assert_eq!(
        harness
            .app
            .status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some("Paste size exceeds maximum of 8 bytes")
    );
}

#[test]
fn select_paste_dirty_defers_switch_until_content_save_ack() {
    let mut harness = make_app();
    harness
        .app
        .all_pastes
        .push(test_summary("beta", "Beta", None, 4));
    harness.app.pastes = harness.app.all_pastes.clone();
    harness.app.selected_content.reset("edited".to_string());
    harness.app.save_status = SaveStatus::Dirty;
    harness.app.last_edit_at = Some(Instant::now());

    assert!(harness.app.select_paste("beta".to_string()));
    assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));
    assert_eq!(harness.app.pending_selection_id.as_deref(), Some("beta"));

    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::UpdatePaste { id, content } => {
            assert_eq!(id, "alpha");
            assert_eq!(content, "edited");
        }
        other => panic!("unexpected command: {:?}", other),
    }

    let mut saved = Paste::new("edited".to_string(), "Alpha".to_string());
    saved.id = "alpha".to_string();
    harness
        .app
        .apply_event(CoreEvent::PasteSaved { paste: saved });

    assert!(harness.app.pending_selection_id.is_none());
    assert_eq!(harness.app.selected_id.as_deref(), Some("beta"));
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::GetPaste { id } => assert_eq!(id, "beta"),
        other => panic!("unexpected command: {:?}", other),
    }
}

#[test]
fn select_paste_while_content_save_in_flight_queues_pending_without_switching() {
    let mut harness = make_app();
    harness
        .app
        .all_pastes
        .push(test_summary("beta", "Beta", None, 4));
    harness.app.pastes = harness.app.all_pastes.clone();
    harness.app.save_status = SaveStatus::Saving;
    harness.app.save_in_flight = true;

    assert!(harness.app.select_paste("beta".to_string()));
    assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));
    assert_eq!(harness.app.pending_selection_id.as_deref(), Some("beta"));
    assert!(matches!(
        harness.cmd_rx.try_recv(),
        Err(TryRecvError::Empty)
    ));
}

#[test]
fn in_flight_selection_uses_latest_pending_target_and_clears_replaced_copy_intent() {
    let mut harness = make_app();
    harness
        .app
        .all_pastes
        .push(test_summary("beta", "Beta", None, 4));
    harness
        .app
        .all_pastes
        .push(test_summary("gamma", "Gamma", None, 4));
    harness.app.pastes = harness.app.all_pastes.clone();
    harness.app.save_status = SaveStatus::Saving;
    harness.app.save_in_flight = true;
    harness.app.pending_copy_action = Some(PaletteCopyAction::Raw("beta".to_string()));

    assert!(harness.app.select_paste("beta".to_string()));
    assert_eq!(harness.app.pending_selection_id.as_deref(), Some("beta"));
    assert!(harness.app.pending_copy_action.is_some());
    assert!(harness.app.select_paste("gamma".to_string()));
    assert_eq!(harness.app.pending_selection_id.as_deref(), Some("gamma"));
    assert!(
        harness.app.pending_copy_action.is_none(),
        "replacing pending target should clear copy intent bound to replaced id"
    );
    assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));

    let mut saved = Paste::new("content".to_string(), "Alpha".to_string());
    saved.id = "alpha".to_string();
    harness
        .app
        .apply_event(CoreEvent::PasteSaved { paste: saved });

    assert!(harness.app.pending_selection_id.is_none());
    assert_eq!(harness.app.selected_id.as_deref(), Some("gamma"));
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::GetPaste { id } => assert_eq!(id, "gamma"),
        other => panic!("unexpected command: {:?}", other),
    }
}

#[test]
fn paste_created_without_unsaved_state_selects_inline_without_get_roundtrip() {
    let mut harness = make_app();

    let mut created = Paste::new("new-content".to_string(), "new-note".to_string());
    created.id = "new-id".to_string();
    harness
        .app
        .apply_event(CoreEvent::PasteCreated { paste: created });

    assert_eq!(harness.app.selected_id.as_deref(), Some("new-id"));
    assert_eq!(harness.app.selected_content.as_str(), "new-content");
    assert_eq!(
        harness
            .app
            .selected_paste
            .as_ref()
            .map(|paste| paste.id.as_str()),
        Some("new-id")
    );
    assert!(matches!(
        harness.cmd_rx.try_recv(),
        Err(TryRecvError::Empty)
    ));
}

#[test]
fn paste_created_while_dirty_preserves_current_buffers_until_switch_completes() {
    let mut harness = make_app();
    harness.app.selected_content.reset("edited-old".to_string());
    harness.app.save_status = SaveStatus::Dirty;
    harness.app.last_edit_at = Some(Instant::now());

    let mut created = Paste::new("new-content".to_string(), "new-note".to_string());
    created.id = "new-id".to_string();
    harness
        .app
        .apply_event(CoreEvent::PasteCreated { paste: created });

    assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));
    assert_eq!(harness.app.pending_selection_id.as_deref(), Some("new-id"));
    assert_eq!(harness.app.selected_content.as_str(), "edited-old");
    assert!(matches!(harness.app.save_status, SaveStatus::Saving));
    assert!(harness.app.save_in_flight);

    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::UpdatePaste { id, content } => {
            assert_eq!(id, "alpha");
            assert_eq!(content, "edited-old");
        }
        other => panic!("unexpected command: {:?}", other),
    }

    let mut saved = Paste::new("edited-old".to_string(), "Alpha".to_string());
    saved.id = "alpha".to_string();
    harness
        .app
        .apply_event(CoreEvent::PasteSaved { paste: saved });

    assert!(harness.app.pending_selection_id.is_none());
    assert_eq!(harness.app.selected_id.as_deref(), Some("new-id"));
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::GetPaste { id } => assert_eq!(id, "new-id"),
        other => panic!("unexpected command: {:?}", other),
    }
}

#[test]
fn metadata_ack_preserves_newer_local_edits_typed_after_dispatch() {
    let mut harness = make_app();
    harness.app.metadata_dirty = true;
    harness.app.edit_name = "Initial Name".to_string();
    harness.app.edit_language = Some("python".to_string());
    harness.app.edit_language_is_manual = true;
    harness.app.edit_tags = "alpha,beta".to_string();
    harness.app.save_metadata_now();
    let _ = recv_cmd(&harness.cmd_rx);
    assert!(harness.app.metadata_save_in_flight);

    harness.app.edit_name = "Locally Newer Name".to_string();
    harness.app.edit_tags = "alpha,beta,gamma".to_string();
    harness.app.metadata_dirty = true;

    let mut ack = Paste::new("content".to_string(), "Initial Name".to_string());
    ack.id = "alpha".to_string();
    ack.language = Some("python".to_string());
    ack.language_is_manual = true;
    ack.tags = vec!["alpha".to_string(), "beta".to_string()];
    harness
        .app
        .apply_event(CoreEvent::PasteMetaSaved { paste: ack });

    assert!(!harness.app.metadata_save_in_flight);
    assert!(harness.app.metadata_save_request.is_none());
    assert!(harness.app.metadata_dirty);
    assert_eq!(harness.app.edit_name, "Locally Newer Name");
    assert_eq!(harness.app.edit_tags, "alpha,beta,gamma");
    assert_eq!(
        harness
            .app
            .selected_paste
            .as_ref()
            .map(|paste| paste.name.as_str()),
        Some("Initial Name")
    );
}

#[test]
fn metadata_ack_clears_dirty_state_when_draft_matches_dispatched_request() {
    let mut harness = make_app();
    harness.app.metadata_dirty = true;
    harness.app.edit_name = "Acked".to_string();
    harness.app.edit_language = Some("rust".to_string());
    harness.app.edit_language_is_manual = true;
    harness.app.edit_tags = "one,two".to_string();
    harness.app.save_metadata_now();
    let _ = recv_cmd(&harness.cmd_rx);
    assert!(harness.app.metadata_save_in_flight);

    let mut ack = Paste::new("content".to_string(), "Acked".to_string());
    ack.id = "alpha".to_string();
    ack.language = Some("rust".to_string());
    ack.language_is_manual = true;
    ack.tags = vec!["one".to_string(), "two".to_string()];
    harness
        .app
        .apply_event(CoreEvent::PasteMetaSaved { paste: ack });

    assert!(!harness.app.metadata_dirty);
    assert!(!harness.app.metadata_save_in_flight);
    assert!(harness.app.metadata_save_request.is_none());
    assert_eq!(harness.app.edit_name, "Acked");
    assert_eq!(harness.app.edit_tags, "one, two");
}

#[test]
fn select_paste_metadata_dirty_defers_switch_until_meta_save_ack() {
    let mut harness = make_app();
    harness
        .app
        .all_pastes
        .push(test_summary("beta", "Beta", None, 4));
    harness.app.pastes = harness.app.all_pastes.clone();
    harness.app.metadata_dirty = true;
    harness.app.edit_name = "Alpha renamed".to_string();
    harness.app.edit_tags = "tag-a".to_string();

    assert!(harness.app.select_paste("beta".to_string()));
    assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));
    assert_eq!(harness.app.pending_selection_id.as_deref(), Some("beta"));

    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::UpdatePasteMeta { id, .. } => assert_eq!(id, "alpha"),
        other => panic!("unexpected command: {:?}", other),
    }

    let mut saved = Paste::new("content".to_string(), "Alpha renamed".to_string());
    saved.id = "alpha".to_string();
    saved.tags = vec!["tag-a".to_string()];
    harness
        .app
        .apply_event(CoreEvent::PasteMetaSaved { paste: saved });

    assert!(harness.app.pending_selection_id.is_none());
    assert_eq!(harness.app.selected_id.as_deref(), Some("beta"));
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::GetPaste { id } => assert_eq!(id, "beta"),
        other => panic!("unexpected command: {:?}", other),
    }
}

#[test]
fn select_paste_rolls_back_pending_when_metadata_dispatch_fails_with_content_in_flight() {
    let TestHarness {
        _dir: _guard,
        mut app,
        cmd_rx,
    } = make_app();
    drop(cmd_rx);

    app.all_pastes.push(test_summary("beta", "Beta", None, 4));
    app.pastes = app.all_pastes.clone();
    app.metadata_dirty = true;
    app.edit_name = "Alpha renamed".to_string();
    app.save_status = SaveStatus::Dirty;
    app.save_in_flight = true;
    app.last_edit_at = Some(Instant::now());

    assert!(!app.select_paste("beta".to_string()));
    assert_eq!(app.selected_id.as_deref(), Some("alpha"));
    assert!(app.pending_selection_id.is_none());
    assert!(matches!(app.save_status, SaveStatus::Dirty));
    assert!(!app.save_in_flight);
    assert!(app.metadata_dirty);
    assert!(!app.metadata_save_in_flight);
    assert_eq!(
        app.status.as_ref().map(|status| status.text.as_str()),
        Some("Metadata save failed: backend unavailable.")
    );
}

#[test]
fn save_error_clears_pending_selection_and_keeps_current_selection() {
    let mut harness = make_app();
    harness
        .app
        .all_pastes
        .push(test_summary("beta", "Beta", None, 4));
    harness.app.pastes = harness.app.all_pastes.clone();
    harness.app.selected_content.reset("edited".to_string());
    harness.app.save_status = SaveStatus::Dirty;
    harness.app.last_edit_at = Some(Instant::now());

    assert!(harness.app.select_paste("beta".to_string()));
    let _ = recv_cmd(&harness.cmd_rx);
    assert_eq!(harness.app.pending_selection_id.as_deref(), Some("beta"));

    harness.app.apply_event(CoreEvent::Error {
        source: CoreErrorSource::SaveContent,
        message: "Update failed: injected".to_string(),
    });

    assert!(harness.app.pending_selection_id.is_none());
    assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));
    assert!(matches!(
        harness.cmd_rx.try_recv(),
        Err(TryRecvError::Empty)
    ));
}

#[test]
fn paste_saved_during_active_search_forces_fresh_backend_search() {
    let mut harness = make_app();
    harness.app.search_query = "alpha".to_string();
    harness.app.search_last_sent = "alpha".to_string();

    let mut saved = Paste::new("updated body".to_string(), "Alpha".to_string());
    saved.id = "alpha".to_string();
    harness
        .app
        .apply_event(CoreEvent::PasteSaved { paste: saved });

    assert!(
        harness.app.search_last_sent.is_empty(),
        "paste save should invalidate cached search query"
    );

    harness.app.maybe_dispatch_search();
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::SearchPastes { query, .. } => assert_eq!(query, "alpha"),
        other => panic!("unexpected command: {:?}", other),
    }
}

#[test]
fn save_now_send_failure_restores_dirty_state() {
    let TestHarness {
        _dir: _guard,
        mut app,
        cmd_rx,
    } = make_app();
    drop(cmd_rx);

    app.selected_content.reset("manual-save".to_string());
    app.save_status = SaveStatus::Dirty;
    app.save_in_flight = false;
    app.save_now();

    assert!(matches!(app.save_status, SaveStatus::Dirty));
    assert!(!app.save_in_flight);
    assert!(app.last_edit_at.is_some());
    assert_eq!(
        app.status.as_ref().map(|status| status.text.as_str()),
        Some("Save failed: backend unavailable.")
    );
}

#[test]
fn autosave_send_failure_restores_dirty_state() {
    let TestHarness {
        _dir: _guard,
        mut app,
        cmd_rx,
    } = make_app();
    drop(cmd_rx);

    app.selected_content.reset("auto-save".to_string());
    app.save_status = SaveStatus::Dirty;
    app.save_in_flight = false;
    app.last_edit_at = Some(Instant::now() - app.autosave_delay - Duration::from_millis(5));
    app.maybe_autosave();

    assert!(matches!(app.save_status, SaveStatus::Dirty));
    assert!(!app.save_in_flight);
    assert!(app.last_edit_at.is_some());
    assert_eq!(
        app.status.as_ref().map(|status| status.text.as_str()),
        Some("Autosave failed: backend unavailable.")
    );
}

#[test]
fn on_exit_dispatches_dirty_save_and_drop_releases_selected_lock() {
    let TestHarness {
        _dir: _guard,
        mut app,
        cmd_rx,
    } = make_app();
    app.selected_content.reset("exit-save-content".to_string());
    app.save_status = SaveStatus::Dirty;
    app.save_in_flight = false;
    app.last_edit_at = Some(Instant::now());
    app.locks.lock("alpha");
    let locks = app.locks.clone();

    eframe::App::on_exit(&mut app, None);

    match recv_cmd(&cmd_rx) {
        CoreCmd::UpdatePaste { id, content } => {
            assert_eq!(id, "alpha");
            assert_eq!(content, "exit-save-content");
        }
        other => panic!("unexpected command: {:?}", other),
    }
    drop(app);
    assert!(
        !locks.is_locked("alpha"),
        "drop should release the selected paste lock"
    );
}
