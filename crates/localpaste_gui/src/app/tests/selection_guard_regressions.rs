//! Selection guards preserve drafts and reject stale asynchronous transitions.

use super::*;

#[test]
fn revisiting_paste_ignores_snapshot_from_previous_selection_lock() {
    let mut harness = make_app();
    let dir = TempDir::new().expect("temporary database");
    let path = dir.path().join("db");
    let db = Database::new(path.to_str().unwrap()).expect("database");
    for (id, content) in [("alpha", "alpha body"), ("beta", "old beta body")] {
        let mut paste =
            Paste::new_with_language(content.into(), id.into(), Some("text".into()), true);
        paste.id = id.into();
        db.pastes.create(&paste).unwrap();
    }
    let config = Config {
        db_path: path.to_string_lossy().into_owned(),
        port: 0,
        max_paste_size: 10 * 1024 * 1024,
        auto_save_interval: 2000,
        auto_backup: false,
        search_case_sensitive: false,
    };
    let state = AppState::with_locks(config, db.share().unwrap(), harness.app.locks.clone());
    let server = EmbeddedServer::start(state, false).unwrap();
    let mut backend = crate::backend::spawn_backend_with_locks_and_owner(
        db.share().unwrap(),
        10 * 1024 * 1024,
        harness.app.locks.clone(),
        harness.app.lock_owner_id.clone(),
    );
    harness
        .app
        .all_pastes
        .push(test_summary("beta", "Beta", Some("text"), 13));
    harness.app.pastes = harness.app.all_pastes.clone();
    assert!(harness.app.acquire_paste_lock("alpha"));
    let receive = || backend.evt_rx.recv_timeout(Duration::from_secs(5)).unwrap();

    assert!(harness.app.select_paste("beta".into()));
    backend.cmd_tx.send(recv_cmd(&harness.cmd_rx)).unwrap();
    // The real worker read completes, but the UI has not drained its reply yet.
    let old_beta = receive();
    assert!(
        matches!(&old_beta, CoreEvent::PasteLoaded { paste, .. } if paste.content == "old beta body")
    );
    assert!(harness.app.select_paste("alpha".into()));
    backend.cmd_tx.send(recv_cmd(&harness.cmd_rx)).unwrap();
    let alpha = receive();
    assert!(!harness.app.locks.is_locked("beta").unwrap());
    // The embedded API legitimately updates beta while its GUI edit lock is released.
    let url = format!("http://{}/api/paste/beta", server.addr());
    let response = reqwest::blocking::Client::new()
        .put(url)
        .json(&serde_json::json!({ "content": "latest beta body" }))
        .send()
        .unwrap();
    assert!(response.status().is_success());
    assert!(harness.app.select_paste("beta".into()));
    backend.cmd_tx.send(recv_cmd(&harness.cmd_rx)).unwrap();
    let latest_beta = receive();
    assert!(
        matches!(&latest_beta, CoreEvent::PasteLoaded { paste, .. } if paste.content == "latest beta body")
    );

    // Preserve actual FIFO response order; no local edit occurs between replies.
    for reply in [old_beta, alpha, latest_beta] {
        harness.app.apply_event(reply);
    }
    insert_active_text(&mut harness.app, "local edit ", 0);
    harness.app.mark_dirty();
    harness.app.save_now();
    backend.cmd_tx.send(recv_cmd(&harness.cmd_rx)).unwrap();
    assert!(matches!(receive(), CoreEvent::PasteSaved { .. }));
    backend
        .shutdown_and_join(true, Duration::from_secs(5))
        .unwrap();
    drop(backend);
    drop(server);
    drop(db);
    let reopened = Database::new(path.to_str().unwrap()).unwrap();
    let persisted = reopened.pastes.get("beta").unwrap().unwrap().content;
    assert_eq!(
        persisted, "local edit latest beta body",
        "stale selection reply overwrote the API update"
    );
}

