//! Picker copy actions, request identity, and detached response regressions.

use super::*;

#[test]
fn paste_picker_copy_loads_the_requested_result() {
    let mut harness = make_app();
    harness.app.open_paste_picker();
    harness.app.queue_palette_copy("picked".into(), false);
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::GetPasteForCopy { id, .. } => assert_eq!(id, "picked"),
        other => panic!("unexpected picker copy action: {other:?}"),
    }

    let mut picked = Paste::new("copied content".into(), "Picked".into());
    picked.id = "picked".into();
    harness.app.apply_event(CoreEvent::PasteCopyLoaded {
        paste: picked,
        request_id: 1,
    });
    assert_eq!(
        harness.app.clipboard_outgoing.as_deref(),
        Some("copied content")
    );
    assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));
    assert_eq!(harness.app.active_snapshot(), "content");
    assert!(harness.app.picker_selection_pin.is_none());
    assert!(harness.app.pending_copy_action.is_none());
}

#[test]
fn picker_copy_of_active_result_cancels_a_stale_detached_request() {
    let mut harness = make_app();
    set_active_content(&mut harness.app, "dirty alpha");
    harness.app.open_paste_picker();
    harness.app.queue_palette_copy("beta".into(), false);
    assert!(matches!(
        recv_cmd(&harness.cmd_rx),
        CoreCmd::GetPasteForCopy { ref id, .. } if id == "beta"
    ));

    harness.app.queue_palette_copy("alpha".into(), false);
    assert_eq!(
        harness.app.clipboard_outgoing.as_deref(),
        Some("dirty alpha")
    );
    assert!(harness.app.pending_copy_action.is_none());

    let mut beta = Paste::new("stale beta".into(), "Beta".into());
    beta.id = "beta".into();
    harness.app.apply_event(CoreEvent::PasteCopyLoaded {
        paste: beta,
        request_id: 1,
    });
    assert_eq!(
        harness.app.clipboard_outgoing.as_deref(),
        Some("dirty alpha")
    );
    assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));
    assert_eq!(harness.app.active_snapshot(), "dirty alpha");
}

#[test]
fn picker_copy_preserves_dirty_editor_and_drops_stale_responses() {
    let mut harness = make_app();
    set_active_content(&mut harness.app, "dirty alpha");
    harness.app.save_status = SaveStatus::Dirty;
    harness
        .app
        .locks
        .acquire("alpha", &harness.app.lock_owner_id)
        .expect("acquire active lock");
    harness.app.open_paste_picker();
    harness.app.queue_palette_copy("beta".into(), false);
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::GetPasteForCopy { id, .. } => assert_eq!(id, "beta"),
        other => panic!("unexpected first picker copy command: {other:?}"),
    }
    assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));
    assert_eq!(harness.app.active_snapshot(), "dirty alpha");
    assert_eq!(harness.app.save_status, SaveStatus::Dirty);
    assert!(harness.app.pending_selection_id.is_none());
    assert!(harness.app.picker_selection_pin.is_none());
    assert!(harness.app.locks.is_locked("alpha").expect("active lock"));
    assert!(!harness.app.locks.is_locked("beta").expect("target lock"));

    harness.app.queue_palette_copy("gamma".into(), true);
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::GetPasteForCopy { id, .. } => assert_eq!(id, "gamma"),
        other => panic!("unexpected second picker copy command: {other:?}"),
    }
    let mut stale = Paste::new("stale content".into(), "Beta".into());
    stale.id = "beta".into();
    harness.app.apply_event(CoreEvent::PasteCopyLoaded {
        paste: stale,
        request_id: 1,
    });
    harness.app.apply_event(CoreEvent::PasteCopyLoadFailed {
        request_id: 1,
        id: "beta".into(),
        message: "Copy failed: stale".into(),
    });
    assert!(harness.app.clipboard_outgoing.is_none());
    assert!(matches!(
        harness.app.pending_copy_action,
        Some(PaletteCopyAction::Fenced(ref id)) if id == "gamma"
    ));
    assert_eq!(
        harness
            .app
            .status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some("Loading paste for copy...")
    );

    let mut gamma = Paste::new("gamma content".into(), "Gamma".into());
    gamma.id = "gamma".into();
    gamma.language = Some("rust".into());
    harness.app.apply_event(CoreEvent::PasteCopyLoaded {
        paste: gamma,
        request_id: 2,
    });
    assert_eq!(
        harness.app.clipboard_outgoing.as_deref(),
        Some("```rust\ngamma content\n```")
    );
    assert!(harness.app.pending_copy_action.is_none());
    assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));
    assert_eq!(harness.app.active_snapshot(), "dirty alpha");
    assert_eq!(harness.app.save_status, SaveStatus::Dirty);
    assert!(harness.app.locks.is_locked("alpha").expect("active lock"));
    assert!(!harness.app.locks.is_locked("gamma").expect("target lock"));
}

