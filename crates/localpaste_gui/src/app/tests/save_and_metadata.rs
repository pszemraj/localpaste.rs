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
    assert_eq!(
        harness
            .app
            .selected_paste
            .as_ref()
            .map(|paste| paste.content.as_str()),
        Some("edited-during-save")
    );
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
