//! State/event flow tests for basic app selection, status, and toast behavior.

use super::*;

fn assert_delete_send_failure_keeps_lock_and_status(
    delete_action: impl FnOnce(&mut crate::app::LocalPasteApp),
) {
    let TestHarness {
        _dir: _guard,
        mut app,
        cmd_rx,
    } = make_app();
    app.locks
        .acquire("alpha", &app.lock_owner_id)
        .expect("acquire alpha lock");
    drop(cmd_rx);

    delete_action(&mut app);

    assert!(app.locks.is_locked("alpha").expect("is_locked"));
    assert_eq!(
        app.status.as_ref().map(|status| status.text.as_str()),
        Some("Delete failed: backend unavailable.")
    );
}

#[test]
fn paste_missing_updates_selection_and_list_matrix() {
    enum MissingCase {
        Selected,
        NonSelected,
    }

    for case in [MissingCase::Selected, MissingCase::NonSelected] {
        let mut harness = make_app();
        match case {
            MissingCase::Selected => {
                harness.app.apply_event(CoreEvent::PasteMissing {
                    id: "alpha".to_string(),
                });

                assert!(harness.app.pastes.is_empty());
                assert!(harness.app.selected_id.is_none());
                assert!(harness.app.selected_paste.is_none());
                assert_eq!(harness.app.selected_content.len(), 0);
                assert!(harness.app.status.is_some());
            }
            MissingCase::NonSelected => {
                harness
                    .app
                    .pastes
                    .push(test_summary("beta", "Beta", None, 4));

                harness.app.apply_event(CoreEvent::PasteMissing {
                    id: "beta".to_string(),
                });

                assert_eq!(harness.app.pastes.len(), 1);
                assert_eq!(harness.app.pastes[0].id, "alpha");
                assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));
                assert!(harness.app.selected_paste.is_some());
            }
        }
    }
}

