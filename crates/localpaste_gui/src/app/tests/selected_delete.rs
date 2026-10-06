//! Selected-paste delete flow tests.

use super::*;
use crate::backend::CoreErrorSource;

#[test]
fn delete_selected_with_dirty_content_saves_before_delete() {
    let mut harness = make_app();
    set_active_content(&mut harness.app, "new unsaved text");
    harness.app.save_status = SaveStatus::Dirty;

    harness.app.delete_selected();

    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::UpdatePasteVirtual { id, content, .. } => {
            assert_eq!(id, "alpha");
            assert_eq!(content.to_string(), "new unsaved text");
        }
        other => panic!("expected content save before delete, got {:?}", other),
    }
    assert!(matches!(
        harness.cmd_rx.try_recv(),
        Err(TryRecvError::Empty)
    ));

    let mut saved = Paste::new("new unsaved text".to_string(), "Alpha".to_string());
    saved.id = "alpha".to_string();
    harness
        .app
        .apply_event(CoreEvent::PasteSaved { paste: saved });

    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::DeletePaste { id } => assert_eq!(id, "alpha"),
        other => panic!("expected delete after content save, got {:?}", other),
    }
}

#[test]
fn delete_selected_with_dirty_metadata_saves_before_delete() {
    let mut harness = make_app();
    harness.app.metadata_dirty = true;
    harness.app.edit_name = "Renamed".to_string();
    harness.app.edit_language = Some("rust".to_string());
    harness.app.edit_language_is_manual = true;
    harness.app.edit_tags = "one, two".to_string();

    harness.app.delete_selected();

    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::UpdatePasteMeta {
            id,
            name,
            language,
            language_is_manual,
            tags,
            ..
        } => {
            assert_eq!(id, "alpha");
            assert_eq!(name.as_deref(), Some("Renamed"));
            assert_eq!(language.as_deref(), Some("rust"));
            assert_eq!(language_is_manual, Some(true));
            assert_eq!(tags, Some(vec!["one".to_string(), "two".to_string()]));
        }
        other => panic!("expected metadata save before delete, got {:?}", other),
    }
    assert!(matches!(
        harness.cmd_rx.try_recv(),
        Err(TryRecvError::Empty)
    ));

    let mut saved = Paste::new_with_language(
        "content".to_string(),
        "Renamed".to_string(),
        Some("rust".to_string()),
        true,
    );
    saved.id = "alpha".to_string();
    saved.tags = vec!["one".to_string(), "two".to_string()];
    harness
        .app
        .apply_event(CoreEvent::PasteMetaSaved { paste: saved });

    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::DeletePaste { id } => assert_eq!(id, "alpha"),
        other => panic!("expected delete after metadata save, got {:?}", other),
    }
}

#[test]
fn delete_selected_with_content_and_metadata_dirty_waits_for_both_acks() {
    let mut harness = make_app();
    set_active_content(&mut harness.app, "dirty content");
    harness.app.save_status = SaveStatus::Dirty;
    harness.app.metadata_dirty = true;
    harness.app.edit_name = "Dirty Meta".to_string();

    harness.app.delete_selected();

    let first = recv_cmd(&harness.cmd_rx);
    let second = recv_cmd(&harness.cmd_rx);
    assert!(matches!(first, CoreCmd::UpdatePasteVirtual { .. }));
    assert!(matches!(second, CoreCmd::UpdatePasteMeta { .. }));
    assert!(matches!(
        harness.cmd_rx.try_recv(),
        Err(TryRecvError::Empty)
    ));

    let mut content_saved = Paste::new("dirty content".to_string(), "Alpha".to_string());
    content_saved.id = "alpha".to_string();
    harness.app.apply_event(CoreEvent::PasteSaved {
        paste: content_saved,
    });
    assert!(matches!(
        harness.cmd_rx.try_recv(),
        Err(TryRecvError::Empty)
    ));

    let mut meta_saved = Paste::new("dirty content".to_string(), "Dirty Meta".to_string());
    meta_saved.id = "alpha".to_string();
    harness
        .app
        .apply_event(CoreEvent::PasteMetaSaved { paste: meta_saved });

    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::DeletePaste { id } => assert_eq!(id, "alpha"),
        other => panic!("expected delete after both saves, got {:?}", other),
    }
}

