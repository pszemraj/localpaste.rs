//! Regression coverage for cached History/Diff modal preview state.

use super::*;

#[test]
fn active_snapshot_cache_reuses_snapshot_until_editor_revision_changes() {
    let mut harness = make_app();
    harness
        .app
        .selected_content
        .reset("first\nsecond".to_string());

    assert!(harness.app.sync_active_snapshot_cache());
    assert_eq!(
        harness.app.version_ui.active_snapshot_cache_text,
        "first\nsecond"
    );
    assert_eq!(
        harness
            .app
            .version_ui
            .active_snapshot_preview_lines
            .line_count(),
        2
    );
    assert!(
        !harness.app.sync_active_snapshot_cache(),
        "repeated cache sync should be a no-op when active buffer identity is unchanged"
    );

    harness
        .app
        .selected_content
        .insert_text("!", harness.app.selected_content.len());

    assert!(harness.app.sync_active_snapshot_cache());
    assert_eq!(
        harness.app.version_ui.active_snapshot_cache_text,
        "first\nsecond!"
    );
    assert_eq!(
        harness
            .app
            .version_ui
            .active_snapshot_preview_lines
            .line_count(),
        2
    );
}

#[test]
fn history_preview_cache_reuses_loaded_snapshot_until_selected_version_changes() {
    let mut harness = make_app();
    harness.app.version_ui.history_selected_index = 1;
    harness.app.version_ui.history_snapshot =
        Some(localpaste_core::models::paste::VersionSnapshot {
            paste_id: "alpha".to_string(),
            version_id_ms: 41,
            created_at: chrono::Utc::now(),
            content_hash: "hash-41".to_string(),
            len: 4,
            language: None,
            language_is_manual: false,
            content: "text".to_string(),
        });

    assert!(harness.app.sync_history_preview_cache());
    assert_eq!(harness.app.version_ui.history_preview_text, "text");
    assert_eq!(harness.app.version_ui.history_preview_lines.line_count(), 1);
    assert!(
        !harness.app.sync_history_preview_cache(),
        "identical snapshot selections should reuse the cached preview body"
    );

    harness.app.version_ui.history_snapshot =
        Some(localpaste_core::models::paste::VersionSnapshot {
            paste_id: "alpha".to_string(),
            version_id_ms: 42,
            created_at: chrono::Utc::now(),
            content_hash: "hash-42".to_string(),
            len: 4,
            language: None,
            language_is_manual: false,
            content: "next".to_string(),
        });

    assert!(harness.app.sync_history_preview_cache());
    assert_eq!(harness.app.version_ui.history_preview_text, "next");
    assert_eq!(harness.app.version_ui.history_preview_lines.line_count(), 1);
}

#[test]
fn diff_preview_cache_queues_worker_request_and_reuses_it_until_inputs_change() {
    let mut harness = make_app();

    assert!(
        !harness.app.sync_diff_preview_cache(),
        "diff cache should stay cold until a comparison target is loaded"
    );
    assert!(harness.app.version_ui.diff_preview.is_none());
    assert!(harness.app.version_ui.active_snapshot_cache_text.is_empty());

    let mut rhs = Paste::new("beta-content".to_string(), "Beta".to_string());
    rhs.id = "beta".to_string();
    harness.app.version_ui.diff_target_paste = Some(rhs);

    assert!(harness.app.sync_diff_preview_cache());
    let request_id = match recv_cmd(&harness.cmd_rx) {
        CoreCmd::ComputeDiffPreview {
            request_id,
            left_text,
            right_text,
        } => {
            assert_eq!(left_text, "content");
            assert_eq!(right_text, "beta-content");
            request_id
        }
        other => panic!("unexpected command: {:?}", other),
    };
    assert!(harness.app.version_ui.diff_preview.is_none());
    assert!(
        !harness.app.sync_diff_preview_cache(),
        "same current snapshot + in-flight worker request should not redispatch"
    );
    assert!(matches!(
        harness.cmd_rx.try_recv(),
        Err(crossbeam_channel::TryRecvError::Empty)
    ));

    harness.app.apply_event(CoreEvent::DiffPreviewComputed {
        request_id,
        diff: localpaste_core::diff::DiffResponse {
            equal: false,
            unified: localpaste_core::diff::unified_diff_lines("content", "beta-content"),
        },
    });
    assert!(matches!(
        harness.app.version_ui.diff_preview.as_ref(),
        Some(crate::app::ui::diff_modal::InlineDiffPreview::Lines(lines)) if !lines.is_empty()
    ));
    assert!(
        !harness.app.sync_diff_preview_cache(),
        "same current snapshot + target paste should reuse the completed worker preview"
    );

    harness
        .app
        .selected_content
        .insert_text("!", harness.app.selected_content.len());

    assert!(harness.app.sync_diff_preview_cache());
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::ComputeDiffPreview {
            request_id: next_request_id,
            left_text,
            right_text,
        } => {
            assert_ne!(next_request_id, request_id);
            assert_eq!(left_text, "content!");
            assert_eq!(right_text, "beta-content");
        }
        other => panic!("unexpected command: {:?}", other),
    }
}

