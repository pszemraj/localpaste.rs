//! Shutdown-flush command emission tests.

use super::*;

#[test]
fn on_exit_dispatches_dirty_content_and_metadata_save_and_drop_releases_selected_lock() {
    let TestHarness {
        _dir: _guard,
        mut app,
        cmd_rx,
    } = make_app();
    app.selected_content.reset("exit-save-content".to_string());
    app.save_status = SaveStatus::Dirty;
    app.save_in_flight = false;
    app.last_edit_at = Some(Instant::now());
    app.edit_name = "exit-name".to_string();
    app.edit_language = Some("rust".to_string());
    app.edit_language_is_manual = true;
    app.edit_tags = "one, two".to_string();
    app.metadata_dirty = true;
    app.metadata_save_in_flight = false;
    app.locks
        .acquire("alpha", &app.lock_owner_id)
        .expect("acquire alpha lock");
    let locks = app.locks.clone();

    eframe::App::on_exit(&mut app, None);

    match recv_cmd(&cmd_rx) {
        CoreCmd::UpdatePaste { id, content } => {
            assert_eq!(id, "alpha");
            assert_eq!(content, "exit-save-content");
        }
        other => panic!("unexpected command: {:?}", other),
    }
    match recv_cmd(&cmd_rx) {
        CoreCmd::UpdatePasteMeta {
            id,
            name,
            language,
            language_is_manual,
            folder_id,
            tags,
        } => {
            assert_eq!(id, "alpha");
            assert_eq!(name.as_deref(), Some("exit-name"));
            assert_eq!(language.as_deref(), Some("rust"));
            assert_eq!(language_is_manual, Some(true));
            assert!(folder_id.is_none());
            assert_eq!(tags, Some(vec!["one".to_string(), "two".to_string()]));
        }
        other => panic!("unexpected command: {:?}", other),
    }
    drop(app);
    assert!(
        !locks.is_locked("alpha").expect("is_locked"),
        "drop should release the selected paste lock"
    );
}

#[test]
fn on_exit_requeues_dirty_tails_after_stale_in_flight_acks() {
    let (mut harness, evt_tx) = make_app_with_event_tx();
    harness
        .app
        .selected_content
        .reset("exit-newer-content".to_string());
    harness.app.save_status = SaveStatus::Dirty;
    harness.app.save_in_flight = true;
    harness.app.save_request_revision = None;
    harness.app.last_edit_at = Some(Instant::now());
    harness.app.edit_name = "new-name".to_string();
    harness.app.edit_language = Some("rust".to_string());
    harness.app.edit_language_is_manual = true;
    harness.app.edit_tags = "one, two".to_string();
    harness.app.metadata_dirty = true;
    harness.app.metadata_save_in_flight = true;
    harness.app.metadata_save_request = Some(MetadataDraftSnapshot {
        name: "old-name".to_string(),
        language: None,
        language_is_manual: false,
        tags_csv: String::new(),
    });

    std::thread::spawn(move || {
        let mut stale_content = Paste::new("stale-content".to_string(), "Alpha".to_string());
        stale_content.id = "alpha".to_string();
        let _ = evt_tx.send(CoreEvent::PasteSaved {
            paste: stale_content,
        });

        let mut stale_meta = Paste::new("stale-content".to_string(), "old-name".to_string());
        stale_meta.id = "alpha".to_string();
        stale_meta.language = None;
        stale_meta.language_is_manual = false;
        stale_meta.tags = Vec::new();
        let _ = evt_tx.send(CoreEvent::PasteMetaSaved { paste: stale_meta });
    });

    eframe::App::on_exit(&mut harness.app, None);

    let first = recv_cmd(&harness.cmd_rx);
    let second = recv_cmd(&harness.cmd_rx);
    let mut saw_content = false;
    let mut saw_metadata = false;

    for cmd in [first, second] {
        match cmd {
            CoreCmd::UpdatePaste { id, content } => {
                assert_eq!(id, "alpha");
                assert_eq!(content, "exit-newer-content");
                saw_content = true;
            }
            CoreCmd::UpdatePasteMeta {
                id,
                name,
                language,
                language_is_manual,
                folder_id,
                tags,
            } => {
                assert_eq!(id, "alpha");
                assert_eq!(name.as_deref(), Some("new-name"));
                assert_eq!(language.as_deref(), Some("rust"));
                assert_eq!(language_is_manual, Some(true));
                assert!(folder_id.is_none());
                assert_eq!(tags, Some(vec!["one".to_string(), "two".to_string()]));
                saw_metadata = true;
            }
            other => panic!("unexpected command: {:?}", other),
        }
    }

    assert!(
        saw_content,
        "expected shutdown to queue a final content save"
    );
    assert!(
        saw_metadata,
        "expected shutdown to queue a final metadata save"
    );
}

#[test]
fn on_exit_forces_final_dirty_snapshots_even_when_acks_never_arrive() {
    let mut harness = make_app();
    harness
        .app
        .selected_content
        .reset("shutdown-final-content".to_string());
    harness.app.save_status = SaveStatus::Dirty;
    harness.app.save_in_flight = true;
    harness.app.save_request_revision = None;
    harness.app.last_edit_at = Some(Instant::now());
    harness.app.edit_name = "shutdown-name".to_string();
    harness.app.edit_language = Some("rust".to_string());
    harness.app.edit_language_is_manual = true;
    harness.app.edit_tags = "one, two".to_string();
    harness.app.metadata_dirty = true;
    harness.app.metadata_save_in_flight = true;
    harness.app.metadata_save_request = Some(MetadataDraftSnapshot {
        name: "old-name".to_string(),
        language: None,
        language_is_manual: false,
        tags_csv: String::new(),
    });

    eframe::App::on_exit(&mut harness.app, None);

    let first = recv_cmd(&harness.cmd_rx);
    let second = recv_cmd(&harness.cmd_rx);
    let mut saw_content = false;
    let mut saw_metadata = false;

    for cmd in [first, second] {
        match cmd {
            CoreCmd::UpdatePaste { id, content } => {
                assert_eq!(id, "alpha");
                assert_eq!(content, "shutdown-final-content");
                saw_content = true;
            }
            CoreCmd::UpdatePasteMeta {
                id,
                name,
                language,
                language_is_manual,
                folder_id,
                tags,
            } => {
                assert_eq!(id, "alpha");
                assert_eq!(name.as_deref(), Some("shutdown-name"));
                assert_eq!(language.as_deref(), Some("rust"));
                assert_eq!(language_is_manual, Some(true));
                assert!(folder_id.is_none());
                assert_eq!(tags, Some(vec!["one".to_string(), "two".to_string()]));
                saw_metadata = true;
            }
            other => panic!("unexpected command: {:?}", other),
        }
    }

    assert!(
        saw_content,
        "expected shutdown to force a final content save without waiting for ack"
    );
    assert!(
        saw_metadata,
        "expected shutdown to force a final metadata save without waiting for ack"
    );
}