#[test]
fn empty_search_preserves_dirty_content() {
    let mut harness = make_app();
    assert!(harness.app.acquire_paste_lock("alpha"));
    let mut paste = Paste::new_with_language(
        "saved text".into(),
        "Alpha".into(),
        Some("text".into()),
        true,
    );
    paste.id = "alpha".into();
    harness.app.select_loaded_paste(paste);
    insert_active_text(&mut harness.app, "unsaved note ", 0);
    harness.app.mark_dirty();
    harness.app.search_query = "no matching paste".into();
    harness.app.search_last_sent = "no matching paste".into();
    harness.app.apply_event(CoreEvent::SearchResults {
        collection: SidebarCollection::All,
        scope: SearchScope::All,
        query: "no matching paste".into(),
        folder_id: None,
        language: None,
        items: vec![],
    });
    assert_eq!(
        harness.app.active_snapshot(),
        "unsaved note saved text",
        "empty search destroyed unsaved content"
    );
    assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));
    assert_eq!(harness.app.save_status, SaveStatus::Dirty);
    assert!(harness.app.locks.is_locked("alpha").unwrap());
    assert!(harness.app.locks.begin_mutation("alpha").is_err());
}

#[test]
fn empty_collection_preserves_metadata_draft() {
    let mut harness = make_app();
    assert!(harness.app.acquire_paste_lock("alpha"));
    harness.app.edit_name = "unsaved title".into();
    harness.app.edit_tags = "unsaved tag".into();
    harness.app.metadata_dirty = true;
    harness
        .app
        .set_active_collection(SidebarCollection::Documents);
    assert_eq!(
        harness.app.edit_name, "unsaved title",
        "empty collection destroyed unsaved metadata"
    );
    assert!(harness.app.metadata_dirty);
    assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));
    assert!(harness.app.locks.is_locked("alpha").unwrap());
}

#[test]
fn empty_list_preserves_inflight_content_and_newer_edit() {
    let mut harness = make_app();
    assert!(harness.app.acquire_paste_lock("alpha"));
    let mut paste = Paste::new_with_language(
        "saved text".into(),
        "Alpha".into(),
        Some("text".into()),
        true,
    );
    paste.id = "alpha".into();
    harness.app.select_loaded_paste(paste);
    insert_active_text(&mut harness.app, "first edit ", 0);
    harness.app.mark_dirty();
    harness.app.save_now();
    assert!(matches!(
        recv_cmd(&harness.cmd_rx),
        CoreCmd::UpdatePasteVirtual { .. }
    ));
    insert_active_text(&mut harness.app, "second edit ", 0);
    harness.app.mark_dirty();
    harness.app.active_collection = SidebarCollection::Documents;
    harness.app.apply_event(CoreEvent::PasteList {
        items: vec![test_summary("alpha", "Alpha", Some("text"), 10)],
    });
    assert_eq!(
        harness.app.active_snapshot(),
        "second edit first edit saved text",
        "empty filtered list destroyed edits made during save"
    );
    assert!(harness.app.save_in_flight);
    assert_eq!(harness.app.save_status, SaveStatus::Dirty);
    assert!(harness.app.locks.is_locked("alpha").unwrap());
}

#[test]
fn empty_collection_preserves_inflight_metadata_request() {
    let mut harness = make_app();
    assert!(harness.app.acquire_paste_lock("alpha"));
    harness.app.edit_name = "title being saved".into();
    harness.app.metadata_dirty = true;
    harness.app.save_metadata_now();
    assert!(matches!(
        recv_cmd(&harness.cmd_rx),
        CoreCmd::UpdatePasteMeta { .. }
    ));
    harness.app.edit_name = "newer unsaved title".into();
    harness
        .app
        .set_active_collection(SidebarCollection::Documents);
    assert_eq!(harness.app.edit_name, "newer unsaved title");
    assert!(harness.app.metadata_save_in_flight);
    assert!(harness.app.metadata_save_request.is_some());
    assert!(harness.app.locks.is_locked("alpha").unwrap());
}

#[test]
fn selecting_active_paste_cancels_pending_switch() {
    let mut harness = make_app();
    harness
        .app
        .all_pastes
        .push(test_summary("beta", "Beta", Some("text"), 10));
    harness.app.pastes = harness.app.all_pastes.clone();
    let mut alpha = Paste::new_with_language(
        "saved text".into(),
        "Alpha".into(),
        Some("text".into()),
        true,
    );
    alpha.id = "alpha".into();
    harness.app.select_loaded_paste(alpha.clone());
    insert_active_text(&mut harness.app, "edit ", 0);
    harness.app.mark_dirty();
    assert!(harness.app.select_paste("beta".into()));
    assert!(matches!(
        recv_cmd(&harness.cmd_rx),
        CoreCmd::UpdatePasteVirtual { .. }
    ));
    assert_eq!(harness.app.pending_selection_id.as_deref(), Some("beta"));
    // The user's second selection is the currently displayed paste.
    assert!(harness.app.select_paste("alpha".into()));
    alpha.content = "edit saved text".into();
    harness
        .app
        .apply_event(CoreEvent::PasteSaved { paste: alpha });
    assert_eq!(
        harness.app.selected_id.as_deref(),
        Some("alpha"),
        "older deferred switch overrides the user's latest selection"
    );
    assert!(harness.app.pending_selection_id.is_none());
}