#[test]
fn delete_selected_with_in_flight_stale_save_dispatches_newer_save_before_delete() {
    let mut harness = make_app();
    set_active_content(&mut harness.app, "newer local edit");
    harness.app.save_status = SaveStatus::Dirty;
    harness.app.save_in_flight = true;

    harness.app.delete_selected();
    assert!(matches!(
        harness.cmd_rx.try_recv(),
        Err(TryRecvError::Empty)
    ));

    let mut stale_saved = Paste::new("older saved edit".to_string(), "Alpha".to_string());
    stale_saved.id = "alpha".to_string();
    harness
        .app
        .apply_event(CoreEvent::PasteSaved { paste: stale_saved });

    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::UpdatePasteVirtual { id, content, .. } => {
            assert_eq!(id, "alpha");
            assert_eq!(content.to_string(), "newer local edit");
        }
        other => panic!(
            "expected second content save before delete, got {:?}",
            other
        ),
    }

    let mut latest_saved = Paste::new("newer local edit".to_string(), "Alpha".to_string());
    latest_saved.id = "alpha".to_string();
    harness.app.apply_event(CoreEvent::PasteSaved {
        paste: latest_saved,
    });

    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::DeletePaste { id } => assert_eq!(id, "alpha"),
        other => panic!("expected delete after newer save, got {:?}", other),
    }
}

#[test]
fn palette_delete_selected_uses_deferred_save_but_nonselected_delete_is_immediate() {
    let (mut selected, _selected_evt_tx) = make_app_with_event_tx();
    selected.app.paste_picker_open = true;
    set_active_content(&mut selected.app, "palette dirty");
    selected.app.save_status = SaveStatus::Dirty;

    let selected_ctx = egui::Context::default();
    selected
        .app
        .send_palette_delete(&selected_ctx, "alpha".to_string());

    assert!(
        selected.app.paste_picker_open,
        "typing needs a safe destination during deferred deletion"
    );
    match recv_cmd(&selected.cmd_rx) {
        CoreCmd::UpdatePasteVirtual { id, content, .. } => {
            assert_eq!(id, "alpha");
            assert_eq!(content.to_string(), "palette dirty");
        }
        other => panic!(
            "expected selected palette delete to save first, got {:?}",
            other
        ),
    }

    run_full_update(&mut selected.app, &selected_ctx, vec![]);
    run_full_update(
        &mut selected.app,
        &selected_ctx,
        vec![egui::Event::Text("keep".into())],
    );
    assert_eq!(selected.app.paste_picker_query, "keep");
    assert_eq!(selected.app.active_snapshot(), "palette dirty");
    let mut saved = Paste::new("palette dirty".into(), "Alpha".into());
    saved.id = "alpha".into();
    selected
        .app
        .apply_event(CoreEvent::PasteSaved { paste: saved });
    assert!(selected.app.picker_delete_transition_active());
    assert!(selected
        .cmd_rx
        .try_iter()
        .any(|cmd| matches!(cmd, CoreCmd::DeletePaste { id } if id == "alpha")));
    selected.app.palette_search_results = vec![test_summary("alpha", "Alpha", None, 7)];
    selected.app.apply_event(CoreEvent::PasteDeleted {
        id: "alpha".into(),
        undo_token: None,
    });
    run_full_update(
        &mut selected.app,
        &selected_ctx,
        vec![egui::Event::Text(" typing".into())],
    );
    assert_eq!(selected.app.paste_picker_query, "keep typing");
    assert!(selected
        .app
        .palette_search_results
        .iter()
        .all(|paste| paste.id != "alpha"));

    let mut nonselected = make_app();
    let ctx = egui::Context::default();
    nonselected.app.paste_picker_open = true;
    set_active_content(&mut nonselected.app, "dirty selected");
    nonselected.app.save_status = SaveStatus::Dirty;

    nonselected
        .app
        .send_palette_delete(&egui::Context::default(), "beta".to_string());

    match recv_cmd(&nonselected.cmd_rx) {
        CoreCmd::DeletePaste { id } => assert_eq!(id, "beta"),
        other => panic!(
            "expected non-selected palette delete immediately, got {:?}",
            other
        ),
    }
    nonselected.app.apply_event(CoreEvent::PasteDeleted {
        id: "beta".into(),
        undo_token: Some("undo-beta".into()),
    });
    assert_eq!(nonselected.app.selected_id.as_deref(), Some("alpha"));
    assert_eq!(nonselected.app.active_snapshot(), "dirty selected");
    assert_eq!(nonselected.app.save_status, SaveStatus::Dirty);
    run_full_update(&mut nonselected.app, &ctx, vec![]);
    assert!(ctx.memory(|memory| memory.has_focus(egui::Id::new(VIRTUAL_EDITOR_ID))));
    run_full_update(
        &mut nonselected.app,
        &ctx,
        vec![egui::Event::Text("R".into())],
    );
    assert_eq!(nonselected.app.active_snapshot(), "Rdirty selected");
    let mut sidebar = make_app();
    run_full_update(&mut sidebar.app, &ctx, vec![]);
    ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(SEARCH_INPUT_ID)));
    sidebar.app.remember_discovery_focus(&ctx);
    sidebar.app.open_paste_picker();
    run_full_update(&mut sidebar.app, &ctx, vec![]);
    sidebar.app.send_palette_delete(&ctx, "beta".into());
    run_full_update(
        &mut sidebar.app,
        &ctx,
        vec![egui::Event::Text("search".into())],
    );
    assert_eq!(sidebar.app.search_query, "search");
    assert_eq!(sidebar.app.active_snapshot(), "content");
}

