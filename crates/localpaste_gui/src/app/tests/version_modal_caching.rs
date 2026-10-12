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
    // The sidebar's own `Alpha` row also reads `auto`, so only Diff window text counts.
    let window = ctx
        .memory(|memory| memory.area_rect(egui::Id::new("Diff")))
        .expect("Diff window");
    let texts = output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) => {
                let rect = egui::Rect::from_min_size(text.pos, text.galley.size());
                window
                    .contains_rect(rect)
                    .then_some((text.galley.job.text.as_str(), rect))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let rects_of = |label: &str| {
        texts
            .iter()
            .filter(|(text, _)| *text == label)
            .map(|(_, rect)| *rect)
            .collect::<Vec<_>>()
    };
    // Same wording as the sidebar: unresolved is `auto`, large buffers are `plain`.
    for (candidate, language) in [("Unset", "auto"), ("Table", "csv"), ("Huge", "plain")] {
        let [name] = rects_of(candidate)[..] else {
            panic!("expected one {candidate} row in the Diff window: {texts:?}");
        };
        assert!(
            rects_of(language).iter().any(|label| {
                (label.center().y - name.center().y).abs() < 1.0 && label.left() > name.right()
            }),
            "the {candidate} row should end with {language}: {texts:?}"
        );
    }
    assert!(!texts
        .iter()
        .any(|(text, _)| text.contains("[text]") || text.contains("[csv]")));
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

/// Opens History over three stored versions with the first one selected.
///
/// Version lengths are 11, 22, and 33 bytes so each row label is distinct.
fn open_history_with_versions(app: &mut LocalPasteApp) {
    let now = chrono::Utc::now();
    app.open_history_modal();
    app.version_ui.history_versions = [11, 22, 33]
        .into_iter()
        .enumerate()
        .map(|(index, len)| localpaste_core::models::paste::VersionMeta {
            version_id_ms: index as u64 + 1,
            created_at: now,
            content_hash: format!("hash-{len}"),
            len,
            language: Some("rust".to_string()),
            language_is_manual: false,
        })
        .collect();
    app.version_ui.history_selected_index = 1;
}

/// Rests the pointer on the History row whose label contains `size`, letting fades finish.
fn hover_history_row(
    app: &mut LocalPasteApp,
    ctx: &egui::Context,
    size: &str,
) -> (egui::FullOutput, egui::Pos2) {
    let input = |time: f64, events: Vec<egui::Event>| egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1600.0, 1000.0),
        )),
        time: Some(time),
        events,
        ..Default::default()
    };
    let _ = run_full_update_with_input(app, ctx, input(0.0, Vec::new()));
    let settled = run_full_update_with_input(app, ctx, input(1.0, Vec::new()));
    let label = format!("| {size} |");
    let center = settled
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) if text.galley.job.text.contains(&label) => {
                Some(egui::Rect::from_min_size(text.pos, text.galley.size()).center())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing History row {label}"));
    // Hover resolves against the previous frame's widget rects.
    let mut output = settled;
    for step in 0..4 {
        output = run_full_update_with_input(
            app,
            ctx,
            input(
                2.0 + f64::from(step),
                vec![egui::Event::PointerMoved(center)],
            ),
        );
    }
    (output, center)
}

#[test]
fn history_rows_hover_neutral_and_keep_the_selection_fill() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    open_history_with_versions(&mut harness.app);
    let (output, center) = hover_history_row(&mut harness.app, &ctx, "22 B");
    let fills = fills_behind(&output, center);
    assert!(
        fills.contains(&COLOR_BG_TERTIARY),
        "hovered version row should use the neutral list fill: {fills:?}"
    );
    assert!(
        !fills.contains(&COLOR_ACCENT_SURFACE_HOVER) && !fills.contains(&COLOR_ACCENT_SURFACE),
        "version rows must not hover with the toolbar accent: {fills:?}"
    );
    assert_eq!(
        harness.app.version_ui.history_selected_index, 1,
        "hover keeps selection"
    );

    let mut harness = make_app();
    let ctx = egui::Context::default();
    open_history_with_versions(&mut harness.app);
    let (output, center) = hover_history_row(&mut harness.app, &ctx, "11 B");
    let fills = fills_behind(&output, center);
    let selection = ctx.style().visuals.selection.bg_fill;
    assert!(
        fills.contains(&selection),
        "hovered selected version keeps the selection fill: {fills:?}"
    );
    assert!(
        !fills.contains(&COLOR_BG_TERTIARY) && !fills.contains(&COLOR_ACCENT_SURFACE_HOVER),
        "hover must not replace the selection fill: {fills:?}"
    );
}

#[test]
fn history_snapshot_caption_shows_a_readable_save_time() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    open_history_with_versions(&mut harness.app);
    let (output, _) = hover_history_row(&mut harness.app, &ctx, "22 B");
    let captions = output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) if text.galley.job.text.starts_with("Saved ") => {
                Some(text.galley.job.text.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let [caption] = captions.as_slice() else {
        panic!("expected one snapshot caption, found {captions:?}");
    };
    assert!(
        caption.starts_with("Saved just now · ") && caption.ends_with(" · 11 B"),
        "the caption should match its row's age and size: {caption}"
    );
    assert!(
        !caption.contains("Version") && !caption.contains('T') && !caption.contains("+00:00"),
        "the caption must not show the raw id or an RFC 3339 stamp: {caption}"
    );
}
