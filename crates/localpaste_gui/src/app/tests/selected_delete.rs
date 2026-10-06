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
    let mut selected = make_app();
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
fn save_error_cancels_pending_delete_and_preserves_dirty_selected_state() {
    let mut harness = make_app();
    set_active_content(&mut harness.app, "still dirty");
    harness.app.save_status = SaveStatus::Dirty;

    harness.app.delete_selected();
    assert!(matches!(
        recv_cmd(&harness.cmd_rx),
        CoreCmd::UpdatePasteVirtual { .. }
    ));

    harness.app.apply_event(CoreEvent::Error {
        source: CoreErrorSource::SaveContent,
        message: "Save failed: disk full.".to_string(),
    });

    assert!(harness.app.pending_delete_id.is_none());
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

    harness.app.delete_selected();
    assert!(matches!(
        recv_cmd(&harness.cmd_rx),
        CoreCmd::UpdatePasteMeta { .. }
    ));

    harness.app.apply_event(CoreEvent::Error {
        source: CoreErrorSource::SaveMetadata,
        message: "Metadata save failed: disk full.".to_string(),
    });

    assert!(harness.app.pending_delete_id.is_none());
    assert!(harness.app.metadata_dirty);
    assert!(!harness.app.metadata_save_in_flight);
    assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));
    assert!(matches!(
        harness.cmd_rx.try_recv(),
        Err(TryRecvError::Empty)
    ));
}