#[test]
fn selected_picker_delete_owns_escape_before_ack_and_replacement_load() {
    let (mut harness, _evt_tx) = make_app_with_event_tx();
    harness
        .app
        .all_pastes
        .push(test_summary("beta", "Beta", None, 12));
    harness.app.pastes = harness.app.all_pastes.clone();
    harness.app.paste_picker_open = true;
    let ctx = egui::Context::default();
    run_full_update(&mut harness.app, &ctx, vec![]);

    harness.app.send_palette_delete(&ctx, "alpha".to_string());
    assert!(matches!(
        recv_cmd(&harness.cmd_rx),
        CoreCmd::DeletePaste { id } if id == "alpha"
    ));
    assert!(harness.app.picker_delete_transition_active());
    harness.app.delete_selected();
    harness.app.create_new_paste();
    assert!(
        matches!(harness.cmd_rx.try_recv(), Err(TryRecvError::Empty)),
        "the accepted picker delete must be the only mutation dispatched"
    );
    harness.app.open_diff_modal();
    assert!(!harness.app.version_overlay_open());
    run_full_update(
        &mut harness.app,
        &ctx,
        vec![key_event(egui::Key::F1, egui::Modifiers::NONE)],
    );
    assert!(harness.app.paste_picker_open);
    assert!(!harness.app.shortcut_help_open);

    run_full_update(
        &mut harness.app,
        &ctx,
        vec![
            key_event(egui::Key::Escape, egui::Modifiers::NONE),
            egui::Event::Text("before".into()),
        ],
    );
    run_full_update(&mut harness.app, &ctx, vec![]);
    assert!(harness.app.paste_picker_open);
    assert_eq!(harness.app.paste_picker_query, "before");
    assert!(!harness.app.select_paste("beta".to_string()));
    assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));

    harness.app.apply_event(CoreEvent::PasteDeleted {
        id: "alpha".into(),
        undo_token: None,
    });
    let replacement_epoch = harness
        .cmd_rx
        .try_iter()
        .find_map(|command| match command {
            CoreCmd::GetPaste {
                id,
                selection_epoch,
            } if id == "beta" => Some(selection_epoch),
            _ => None,
        })
        .expect("adjacent paste load");
    assert_eq!(harness.app.selected_id.as_deref(), Some("beta"));
    assert!(harness.app.picker_delete_transition_active());

    run_full_update(
        &mut harness.app,
        &ctx,
        vec![
            key_event(egui::Key::Escape, egui::Modifiers::NONE),
            egui::Event::Text("-after".into()),
        ],
    );
    run_full_update(&mut harness.app, &ctx, vec![]);
    assert!(harness.app.paste_picker_open);
    assert_eq!(harness.app.paste_picker_query, "before-after");

    let mut beta = Paste::new("beta content".into(), "Beta".into());
    beta.id = "beta".into();
    harness.app.apply_event(CoreEvent::PasteLoaded {
        paste: beta,
        selection_epoch: replacement_epoch,
    });
    assert!(!harness.app.picker_delete_transition_active());
    assert!(harness.app.paste_picker_open);
    run_full_update(
        &mut harness.app,
        &ctx,
        vec![key_event(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert!(!harness.app.paste_picker_open);
}

#[test]
fn selected_picker_delete_ignores_a_dying_paste_load_before_delete_ack() {
    let mut harness = make_app();
    harness
        .app
        .all_pastes
        .push(test_summary("beta", "Beta", None, 12));
    harness.app.pastes = harness.app.all_pastes.clone();
    harness.app.paste_picker_open = true;
    harness
        .app
        .send_palette_delete(&egui::Context::default(), "alpha".into());
    assert!(matches!(
        recv_cmd(&harness.cmd_rx),
        CoreCmd::DeletePaste { id } if id == "alpha"
    ));

    let mut dying = Paste::new("late alpha body".into(), "Alpha".into());
    dying.id = "alpha".into();
    let dying_epoch = harness.app.active_buffer_epoch;
    harness.app.apply_event(CoreEvent::PasteLoaded {
        paste: dying,
        selection_epoch: dying_epoch,
    });

    assert!(
        harness.app.picker_delete_transition_active(),
        "a load reply for the paste being deleted must not release deletion ownership"
    );
    assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));

    harness.app.apply_event(CoreEvent::PasteDeleted {
        id: "alpha".into(),
        undo_token: None,
    });
    assert_eq!(harness.app.selected_id.as_deref(), Some("beta"));
    assert!(harness.app.picker_delete_transition_active());
    assert!(harness
        .cmd_rx
        .try_iter()
        .any(|command| matches!(command, CoreCmd::GetPaste { id, .. } if id == "beta")));
}

