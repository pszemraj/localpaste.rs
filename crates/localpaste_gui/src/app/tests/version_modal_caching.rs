//! Regression coverage for cached History/Diff modal preview state.

use super::*;

#[test]
fn active_snapshot_cache_reuses_snapshot_until_editor_revision_changes() {
    let mut harness = make_app();
    set_active_content(&mut harness.app, "first\nsecond");

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

    let end = harness.app.active_text_chars();
    insert_active_text(&mut harness.app, "!", end);

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

    let selected_id = harness.app.selected_id.take();
    assert!(!harness.app.sync_diff_preview_cache());
    assert_eq!(
        harness
            .app
            .version_ui
            .diff_target_paste
            .as_ref()
            .map(|paste| paste.id.as_str()),
        Some("beta")
    );
    harness.app.apply_event(CoreEvent::DiffPreviewComputed {
        request_id,
        diff: localpaste_core::diff::DiffResponse {
            equal: true,
            unified: Vec::new(),
        },
    });
    assert!(harness.app.version_ui.diff_preview.is_none());
    harness.app.selected_id = selected_id;
    assert!(harness.app.sync_diff_preview_cache());
    let request_id = match recv_cmd(&harness.cmd_rx) {
        CoreCmd::ComputeDiffPreview {
            request_id: next_request_id,
            ..
        } => {
            assert_eq!(next_request_id, request_id.wrapping_add(1));
            next_request_id
        }
        other => panic!("unexpected command: {:?}", other),
    };

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

    let end = harness.app.active_text_chars();
    insert_active_text(&mut harness.app, "!", end);

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

    let end = harness.app.active_text_chars();
    insert_active_text(&mut harness.app, "!", end);
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
fn diff_preview_cache_invalidates_when_rhs_content_changes_without_metadata_change() {
    let mut harness = make_app();
    let mut rhs = Paste::new("beta-one".to_string(), "Beta".to_string());
    rhs.id = "beta".to_string();
    let stable_updated_at = rhs.updated_at;
    harness.app.version_ui.diff_target_paste = Some(rhs);

    assert!(harness.app.sync_diff_preview_cache());
    let request_id = match recv_cmd(&harness.cmd_rx) {
        CoreCmd::ComputeDiffPreview {
            request_id,
            left_text,
            right_text,
        } => {
            assert_eq!(left_text, "content");
            assert_eq!(right_text, "beta-one");
            request_id
        }
        other => panic!("unexpected command: {:?}", other),
    };
    harness.app.apply_event(CoreEvent::DiffPreviewComputed {
        request_id,
        diff: localpaste_core::diff::DiffResponse {
            equal: false,
            unified: localpaste_core::diff::unified_diff_lines("content", "beta-one"),
        },
    });

    let mut rhs_same_metadata = Paste::new("beta-two".to_string(), "Beta".to_string());
    rhs_same_metadata.id = "beta".to_string();
    rhs_same_metadata.updated_at = stable_updated_at;
    harness.app.version_ui.diff_target_paste = Some(rhs_same_metadata);

    assert!(
        harness.app.sync_diff_preview_cache(),
        "right-hand content changes must invalidate the cached diff even if len/id/timestamp stay the same"
    );
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::ComputeDiffPreview {
            request_id: next_request_id,
            left_text,
            right_text,
        } => {
            assert_ne!(next_request_id, request_id);
            assert_eq!(left_text, "content");
            assert_eq!(right_text, "beta-two");
        }
        other => panic!("unexpected command: {:?}", other),
    }
}

#[test]
fn diff_preview_cache_short_circuits_large_payloads_before_snapshot_clone() {
    let mut harness = make_app();
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

#[test]
fn diff_candidates_use_the_sidebar_language_labels() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    harness.app.all_pastes = vec![
        test_summary("alpha", "Alpha", None, 7),
        test_summary("unset", "Unset", None, 10),
        test_summary("table", "Table", Some("csv"), 10),
        test_summary("huge", "Huge", Some("rust"), HIGHLIGHT_PLAIN_THRESHOLD),
    ];
    harness.app.open_diff_modal();
    let input = || egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1600.0, 1000.0),
        )),
        ..Default::default()
    };
    let _ = run_full_update_with_input(&mut harness.app, &ctx, input());
    let output = run_full_update_with_input(&mut harness.app, &ctx, input());
    let texts = output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    for candidate in ["Unset", "Table", "Huge"] {
        assert!(texts.contains(&candidate), "missing {candidate}: {texts:?}");
    }
    // Same wording as the sidebar: unresolved is `auto`, large buffers are `plain`.
    for label in ["auto", "csv", "plain"] {
        assert!(texts.contains(&label), "missing {label}: {texts:?}");
    }
    assert!(!texts
        .iter()
        .any(|text| text.contains("[text]") || text.contains("[csv]")));
}

#[test]
fn diff_rows_tint_changed_lines_across_the_pane() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    harness.app.open_diff_modal();
    let mut rhs = Paste::new("beta-content".to_string(), "Beta".to_string());
    rhs.id = "beta".to_string();
    harness.app.version_ui.diff_target_paste = Some(rhs);
    assert!(harness.app.sync_diff_preview_cache());
    let CoreCmd::ComputeDiffPreview { request_id, .. } = recv_cmd(&harness.cmd_rx) else {
        panic!("expected a diff preview request");
    };
    harness.app.apply_event(CoreEvent::DiffPreviewComputed {
        request_id,
        diff: localpaste_core::diff::DiffResponse {
            equal: false,
            unified: vec![" same".into(), "-content".into(), "+beta-content".into()],
        },
    });
    // Frames a second apart let the window's fade-in finish, so fills are exact.
    let input = |time: f64| egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1600.0, 1000.0),
        )),
        time: Some(time),
        ..Default::default()
    };
    let _ = run_full_update_with_input(&mut harness.app, &ctx, input(0.0));
    let output = run_full_update_with_input(&mut harness.app, &ctx, input(1.0));

    let tints = |fill: egui::Color32| {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(rect) if rect.fill == fill => Some(rect.rect),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let text_rect = |label: &str| {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.job.text == label => {
                    Some(egui::Rect::from_min_size(text.pos, text.galley.size()))
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing {label}"))
    };
    for (label, fill) in [
        ("-content", COLOR_DIFF_REMOVED_BG),
        ("+beta-content", COLOR_DIFF_ADDED_BG),
    ] {
        let line = text_rect(label);
        let [band] = tints(fill)[..] else {
            panic!("expected one {label} band");
        };
        assert!(band.contains(line.center()), "{label} sits on its band");
        assert!(
            band.width() > 4.0 * line.width(),
            "{label} band spans the pane"
        );
    }
    let removed = tints(COLOR_DIFF_REMOVED_BG)[0];
    let added = tints(COLOR_DIFF_ADDED_BG)[0];
    assert!(
        (removed.bottom() - added.top()).abs() < 0.5,
        "adjacent changes form one band"
    );
    let context = text_rect(" same").center();
    assert!(!removed.contains(context) && !added.contains(context));
}