#[test]
fn paste_load_failed_updates_lock_and_selection_matrix() {
    enum LoadFailedCase {
        Selected,
        Stale,
    }

    for case in [LoadFailedCase::Selected, LoadFailedCase::Stale] {
        let mut harness = make_app();
        match case {
            LoadFailedCase::Selected => {
                harness
                    .app
                    .locks
                    .acquire("alpha", &harness.app.lock_owner_id)
                    .expect("acquire alpha lock");
                harness.app.pending_copy_action = Some(PaletteCopyAction::Raw("alpha".to_string()));

                harness.app.apply_event(CoreEvent::PasteLoadFailed {
                    id: "alpha".to_string(),
                    message: "Get failed: injected".to_string(),
                });

                assert!(
                    !harness.app.locks.is_locked("alpha").expect("is_locked"),
                    "selected paste lock should be released on load failure"
                );
                assert!(harness.app.selected_id.is_none());
                assert!(harness.app.selected_paste.is_none());
                assert!(harness.app.pending_copy_action.is_none());
                assert_eq!(
                    harness
                        .app
                        .status
                        .as_ref()
                        .map(|status| status.text.as_str()),
                    Some("Get failed: injected")
                );
            }
            LoadFailedCase::Stale => {
                harness.app.selected_id = Some("beta".to_string());
                harness.app.selected_paste =
                    Some(Paste::new("beta".to_string(), "Beta".to_string()));
                harness
                    .app
                    .locks
                    .acquire("beta", &harness.app.lock_owner_id)
                    .expect("acquire beta lock");
                harness.app.pending_copy_action = Some(PaletteCopyAction::Raw("alpha".to_string()));

                harness.app.apply_event(CoreEvent::PasteLoadFailed {
                    id: "alpha".to_string(),
                    message: "Get failed: stale".to_string(),
                });

                assert!(
                    harness.app.locks.is_locked("beta").expect("is_locked"),
                    "stale load failure should not unlock current selection"
                );
                assert_eq!(harness.app.selected_id.as_deref(), Some("beta"));
                assert!(harness.app.pending_copy_action.is_none());
                assert_eq!(
                    harness
                        .app
                        .status
                        .as_ref()
                        .map(|status| status.text.as_str()),
                    Some("Get failed: stale")
                );
            }
        }
    }
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
fn delete_actions_keep_lock_until_delete_event_matrix() {
    enum DeleteAction {
        Selected,
        Palette,
    }

    for action in [DeleteAction::Selected, DeleteAction::Palette] {
        let mut harness = make_app();
        harness
            .app
            .locks
            .acquire("alpha", &harness.app.lock_owner_id)
            .expect("acquire alpha lock");
        assert!(harness.app.locks.is_locked("alpha").expect("is_locked"));

        match action {
            DeleteAction::Selected => harness.app.delete_selected(),
            DeleteAction::Palette => harness.app.send_palette_delete("alpha".to_string()),
        }
        assert!(harness.app.locks.is_locked("alpha").expect("is_locked"));

        match recv_cmd(&harness.cmd_rx) {
            CoreCmd::DeletePaste { id } => assert_eq!(id, "alpha"),
            other => panic!("expected delete command, got {:?}", other),
        }

        harness.app.apply_event(CoreEvent::PasteDeleted {
            id: "alpha".to_string(),
        });
        assert!(!harness.app.locks.is_locked("alpha").expect("is_locked"));
    }
}

#[test]
fn paste_deleted_clears_pending_copy_action_for_deleted_id() {
    let mut harness = make_app();
    harness.app.pending_copy_action = Some(PaletteCopyAction::Raw("alpha".to_string()));

    harness.app.apply_event(CoreEvent::PasteDeleted {
        id: "alpha".to_string(),
    });

    assert!(harness.app.pending_copy_action.is_none());
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
fn delete_send_failure_keeps_lock_and_shows_error_status_matrix() {
    assert_delete_send_failure_keeps_lock_and_status(|app| app.delete_selected());
    assert_delete_send_failure_keeps_lock_and_status(|app| {
        app.send_palette_delete("alpha".to_string())
    });
}

#[test]
fn palette_open_failure_keeps_palette_open() {
    let TestHarness {
        _dir: _guard,
        mut app,
        cmd_rx,
    } = make_app();
    drop(cmd_rx);

    app.command_palette_open = true;
    app.open_palette_selection("beta".to_string());

    assert!(
        app.command_palette_open,
        "palette should stay open when open action fails"
    );
    assert_eq!(
        app.status.as_ref().map(|status| status.text.as_str()),
        Some("Get paste failed: backend unavailable.")
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
fn palette_copy_success_matrix_uses_expected_content_and_language() {
    struct PaletteCopyCase {
        fenced: bool,
        saved_content: &'static str,
        selected_content: &'static str,
        paste_language: Option<&'static str>,
        edit_language: Option<&'static str>,
        expected_clipboard: &'static str,
    }

    let cases = [
        PaletteCopyCase {
            fenced: false,
            saved_content: "content",
            selected_content: "content",
            paste_language: None,
            edit_language: None,
            expected_clipboard: "content",
        },
        PaletteCopyCase {
            fenced: true,
            saved_content: "content",
            selected_content: "content",
            paste_language: Some("rust"),
            edit_language: None,
            expected_clipboard: "```rust\ncontent\n```",
        },
        PaletteCopyCase {
            fenced: false,
            saved_content: "saved",
            selected_content: "unsaved",
            paste_language: Some("rust"),
            edit_language: None,
            expected_clipboard: "unsaved",
        },
        PaletteCopyCase {
            fenced: true,
            saved_content: "saved",
            selected_content: "unsaved",
            paste_language: Some("rust"),
            edit_language: Some("python"),
            expected_clipboard: "```python\nunsaved\n```",
        },
    ];

    for case in cases {
        let mut harness = make_app();
        if let Some(paste) = harness.app.selected_paste.as_mut() {
            paste.id = "alpha".to_string();
            paste.content = case.saved_content.to_string();
            paste.language = case.paste_language.map(str::to_string);
        }
        harness
            .app
            .selected_content
            .reset(case.selected_content.to_string());
        harness.app.edit_language = case.edit_language.map(str::to_string);
        harness.app.pending_copy_action = None;

        harness
            .app
            .queue_palette_copy("alpha".to_string(), case.fenced);

        assert_eq!(
            harness.app.clipboard_outgoing.as_deref(),
            Some(case.expected_clipboard)
        );
        assert!(harness.app.pending_copy_action.is_none());
    }
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
        !app.locks.is_locked("alpha").expect("is_locked"),
        "failed reselect should not leak a stale lock"
    );
    assert!(
        app.selected_id.is_none(),
        "failed reselect should clear stale selection state"
    );
}