#[test]
fn selected_picker_delete_prefers_the_selection_queued_before_delete() {
    let mut harness = make_app();
    harness
        .app
        .all_pastes
        .push(test_summary("beta", "Beta", None, 12));
    harness.app.pastes = harness.app.all_pastes.clone();
    harness.app.palette_search_results = vec![test_summary("gamma", "Gamma", None, 14)];
    set_active_content(&mut harness.app, "dirty alpha");
    harness.app.save_status = SaveStatus::Dirty;

    assert!(harness.app.select_paste("gamma".into()));
    assert_eq!(harness.app.pending_selection_id.as_deref(), Some("gamma"));
    assert!(matches!(
        recv_cmd(&harness.cmd_rx),
        CoreCmd::UpdatePasteVirtual { id, .. } if id == "alpha"
    ));

    harness.app.paste_picker_open = true;
    harness
        .app
        .send_palette_delete(&egui::Context::default(), "alpha".into());
    assert!(harness.app.picker_delete_transition_active());
    assert_eq!(harness.app.pending_selection_id.as_deref(), Some("gamma"));

    let mut saved = Paste::new("dirty alpha".into(), "Alpha".into());
    saved.id = "alpha".into();
    harness
        .app
        .apply_event(CoreEvent::PasteSaved { paste: saved });
    assert!(matches!(
        recv_cmd(&harness.cmd_rx),
        CoreCmd::DeletePaste { id } if id == "alpha"
    ));
    assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));
    assert_eq!(harness.app.pending_selection_id.as_deref(), Some("gamma"));

    harness.app.apply_event(CoreEvent::PasteDeleted {
        id: "alpha".into(),
        undo_token: None,
    });
    let replacement_epoch = harness
        .cmd_rx
        .try_iter()
        .find_map(|command| match command {
            CoreCmd::GetPaste {
                id,
                selection_epoch,
            } if id == "gamma" => Some(selection_epoch),
            _ => None,
        })
        .expect("queued user selection should become the replacement load");
    assert_eq!(harness.app.selected_id.as_deref(), Some("gamma"));
    assert!(harness.app.picker_delete_transition_active());

    let mut gamma = Paste::new("gamma body".into(), "Gamma".into());
    gamma.id = "gamma".into();
    harness.app.apply_event(CoreEvent::PasteLoaded {
        paste: gamma,
        selection_epoch: replacement_epoch,
    });
    assert!(!harness.app.picker_delete_transition_active());
    assert_eq!(harness.app.selected_id.as_deref(), Some("gamma"));
    assert_eq!(harness.app.active_snapshot(), "gamma body");
}