#[test]
fn repeated_target_load_does_not_replace_new_draft() {
    let mut harness = make_app();
    harness
        .app
        .all_pastes
        .push(test_summary("beta", "Beta", Some("text"), 10));
    harness.app.pastes = harness.app.all_pastes.clone();
    let mut alpha = Paste::new_with_language(
        "alpha body".into(),
        "Alpha".into(),
        Some("text".into()),
        true,
    );
    alpha.id = "alpha".into();
    harness.app.select_loaded_paste(alpha.clone());
    let mut beta =
        Paste::new_with_language("beta body".into(), "Beta".into(), Some("text".into()), true);
    beta.id = "beta".into();
    assert!(harness.app.select_paste("beta".into()));
    let old_beta_epoch = harness.app.active_buffer_epoch;
    assert!(harness.app.select_paste("alpha".into()));
    let alpha_epoch = harness.app.active_buffer_epoch;
    assert!(harness.app.select_paste("beta".into()));
    let beta_epoch = harness.app.active_buffer_epoch;
    for id in ["beta", "alpha", "beta"] {
        assert!(
            matches!(recv_cmd(&harness.cmd_rx), CoreCmd::GetPaste { id: received, .. } if received == id)
        );
    }
    // Older requests cannot initialize the revisited selection.
    harness.app.apply_event(CoreEvent::PasteLoaded {
        paste: beta.clone(),
        selection_epoch: old_beta_epoch,
    });
    assert!(harness.app.selected_paste.is_none());
    harness.app.apply_event(CoreEvent::PasteLoaded {
        paste: alpha,
        selection_epoch: alpha_epoch,
    });
    harness.app.apply_event(CoreEvent::PasteLoaded {
        paste: beta.clone(),
        selection_epoch: beta_epoch,
    });
    insert_active_text(&mut harness.app, "new draft ", 0);
    harness.app.mark_dirty();
    harness.app.apply_event(CoreEvent::PasteLoaded {
        paste: beta,
        selection_epoch: beta_epoch,
    });
    assert_eq!(
        harness.app.active_snapshot(),
        "new draft beta body",
        "a duplicate same-id load destroyed the new draft"
    );
    assert_eq!(harness.app.save_status, SaveStatus::Dirty);
}

#[test]
fn stale_selection_failures_preserve_revisited_paste_and_its_draft() {
    for missing in [false, true] {
        let mut harness = make_app();
        harness
            .app
            .all_pastes
            .push(test_summary("beta", "Beta", Some("text"), 9));
        harness.app.pastes = harness.app.all_pastes.clone();
        assert!(harness.app.select_paste("beta".into()));
        let old_epoch = harness.app.active_buffer_epoch;
        assert!(harness.app.select_paste("alpha".into()));
        assert!(harness.app.select_paste("beta".into()));
        let current_epoch = harness.app.active_buffer_epoch;
        let mut beta = Paste::new("beta body".into(), "Beta".into());
        beta.id = "beta".into();
        harness.app.apply_event(CoreEvent::PasteLoaded {
            paste: beta,
            selection_epoch: current_epoch,
        });
        insert_active_text(&mut harness.app, "draft ", 0);
        harness.app.mark_dirty();
        // Cover both an older request and a duplicate outcome after initialization.
        for selection_epoch in [old_epoch, current_epoch] {
            let event = if missing {
                CoreEvent::PasteSelectionMissing {
                    id: "beta".into(),
                    selection_epoch,
                }
            } else {
                CoreEvent::PasteLoadFailed {
                    id: "beta".into(),
                    selection_epoch,
                    message: "late failure".into(),
                }
            };
            harness.app.apply_event(event);
            assert_eq!(harness.app.selected_id.as_deref(), Some("beta"));
            assert_eq!(harness.app.active_snapshot(), "draft beta body");
            assert_eq!(harness.app.save_status, SaveStatus::Dirty);
            assert!(harness.app.locks.is_locked("beta").unwrap());
            assert!(harness
                .app
                .all_pastes
                .iter()
                .any(|paste| paste.id == "beta"));
        }
    }
}

