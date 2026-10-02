//! Selection guards preserve drafts and reject stale asynchronous transitions.

use super::*;

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
    assert!(harness.app.select_paste("alpha".into()));
    assert!(harness.app.select_paste("beta".into()));
    for id in ["beta", "alpha", "beta"] {
        assert!(
            matches!(recv_cmd(&harness.cmd_rx), CoreCmd::GetPaste { id: received } if received == id)
        );
    }
    // Backend replies stay in request order. The first beta reply reaches a UI
    // frame before the other two loads complete, and the user starts editing it.
    harness.app.apply_event(CoreEvent::PasteLoaded {
        paste: beta.clone(),
    });
    insert_active_text(&mut harness.app, "new draft ", 0);
    harness.app.mark_dirty();
    harness
        .app
        .apply_event(CoreEvent::PasteLoaded { paste: alpha });
    harness
        .app
        .apply_event(CoreEvent::PasteLoaded { paste: beta });
    assert_eq!(
        harness.app.active_snapshot(),
        "new draft beta body",
        "a duplicate same-id load destroyed the new draft"
    );
    assert_eq!(harness.app.save_status, SaveStatus::Dirty);
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
