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
    selected.app.command_palette_open = true;
    set_active_content(&mut selected.app, "palette dirty");
    selected.app.save_status = SaveStatus::Dirty;

    selected.app.send_palette_delete("alpha".to_string());

    assert!(
        !selected.app.command_palette_open,
        "accepted deferred delete should close the palette"
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

    let mut nonselected = make_app();
    nonselected.app.command_palette_open = true;
    set_active_content(&mut nonselected.app, "dirty selected");
    nonselected.app.save_status = SaveStatus::Dirty;

    nonselected.app.send_palette_delete("beta".to_string());

    match recv_cmd(&nonselected.cmd_rx) {
        CoreCmd::DeletePaste { id } => assert_eq!(id, "beta"),
        other => panic!(
            "expected non-selected palette delete immediately, got {:?}",
            other
        ),
    }
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