#[test]
fn command_v_during_selected_picker_delete_pastes_into_query_without_blocked_status() {
    let (mut harness, _evt_tx) = make_app_with_event_tx();
    harness.app.paste_picker_open = true;
    let ctx = egui::Context::default();
    run_full_update(&mut harness.app, &ctx, vec![]);
    harness.app.send_palette_delete(&ctx, "alpha".into());
    assert!(matches!(
        recv_cmd(&harness.cmd_rx),
        CoreCmd::DeletePaste { id } if id == "alpha"
    ));
    harness.app.status = None;

    run_full_update(
        &mut harness.app,
        &ctx,
        vec![
            command_key_event(egui::Key::V),
            egui::Event::Paste("query paste".into()),
        ],
    );

    assert_eq!(harness.app.paste_picker_query, "query paste");
    assert!(harness.app.picker_delete_transition_active());
    assert!(harness.app.status.is_none());
    assert!(!harness
        .cmd_rx
        .try_iter()
        .any(|command| matches!(command, CoreCmd::CreatePaste { .. })));
}

#[test]
fn selected_picker_delete_failure_releases_ownership_fence() {
    let mut harness = make_app();
    harness.app.paste_picker_open = true;
    let ctx = egui::Context::default();
    harness.app.send_palette_delete(&ctx, "alpha".to_string());
    assert!(matches!(
        recv_cmd(&harness.cmd_rx),
        CoreCmd::DeletePaste { .. }
    ));

    harness.app.apply_event(CoreEvent::PasteDeleteFailed {
        id: "alpha".into(),
        message: "Delete failed: locked.".into(),
    });

    assert!(!harness.app.picker_delete_transition_active());
    assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));
    run_full_update(
        &mut harness.app,
        &ctx,
        vec![key_event(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert!(!harness.app.paste_picker_open);
}

#[test]
fn selected_picker_delete_immediate_dispatch_failure_does_not_start_a_fence() {
    let mut harness = make_app();
    harness.app.paste_picker_open = true;
    let (_replacement_tx, replacement_rx) = unbounded();
    let live_rx = std::mem::replace(&mut harness.cmd_rx, replacement_rx);
    drop(live_rx);

    harness
        .app
        .send_palette_delete(&egui::Context::default(), "alpha".into());

    assert!(!harness.app.picker_delete_transition_active());
    assert!(harness.app.paste_picker_open);
    assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));
    assert_eq!(
        harness
            .app
            .status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some("Delete failed: backend unavailable.")
    );
}

#[test]
fn selected_picker_delete_dispatch_failure_after_save_releases_ownership_fence() {
    let mut harness = make_app();
    harness.app.paste_picker_open = true;
    set_active_content(&mut harness.app, "dirty content");
    harness.app.save_status = SaveStatus::Dirty;
    harness
        .app
        .send_palette_delete(&egui::Context::default(), "alpha".into());
    assert!(matches!(
        recv_cmd(&harness.cmd_rx),
        CoreCmd::UpdatePasteVirtual { .. }
    ));
    assert!(harness.app.picker_delete_transition_active());
    let (_replacement_tx, replacement_rx) = unbounded();
    let live_rx = std::mem::replace(&mut harness.cmd_rx, replacement_rx);
    drop(live_rx);

    let mut saved = Paste::new("dirty content".into(), "Alpha".into());
    saved.id = "alpha".into();
    harness
        .app
        .apply_event(CoreEvent::PasteSaved { paste: saved });

    assert!(harness.app.pending_delete_id.is_none());
    assert!(!harness.app.picker_delete_transition_active());
    assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));
}

