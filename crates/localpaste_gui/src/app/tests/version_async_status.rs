//! Regression coverage for stale detached History/Diff async status handling.

use super::*;

#[test]
fn version_modal_failure_cleanup_is_scoped_to_the_matching_request() {
    let mut harness = make_app();
    harness.app.version_ui.diff_target_id = Some("beta".to_string());
    harness.app.version_ui.diff_loading_target = true;
    harness.app.version_ui.history_loading_snapshot_id = Some(42);
    harness.app.version_ui.history_snapshot =
        Some(localpaste_core::models::paste::VersionSnapshot {
            paste_id: "alpha".to_string(),
            version_id_ms: 42,
            created_at: chrono::Utc::now(),
            content_hash: "hash".to_string(),
            len: 4,
            language: None,
            language_is_manual: false,
            content: "text".to_string(),
        });
    harness.app.version_ui.history_reset_in_flight_paste_id = Some("alpha".to_string());

    harness.app.apply_event(CoreEvent::DiffTargetLoadFailed {
        id: "beta".to_string(),
        message: "Diff load failed: missing".to_string(),
    });
    assert!(!harness.app.version_ui.diff_loading_target);
    assert!(harness.app.version_ui.diff_target_id.is_none());
    assert_eq!(harness.app.version_ui.history_loading_snapshot_id, Some(42));
    assert!(harness.app.version_ui.history_snapshot.is_some());
    assert_eq!(
        harness
            .app
            .status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some("Diff load failed: missing")
    );

    harness.app.apply_event(CoreEvent::Error {
        source: crate::backend::CoreErrorSource::Other,
        message: "Search failed".to_string(),
    });
    assert_eq!(harness.app.version_ui.history_loading_snapshot_id, Some(42));
    assert!(harness.app.version_ui.history_snapshot.is_some());

    harness.app.apply_event(CoreEvent::PasteVersionLoadFailed {
        paste_id: "alpha".to_string(),
        version_id_ms: 42,
        message: "Get version failed".to_string(),
    });
    assert!(harness.app.version_ui.history_loading_snapshot_id.is_none());
    assert!(harness.app.version_ui.history_snapshot.is_none());
    assert_eq!(
        harness
            .app
            .status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some("Get version failed")
    );

    harness.app.apply_event(CoreEvent::Error {
        source: crate::backend::CoreErrorSource::SaveContent,
        message: "reset failed".to_string(),
    });
    assert!(harness
        .app
        .version_ui
        .history_reset_in_flight_paste_id
        .is_none());
}

#[test]
fn stale_history_load_failures_do_not_override_status_after_snapshot_request_changes() {
    let mut harness = make_app();
    harness.app.set_status("Ready");
    harness.app.version_ui.history_loading_snapshot_id = Some(84);
    harness.app.version_ui.history_snapshot =
        Some(localpaste_core::models::paste::VersionSnapshot {
            paste_id: "alpha".to_string(),
            version_id_ms: 84,
            created_at: chrono::Utc::now(),
            content_hash: "hash-84".to_string(),
            len: 4,
            language: None,
            language_is_manual: false,
            content: "next".to_string(),
        });

    harness.app.apply_event(CoreEvent::PasteVersionLoadFailed {
        paste_id: "alpha".to_string(),
        version_id_ms: 42,
        message: "stale version failure".to_string(),
    });

    assert_eq!(harness.app.version_ui.history_loading_snapshot_id, Some(84));
    assert_eq!(
        harness
            .app
            .version_ui
            .history_snapshot
            .as_ref()
            .map(|snapshot| snapshot.version_id_ms),
        Some(84)
    );
    assert_eq!(
        harness
            .app
            .status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some("Ready")
    );
}

#[test]
fn stale_diff_target_failures_do_not_override_status_after_target_changes() {
    let mut harness = make_app();
    harness.app.set_status("Ready");
    harness.app.version_ui.diff_target_id = Some("gamma".to_string());
    harness.app.version_ui.diff_loading_target = true;

    harness.app.apply_event(CoreEvent::DiffTargetLoadFailed {
        id: "beta".to_string(),
        message: "Diff load failed: stale".to_string(),
    });

    assert!(harness.app.version_ui.diff_loading_target);
    assert_eq!(
        harness.app.version_ui.diff_target_id.as_deref(),
        Some("gamma")
    );
    assert_eq!(
        harness
            .app
            .status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some("Ready")
    );
}

#[test]
fn stale_diff_target_missing_refreshes_lists_without_overwriting_status() {
    let mut harness = make_app();
    harness.app.set_status("Ready");
    harness
        .app
        .all_pastes
        .push(test_summary("beta", "Beta", None, 4));
    harness.app.pastes = harness.app.all_pastes.clone();

    harness.app.apply_event(CoreEvent::DiffTargetMissing {
        id: "beta".to_string(),
    });

    assert!(
        !harness.app.pastes.iter().any(|paste| paste.id == "beta"),
        "stale diff-target missing events should still keep list caches honest"
    );
    assert_eq!(
        harness
            .app
            .status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some("Ready")
    );
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::ListPastes { .. } => {}
        other => panic!("expected ListPastes refresh command, got {:?}", other),
    }
}

#[test]
fn request_diff_target_uses_detached_backend_command() {
    let mut harness = make_app();

    harness.app.request_diff_target("beta".to_string());

    assert_eq!(
        harness.app.version_ui.diff_target_id.as_deref(),
        Some("beta")
    );
    assert!(harness.app.version_ui.diff_loading_target);
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::GetDiffTargetPaste { id } => assert_eq!(id, "beta"),
        other => panic!("expected GetDiffTargetPaste command, got {:?}", other),
    }
}