#[test]
fn empty_collection_clears_saved_selection_and_releases_edit_lock() {
    let mut harness = make_app();
    assert!(harness.app.acquire_paste_lock("alpha"));
    harness
        .app
        .set_active_collection(SidebarCollection::Documents);
    assert!(harness.app.selected_id.is_none());
    assert!(harness.app.active_snapshot().is_empty());
    assert!(!harness.app.locks.is_locked("alpha").unwrap());
}

#[test]
fn hidden_content_draft_remains_locked_until_the_latest_save_ack() {
    let mut harness = make_app();
    assert!(harness.app.acquire_paste_lock("alpha"));
    let mut alpha =
        Paste::new_with_language("original".into(), "Alpha".into(), Some("text".into()), true);
    alpha.id = "alpha".into();
    harness.app.select_loaded_paste(alpha.clone());
    insert_active_text(&mut harness.app, "first ", 0);
    harness.app.mark_dirty();
    harness.app.save_now();
    assert!(matches!(
        recv_cmd(&harness.cmd_rx),
        CoreCmd::UpdatePasteVirtual { .. }
    ));
    insert_active_text(&mut harness.app, "second ", 0);
    harness.app.mark_dirty();
    harness.app.set_active_language_filter(Some("rust".into()));
    alpha.content = "first original".into();
    harness.app.apply_event(CoreEvent::PasteSaved {
        paste: alpha.clone(),
    });
    assert_eq!(harness.app.active_snapshot(), "second first original");
    assert_eq!(harness.app.save_status, SaveStatus::Dirty);
    assert!(harness.app.locks.is_locked("alpha").unwrap());
    harness.app.save_now();
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::UpdatePasteVirtual { id, content, .. } => {
            assert_eq!(id, "alpha");
            assert_eq!(content.to_string(), "second first original");
        }
        other => panic!("expected retained draft save, got {other:?}"),
    }
    alpha.content = "second first original".into();
    harness
        .app
        .apply_event(CoreEvent::PasteSaved { paste: alpha });
    assert!(harness.app.selected_id.is_none());
    assert_eq!(harness.app.save_status, SaveStatus::Saved);
    assert!(!harness.app.locks.is_locked("alpha").unwrap());
}

#[test]
fn hidden_metadata_draft_remains_locked_until_the_latest_save_ack() {
    let mut harness = make_app();
    assert!(harness.app.acquire_paste_lock("alpha"));
    let mut alpha =
        Paste::new_with_language("original".into(), "Alpha".into(), Some("text".into()), true);
    alpha.id = "alpha".into();
    harness.app.select_loaded_paste(alpha.clone());
    harness.app.edit_name = "first title".into();
    harness.app.metadata_dirty = true;
    harness.app.save_metadata_now();
    assert!(matches!(
        recv_cmd(&harness.cmd_rx),
        CoreCmd::UpdatePasteMeta { .. }
    ));
    harness.app.edit_name = "latest title".into();
    harness.app.set_active_language_filter(Some("rust".into()));
    alpha.name = "first title".into();
    harness.app.apply_event(CoreEvent::PasteMetaSaved {
        paste: alpha.clone(),
    });
    assert_eq!(harness.app.edit_name, "latest title");
    assert!(harness.app.metadata_dirty);
    assert!(harness.app.locks.is_locked("alpha").unwrap());
    harness.app.save_metadata_now();
    assert!(
        matches!(recv_cmd(&harness.cmd_rx), CoreCmd::UpdatePasteMeta { name: Some(name), .. } if name == "latest title")
    );
    alpha.name = "latest title".into();
    harness
        .app
        .apply_event(CoreEvent::PasteMetaSaved { paste: alpha });
    assert!(harness.app.selected_id.is_none());
    assert!(!harness.app.metadata_dirty);
    assert!(!harness.app.locks.is_locked("alpha").unwrap());
}
