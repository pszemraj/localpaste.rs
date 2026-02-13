//! State/event flow tests for basic app selection, status, and toast behavior.

use super::*;

#[test]
fn paste_missing_clears_selection_and_removes_list_entry() {
    let mut harness = make_app();
    harness.app.apply_event(CoreEvent::PasteMissing {
        id: "alpha".to_string(),
    });

    assert!(harness.app.pastes.is_empty());
    assert!(harness.app.selected_id.is_none());
    assert!(harness.app.selected_paste.is_none());
    assert_eq!(harness.app.selected_content.len(), 0);
    assert!(harness.app.status.is_some());
}

#[test]
fn paste_missing_non_selected_removes_list_entry() {
    let mut harness = make_app();
    harness.app.pastes.push(PasteSummary {
        id: "beta".to_string(),
        name: "Beta".to_string(),
        language: None,
        content_len: 4,
        updated_at: Utc::now(),
        folder_id: None,
        tags: Vec::new(),
    });

    harness.app.apply_event(CoreEvent::PasteMissing {
        id: "beta".to_string(),
    });

    assert_eq!(harness.app.pastes.len(), 1);
    assert_eq!(harness.app.pastes[0].id, "alpha");
    assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));
    assert!(harness.app.selected_paste.is_some());
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
fn delete_selected_keeps_lock_until_delete_event() {
    let mut harness = make_app();
    harness.app.locks.lock("alpha");
    assert!(harness.app.locks.is_locked("alpha"));

    harness.app.delete_selected();
    assert!(harness.app.locks.is_locked("alpha"));

    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::DeletePaste { id } => assert_eq!(id, "alpha"),
        other => panic!("expected delete command, got {:?}", other),
    }

    harness.app.apply_event(CoreEvent::PasteDeleted {
        id: "alpha".to_string(),
    });
    assert!(!harness.app.locks.is_locked("alpha"));
}

#[test]
fn palette_delete_keeps_lock_until_delete_event() {
    let mut harness = make_app();
    harness.app.locks.lock("alpha");
    assert!(harness.app.locks.is_locked("alpha"));

    harness.app.send_palette_delete("alpha".to_string());
    assert!(harness.app.locks.is_locked("alpha"));

    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::DeletePaste { id } => assert_eq!(id, "alpha"),
        other => panic!("expected delete command, got {:?}", other),
    }

    harness.app.apply_event(CoreEvent::PasteDeleted {
        id: "alpha".to_string(),
    });
    assert!(!harness.app.locks.is_locked("alpha"));
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
fn delete_selected_send_failure_keeps_lock_and_shows_error_status() {
    let TestHarness {
        _dir: _guard,
        mut app,
        cmd_rx,
    } = make_app();
    app.locks.lock("alpha");
    drop(cmd_rx);

    app.delete_selected();

    assert!(app.locks.is_locked("alpha"));
    assert_eq!(
        app.status.as_ref().map(|status| status.text.as_str()),
        Some("Delete failed: backend unavailable.")
    );
}

#[test]
fn palette_delete_send_failure_keeps_lock_and_shows_error_status() {
    let TestHarness {
        _dir: _guard,
        mut app,
        cmd_rx,
    } = make_app();
    app.locks.lock("alpha");
    drop(cmd_rx);

    app.send_palette_delete("alpha".to_string());

    assert!(app.locks.is_locked("alpha"));
    assert_eq!(
        app.status.as_ref().map(|status| status.text.as_str()),
        Some("Delete failed: backend unavailable.")
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
fn palette_copy_raw_for_loaded_selection_sets_clipboard() {
    let mut harness = make_app();
    if let Some(paste) = harness.app.selected_paste.as_mut() {
        paste.id = "alpha".to_string();
    }
    harness.app.pending_copy_action = None;

    harness.app.queue_palette_copy("alpha".to_string(), false);

    assert_eq!(harness.app.clipboard_outgoing.as_deref(), Some("content"));
    assert!(harness.app.pending_copy_action.is_none());
}

#[test]
fn palette_copy_fenced_for_loaded_selection_sets_language_block() {
    let mut harness = make_app();
    if let Some(paste) = harness.app.selected_paste.as_mut() {
        paste.id = "alpha".to_string();
        paste.language = Some("rust".to_string());
    }
    harness.app.pending_copy_action = None;

    harness.app.queue_palette_copy("alpha".to_string(), true);

    assert_eq!(
        harness.app.clipboard_outgoing.as_deref(),
        Some("```rust\ncontent\n```")
    );
    assert!(harness.app.pending_copy_action.is_none());
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
        !app.locks.is_locked("alpha"),
        "failed reselect should not leak a stale lock"
    );
    assert!(
        app.selected_id.is_none(),
        "failed reselect should clear stale selection state"
    );
}