#[test]
fn diff_preview_cache_waits_for_in_flight_worker_before_queueing_latest_revision() {
    let mut harness = make_app();
    let mut rhs = Paste::new("beta-content".to_string(), "Beta".to_string());
    rhs.id = "beta".to_string();
    harness.app.version_ui.diff_target_paste = Some(rhs);

    assert!(harness.app.sync_diff_preview_cache());
    let request_id = match recv_cmd(&harness.cmd_rx) {
        CoreCmd::ComputeDiffPreview { request_id, .. } => request_id,
        other => panic!("unexpected command: {:?}", other),
    };

    harness
        .app
        .selected_content
        .insert_text("!", harness.app.selected_content.len());
    assert!(
        !harness.app.sync_diff_preview_cache(),
        "a new revision should wait until the current worker request resolves"
    );
    assert!(matches!(
        harness.cmd_rx.try_recv(),
        Err(crossbeam_channel::TryRecvError::Empty)
    ));

    harness.app.apply_event(CoreEvent::DiffPreviewComputed {
        request_id,
        diff: localpaste_core::diff::DiffResponse {
            equal: false,
            unified: localpaste_core::diff::unified_diff_lines("content", "beta-content"),
        },
    });
    assert!(harness.app.sync_diff_preview_cache());
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::ComputeDiffPreview {
            request_id: next_request_id,
            left_text,
            right_text,
        } => {
            assert_ne!(next_request_id, request_id);
            assert_eq!(left_text, "content!");
            assert_eq!(right_text, "beta-content");
        }
        other => panic!("unexpected command: {:?}", other),
    }
}

#[test]
fn diff_preview_cache_short_circuits_large_payloads_before_snapshot_clone() {
    let mut harness = make_app();
    harness.app.editor_mode = EditorMode::VirtualEditor;
    harness
        .app
        .reset_virtual_editor(&"x".repeat(crate::app::ui::diff_modal::MAX_INLINE_DIFF_BYTES));

    let mut rhs = Paste::new("y".to_string(), "Beta".to_string());
    rhs.id = "beta".to_string();
    harness.app.version_ui.diff_target_paste = Some(rhs);

    assert!(harness.app.sync_diff_preview_cache());
    assert_eq!(
        harness.app.version_ui.diff_preview,
        Some(crate::app::ui::diff_modal::InlineDiffPreview::TooLarge {
            lhs_bytes: crate::app::ui::diff_modal::MAX_INLINE_DIFF_BYTES,
            rhs_bytes: 1,
        })
    );
    assert!(
        harness.app.version_ui.active_snapshot_cache_text.is_empty(),
        "oversized diff previews should not materialize the current editor snapshot"
    );
    assert!(matches!(
        harness.cmd_rx.try_recv(),
        Err(crossbeam_channel::TryRecvError::Empty)
    ));
}