#[test]
fn picker_copy_terminal_errors_preserve_the_active_editor() {
    for missing in [false, true] {
        let mut harness = make_app();
        set_active_content(&mut harness.app, "dirty alpha");
        harness.app.save_status = SaveStatus::Dirty;
        harness
            .app
            .locks
            .acquire("alpha", &harness.app.lock_owner_id)
            .expect("acquire active lock");
        harness.app.open_paste_picker();
        harness.app.queue_palette_copy("beta".into(), false);
        let _ = recv_cmd(&harness.cmd_rx);

        if missing {
            harness.app.apply_event(CoreEvent::PasteCopyMissing {
                id: "beta".into(),
                request_id: 1,
            });
        } else {
            harness.app.apply_event(CoreEvent::PasteCopyLoadFailed {
                request_id: 1,
                id: "beta".into(),
                message: "Copy failed: injected".into(),
            });
        }

        assert!(
            harness.app.pending_copy_action.is_none(),
            "missing={missing}"
        );
        assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));
        assert_eq!(harness.app.active_snapshot(), "dirty alpha");
        assert_eq!(harness.app.save_status, SaveStatus::Dirty);
        assert!(harness.app.pending_selection_id.is_none());
        assert!(harness.app.picker_selection_pin.is_none());
        assert!(harness.app.locks.is_locked("alpha").expect("active lock"));
        assert!(!harness.app.locks.is_locked("beta").expect("target lock"));
        let expected = if missing {
            "Paste is no longer available to copy."
        } else {
            "Copy failed: injected"
        };
        assert_eq!(
            harness
                .app
                .status
                .as_ref()
                .map(|status| status.text.as_str()),
            Some(expected),
            "missing={missing}"
        );
    }
}

#[test]
fn repeated_picker_copy_ignores_superseded_terminal_errors() {
    for missing in [false, true] {
        let mut harness = make_app();
        harness.app.open_paste_picker();
        harness.app.queue_palette_copy("beta".into(), false);
        let _ = recv_cmd(&harness.cmd_rx);
        harness.app.queue_palette_copy("beta".into(), true);
        let _ = recv_cmd(&harness.cmd_rx);

        let stale = if missing {
            CoreEvent::PasteCopyMissing {
                id: "beta".into(),
                request_id: 1,
            }
        } else {
            CoreEvent::PasteCopyLoadFailed {
                id: "beta".into(),
                request_id: 1,
                message: "Copy failed: superseded".into(),
            }
        };
        harness.app.apply_event(stale);
        assert!(matches!(
            harness.app.pending_copy_action,
            Some(PaletteCopyAction::Fenced(ref id)) if id == "beta"
        ));
        assert!(harness.app.clipboard_outgoing.is_none());
        assert_eq!(
            harness
                .app
                .status
                .as_ref()
                .map(|status| status.text.as_str()),
            Some("Loading paste for copy...")
        );

        let mut beta = Paste::new("latest body".into(), "Beta".into());
        beta.id = "beta".into();
        harness.app.apply_event(CoreEvent::PasteCopyLoaded {
            paste: beta,
            request_id: 2,
        });
        assert!(harness.app.pending_copy_action.is_none());
        assert!(harness
            .app
            .clipboard_outgoing
            .as_deref()
            .unwrap()
            .contains("latest body"));
    }
}

#[test]
fn repeated_picker_copy_uses_the_latest_requested_snapshot() {
    let mut harness = make_app();
    let dir = TempDir::new().expect("temporary database");
    let db = Database::new(dir.path().join("db").to_str().unwrap()).unwrap();
    let mut beta = Paste::new("old beta body".into(), "Beta".into());
    beta.id = "beta".into();
    db.pastes.create(&beta).unwrap();
    let mut backend = crate::backend::spawn_backend_with_locks_and_owner(
        db.share().unwrap(),
        10 * 1024 * 1024,
        harness.app.locks.clone(),
        harness.app.lock_owner_id.clone(),
    );
    harness.app.open_paste_picker();
    harness.app.queue_palette_copy("beta".into(), false);
    assert!(harness.app.paste_picker_open);
    backend.cmd_tx.send(recv_cmd(&harness.cmd_rx)).unwrap();
    let old_reply = backend.evt_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(
        matches!(&old_reply, CoreEvent::PasteCopyLoaded { paste, .. }
        if paste.content == "old beta body")
    );

    // A reply produced after the frame's initial drain waits until the next frame.
    // The unselected target remains writable through the embedded API meanwhile.
    assert!(!harness.app.locks.is_locked("beta").unwrap());
    let mutation = harness.app.locks.begin_mutation("beta").unwrap();
    db.pastes
        .update(
            "beta",
            localpaste_core::models::paste::UpdatePasteRequest {
                content: Some("latest beta body".into()),
                name: None,
                language: None,
                language_is_manual: None,
                folder_id: None,
                tags: None,
            },
        )
        .unwrap();
    drop(mutation);
    harness.app.queue_palette_copy("beta".into(), true);
    backend.cmd_tx.send(recv_cmd(&harness.cmd_rx)).unwrap();
    let latest_reply = backend.evt_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(
        matches!(&latest_reply, CoreEvent::PasteCopyLoaded { paste, .. }
        if paste.content == "latest beta body")
    );
    backend
        .shutdown_and_join(true, Duration::from_secs(5))
        .unwrap();

    // Preserve the worker's real FIFO reply order. The second click requests
    // content after the completed mutation, rather than the first click's snapshot.
    harness.app.apply_event(old_reply);
    harness.app.apply_event(latest_reply);
    assert!(
        harness
            .app
            .clipboard_outgoing
            .as_deref()
            .is_some_and(|copied| {
                copied.contains("latest beta body") && !copied.contains("old beta body")
            }),
        "the older reply incorrectly satisfied the newer picker copy: {:?}",
        harness.app.clipboard_outgoing
    );
}