#[test]
fn selected_picker_delete_failed_save_requeue_releases_ownership_fence() {
    let mut harness = make_app();
    harness.app.paste_picker_open = true;
    set_active_content(&mut harness.app, "newer local edit");
    harness.app.save_status = SaveStatus::Dirty;
    harness.app.save_in_flight = true;
    harness
        .app
        .send_palette_delete(&egui::Context::default(), "alpha".into());
    assert!(harness.app.picker_delete_transition_active());
    assert_eq!(harness.app.pending_delete_id.as_deref(), Some("alpha"));

    let (_replacement_tx, replacement_rx) = unbounded();
    let live_rx = std::mem::replace(&mut harness.cmd_rx, replacement_rx);
    drop(live_rx);

    let mut stale_saved = Paste::new("older saved edit".into(), "Alpha".into());
    stale_saved.id = "alpha".into();
    harness
        .app
        .apply_event(CoreEvent::PasteSaved { paste: stale_saved });

    assert!(harness.app.pending_delete_id.is_none());
    assert!(!harness.app.picker_delete_transition_active());
    assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));
    assert_eq!(harness.app.active_snapshot(), "newer local edit");
    assert_eq!(harness.app.save_status, SaveStatus::Dirty);
    assert_eq!(
        harness
            .app
            .status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some("Delete cancelled because current paste could not be saved.")
    );
}

#[test]
fn selected_picker_replacement_load_failure_releases_ownership_fence() {
    let mut harness = make_app();
    harness
        .app
        .all_pastes
        .push(test_summary("beta", "Beta", None, 12));
    harness.app.pastes = harness.app.all_pastes.clone();
    harness.app.paste_picker_open = true;
    let ctx = egui::Context::default();
    harness.app.send_palette_delete(&ctx, "alpha".to_string());
    let _ = recv_cmd(&harness.cmd_rx);
    harness.app.apply_event(CoreEvent::PasteDeleted {
        id: "alpha".into(),
        undo_token: None,
    });
    let replacement_epoch = harness
        .cmd_rx
        .try_iter()
        .find_map(|command| match command {
            CoreCmd::GetPaste {
                id,
                selection_epoch,
            } if id == "beta" => Some(selection_epoch),
            _ => None,
        })
        .expect("adjacent paste load");

    harness.app.apply_event(CoreEvent::PasteLoadFailed {
        id: "beta".into(),
        selection_epoch: replacement_epoch,
        message: "Load failed: disk error.".into(),
    });

    assert!(!harness.app.picker_delete_transition_active());
    assert!(harness.app.paste_picker_open);
    run_full_update(
        &mut harness.app,
        &ctx,
        vec![key_event(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert!(!harness.app.paste_picker_open);
}

#[test]
fn save_error_cancels_pending_delete_and_preserves_dirty_selected_state() {
    let mut harness = make_app();
    set_active_content(&mut harness.app, "still dirty");
    harness.app.save_status = SaveStatus::Dirty;

    harness.app.paste_picker_open = true;
    harness
        .app
        .send_palette_delete(&egui::Context::default(), "alpha".into());
    assert!(matches!(
        recv_cmd(&harness.cmd_rx),
        CoreCmd::UpdatePasteVirtual { .. }
    ));

    harness.app.apply_event(CoreEvent::Error {
        source: CoreErrorSource::SaveContent,
        message: "Save failed: disk full.".to_string(),
    });

    assert!(harness.app.pending_delete_id.is_none());
    assert!(!harness.app.picker_delete_transition_active());
    assert!(matches!(harness.app.save_status, SaveStatus::Dirty));
    assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));
    assert!(matches!(
        harness.cmd_rx.try_recv(),
        Err(TryRecvError::Empty)
    ));
}

#[test]
fn metadata_save_error_cancels_pending_delete_and_preserves_dirty_selected_state() {
    let mut harness = make_app();
    harness.app.metadata_dirty = true;
    harness.app.edit_name = "Still Dirty".to_string();

    harness.app.paste_picker_open = true;
    harness
        .app
        .send_palette_delete(&egui::Context::default(), "alpha".into());
    assert!(matches!(
        recv_cmd(&harness.cmd_rx),
        CoreCmd::UpdatePasteMeta { .. }
    ));

    harness.app.apply_event(CoreEvent::Error {
        source: CoreErrorSource::SaveMetadata,
        message: "Metadata save failed: disk full.".to_string(),
    });

    assert!(harness.app.pending_delete_id.is_none());
    assert!(!harness.app.picker_delete_transition_active());
    assert!(harness.app.metadata_dirty);
    assert!(!harness.app.metadata_save_in_flight);
    assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));
    assert!(matches!(
        harness.cmd_rx.try_recv(),
        Err(TryRecvError::Empty)
    ));
}
