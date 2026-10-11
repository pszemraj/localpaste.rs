//! State/event flow tests for status and toast behavior.

use super::*;

#[test]
fn routine_status_stays_in_the_status_bar_while_notices_also_toast() {
    let mut harness = make_app();
    harness.app.set_status("Copied paste content.");

    let status = harness.app.status.as_ref().expect("status");
    assert_eq!(status.text, "Copied paste content.");
    assert!(!status.alert);
    assert!(harness.app.toasts.is_empty());

    harness.app.notify("Export failed: disk full");
    let status = harness.app.status.as_ref().expect("status");
    assert_eq!(status.text, "Export failed: disk full");
    assert!(status.alert);
    assert_eq!(
        harness.app.toasts.back().map(|toast| toast.text.as_str()),
        Some("Export failed: disk full")
    );
}

#[test]
fn routine_confirmations_and_progress_do_not_toast() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    // The harness opens `alpha` without its edit lock; switching away releases it.
    assert!(harness.app.acquire_paste_lock("alpha"));
    // Copy and new-paste confirmations.
    harness.app.queue_palette_copy("alpha".into(), false);
    harness.app.create_new_paste();
    harness.app.apply_event(CoreEvent::PasteCreated {
        paste: Paste::new("new".into(), "New".into()),
    });
    run_full_update(&mut harness.app, &ctx, vec![]);
    // Undo restore progress and its result.
    harness.app.notify_with_action(
        "Paste deleted.",
        ToastAction::UndoDelete {
            undo_token: "undo-alpha".to_string(),
        },
    );
    harness.app.restore_deleted_paste("undo-alpha".to_string());
    assert_eq!(
        harness
            .app
            .status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some("Restoring deleted paste...")
    );
    let texts = harness
        .app
        .toasts
        .iter()
        .map(|toast| toast.text.as_str())
        .collect::<Vec<_>>();
    assert_eq!(texts, vec!["Paste deleted."], "only the undo toast remains");
}

#[test]
fn toast_queue_dedupes_tail_and_caps_length() {
    let mut harness = make_app();

    harness.app.notify("Repeated");
    harness.app.notify("Repeated");
    assert_eq!(harness.app.toasts.len(), 1);

    for idx in 0..(TOAST_LIMIT + 2) {
        harness.app.notify(format!("Toast {}", idx));
    }
    assert_eq!(harness.app.toasts.len(), TOAST_LIMIT);
}

#[test]
fn toast_queue_preserves_undo_actions_under_status_pressure() {
    let mut harness = make_app();

    harness.app.notify_with_action(
        "Paste deleted.",
        ToastAction::UndoDelete {
            undo_token: "undo-alpha".to_string(),
        },
    );
    for idx in 0..(TOAST_LIMIT + 2) {
        harness.app.notify(format!("Status {idx}"));
    }

    assert!(
        harness.app.toasts.iter().any(|toast| {
            matches!(
                &toast.action,
                Some(ToastAction::UndoDelete { undo_token }) if undo_token == "undo-alpha"
            )
        }),
        "ordinary status toasts must not evict the only undo affordance"
    );
    assert_eq!(
        harness
            .app
            .toasts
            .iter()
            .filter(|toast| toast.action.is_none())
            .count(),
        TOAST_LIMIT - 1,
        "actionless toasts should absorb overflow before actionable toasts"
    );
}

#[test]
fn toast_queue_caps_undo_actions_to_backend_limit() {
    let mut harness = make_app();

    for idx in 0..(DELETE_UNDO_LIMIT + 3) {
        harness.app.notify_with_action(
            format!("Paste deleted {idx}."),
            ToastAction::UndoDelete {
                undo_token: format!("undo-{idx}"),
            },
        );
    }

    let undo_tokens = harness
        .app
        .toasts
        .iter()
        .filter_map(|toast| {
            toast
                .action
                .as_ref()
                .map(|ToastAction::UndoDelete { undo_token }| undo_token.as_str())
        })
        .collect::<Vec<_>>();
    let expected_last = format!("undo-{}", DELETE_UNDO_LIMIT + 2);
    assert_eq!(undo_tokens.len(), DELETE_UNDO_LIMIT);
    assert_eq!(undo_tokens.first().copied(), Some("undo-3"));
    assert_eq!(undo_tokens.last().copied(), Some(expected_last.as_str()));
}

#[test]
fn toast_expiration_prunes_all_expired_entries_and_uses_earliest_live_deadline() {
    let mut harness = make_app();
    let now = Instant::now();

    harness.app.toasts.push_back(ToastMessage {
        text: "undo".to_string(),
        expires_at: now + Duration::from_secs(5),
        action: Some(ToastAction::UndoDelete {
            undo_token: "undo-alpha".to_string(),
        }),
    });
    harness.app.toasts.push_back(ToastMessage {
        text: "expired middle".to_string(),
        expires_at: now - Duration::from_secs(1),
        action: None,
    });
    harness.app.toasts.push_back(ToastMessage {
        text: "expired tail".to_string(),
        expires_at: now - Duration::from_secs(1),
        action: None,
    });

    harness.app.prune_expired_toasts(now);

    assert_eq!(harness.app.toasts.len(), 1);
    assert_eq!(
        harness.app.toasts.front().map(|toast| toast.text.as_str()),
        Some("undo")
    );
    harness.app.toasts.push_back(ToastMessage {
        text: "short status".to_string(),
        expires_at: now + Duration::from_secs(1),
        action: None,
    });

    assert_eq!(
        harness.app.next_toast_expiration(),
        Some(now + Duration::from_secs(1))
    );
}

#[test]
fn paste_deleted_toast_carries_undo_action_for_restore_window() {
    let mut harness = make_app();
    let before = Instant::now();

    harness.app.apply_event(CoreEvent::PasteDeleted {
        id: "alpha".to_string(),
        undo_token: Some("undo-alpha".to_string()),
    });

    let toast = harness.app.toasts.back().expect("undo toast");
    assert_eq!(
        toast.action,
        Some(ToastAction::UndoDelete {
            undo_token: "undo-alpha".to_string()
        })
    );
    assert!(
        toast.expires_at.saturating_duration_since(before) >= UNDO_DELETE_TOAST_TTL,
        "undo toast should stay visible for the GUI restore window"
    );
    assert!(
        toast.expires_at.saturating_duration_since(before) < Duration::from_secs(10),
        "GUI undo affordance should expire within the local bundle restore window"
    );
}

#[test]
fn paste_deleted_without_undo_token_has_no_undo_action() {
    let mut harness = make_app();

    harness.app.apply_event(CoreEvent::PasteDeleted {
        id: "alpha".to_string(),
        undo_token: None,
    });

    let toast = harness.app.toasts.back().expect("delete toast");
    assert!(toast.action.is_none());
    assert_eq!(toast.text.as_str(), "Paste deleted. Undo unavailable.");
}

#[test]
fn undo_delete_action_dispatches_restore_once_and_keeps_toast_until_ack() {
    let mut harness = make_app();
    harness.app.notify_with_action(
        "Paste deleted.",
        ToastAction::UndoDelete {
            undo_token: "undo-alpha".to_string(),
        },
    );
    harness.app.notify_with_action(
        "Paste deleted.",
        ToastAction::UndoDelete {
            undo_token: "undo-beta".to_string(),
        },
    );

    harness.app.restore_deleted_paste("undo-alpha".to_string());

    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::RestoreDeletedPaste { undo_token } => assert_eq!(undo_token, "undo-alpha"),
        other => panic!("expected RestoreDeletedPaste command, got {:?}", other),
    }
    assert!(
        harness.app.toasts.iter().any(|toast| {
            matches!(
                &toast.action,
                Some(ToastAction::UndoDelete { undo_token }) if undo_token == "undo-alpha"
            )
        }),
        "restore dispatch should keep the matching undo toast visible until backend ack"
    );
    assert!(
        harness
            .app
            .pending_undo_restore_tokens
            .contains("undo-alpha"),
        "an in-flight restore token should suppress duplicate dispatches"
    );
    assert!(
        harness.app.toasts.iter().any(|toast| {
            matches!(
                &toast.action,
                Some(ToastAction::UndoDelete { undo_token }) if undo_token == "undo-beta"
            )
        }),
        "other undo toasts should remain actionable"
    );
    assert_eq!(
        harness
            .app
            .status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some("Restoring deleted paste...")
    );

    harness.app.restore_deleted_paste("undo-alpha".to_string());
    assert!(
        matches!(harness.cmd_rx.try_recv(), Err(TryRecvError::Empty)),
        "an in-flight undo restore must not dispatch a duplicate restore"
    );
}

#[test]
fn paste_restored_ack_removes_matching_undo_toast() {
    let mut harness = make_app();
    harness.app.notify_with_action(
        "Paste deleted.",
        ToastAction::UndoDelete {
            undo_token: "undo-alpha".to_string(),
        },
    );
    harness.app.notify_with_action(
        "Paste deleted.",
        ToastAction::UndoDelete {
            undo_token: "undo-beta".to_string(),
        },
    );
    harness
        .app
        .pending_undo_restore_tokens
        .insert("undo-alpha".to_string());
    let mut restored = Paste::new("restored content".to_string(), "Restored".to_string());
    restored.id = "restored-id".to_string();

    harness.app.apply_event(CoreEvent::PasteRestored {
        paste: restored,
        undo_token: "undo-alpha".to_string(),
    });

    assert!(
        harness.app.toasts.iter().all(|toast| {
            !matches!(
                &toast.action,
                Some(ToastAction::UndoDelete { undo_token }) if undo_token == "undo-alpha"
            )
        }),
        "restore ack should remove only the confirmed undo toast"
    );
    assert!(
        harness.app.toasts.iter().any(|toast| {
            matches!(
                &toast.action,
                Some(ToastAction::UndoDelete { undo_token }) if undo_token == "undo-beta"
            )
        }),
        "other undo toasts should remain actionable"
    );
    assert!(
        !harness
            .app
            .pending_undo_restore_tokens
            .contains("undo-alpha"),
        "restore ack should clear the in-flight token"
    );
}

#[test]
fn undo_eviction_removes_matching_toast_without_touching_other_undo_actions() {
    let mut harness = make_app();
    harness.app.notify_with_action(
        "Paste deleted.",
        ToastAction::UndoDelete {
            undo_token: "undo-alpha".to_string(),
        },
    );
    harness.app.notify_with_action(
        "Paste deleted.",
        ToastAction::UndoDelete {
            undo_token: "undo-beta".to_string(),
        },
    );
    harness
        .app
        .pending_undo_restore_tokens
        .insert("undo-alpha".to_string());

    harness.app.apply_event(CoreEvent::PasteUndoEvicted {
        undo_token: "undo-alpha".to_string(),
    });

    assert!(!undo_toast_exists(&harness.app, "undo-alpha"));
    assert!(undo_toast_exists(&harness.app, "undo-beta"));
    assert!(!harness
        .app
        .pending_undo_restore_tokens
        .contains("undo-alpha"));
}

fn undo_toast_exists(app: &LocalPasteApp, token: &str) -> bool {
    app.toasts.iter().any(|toast| {
        matches!(
            &toast.action,
            Some(ToastAction::UndoDelete { undo_token }) if undo_token == token
        )
    })
}

fn assert_restore_deleted_command(rx: &Receiver<CoreCmd>, expected_token: &str) {
    match recv_cmd(rx) {
        CoreCmd::RestoreDeletedPaste { undo_token } => assert_eq!(undo_token, expected_token),
        other => panic!("expected RestoreDeletedPaste command, got {:?}", other),
    }
}

fn app_with_restore_in_flight(token: &str) -> TestHarness {
    let mut harness = make_app();
    harness.app.notify_with_action(
        "Paste deleted.",
        ToastAction::UndoDelete {
            undo_token: token.to_string(),
        },
    );

    harness.app.restore_deleted_paste(token.to_string());
    assert_restore_deleted_command(&harness.cmd_rx, token);
    harness
}

#[test]
fn undo_delete_retryable_restore_failure_keeps_toast_and_allows_retry() {
    let mut harness = app_with_restore_in_flight("undo-alpha");

    harness.app.apply_event(CoreEvent::PasteRestoreFailed {
        undo_token: "undo-alpha".to_string(),
        message: "Undo delete failed: paste already exists".to_string(),
        retryable: true,
    });

    assert!(
        undo_toast_exists(&harness.app, "undo-alpha"),
        "retryable restore failure should leave the undo affordance available"
    );
    assert!(
        !harness
            .app
            .pending_undo_restore_tokens
            .contains("undo-alpha"),
        "retryable restore failure should clear the in-flight marker"
    );
    assert_eq!(
        harness
            .app
            .status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some("Undo delete failed: paste already exists")
    );

    harness.app.restore_deleted_paste("undo-alpha".to_string());
    assert_restore_deleted_command(&harness.cmd_rx, "undo-alpha");
}

#[test]
fn undo_delete_nonretryable_restore_failure_removes_toast() {
    let mut harness = app_with_restore_in_flight("undo-alpha");

    harness.app.apply_event(CoreEvent::PasteRestoreFailed {
        undo_token: "undo-alpha".to_string(),
        message: "Undo delete expired.".to_string(),
        retryable: false,
    });

    assert!(
        !undo_toast_exists(&harness.app, "undo-alpha"),
        "non-retryable restore failure should remove the stale undo affordance"
    );
    harness.app.restore_deleted_paste("undo-alpha".to_string());
    assert!(
        matches!(harness.cmd_rx.try_recv(), Err(TryRecvError::Empty)),
        "an expired undo token should not dispatch another restore"
    );
}

#[test]
fn undo_delete_send_failure_keeps_retryable_toast() {
    let TestHarness {
        _dir: _guard,
        _highlight_requests,
        mut app,
        cmd_rx,
    } = make_app();
    app.notify_with_action(
        "Paste deleted.",
        ToastAction::UndoDelete {
            undo_token: "undo-alpha".to_string(),
        },
    );
    drop(cmd_rx);

    app.restore_deleted_paste("undo-alpha".to_string());

    assert!(
        undo_toast_exists(&app, "undo-alpha"),
        "failed dispatch must leave the undo action available for retry"
    );
    assert_eq!(
        app.status.as_ref().map(|status| status.text.as_str()),
        Some("Undo delete failed: backend unavailable.")
    );
}

#[test]
fn backend_event_disconnect_clears_in_flight_undo_restore_token() {
    let (mut harness, evt_tx) = make_app_with_event_tx();
    harness.app.notify_with_action(
        "Paste deleted.",
        ToastAction::UndoDelete {
            undo_token: "undo-alpha".to_string(),
        },
    );
    harness.app.restore_deleted_paste("undo-alpha".to_string());
    assert_restore_deleted_command(&harness.cmd_rx, "undo-alpha");
    assert!(harness
        .app
        .pending_undo_restore_tokens
        .contains("undo-alpha"));

    drop(evt_tx);
    let ctx = egui::Context::default();
    run_full_update(&mut harness.app, &ctx, Vec::new());

    assert!(!harness
        .app
        .pending_undo_restore_tokens
        .contains("undo-alpha"));
    assert_eq!(
        harness
            .app
            .status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some("Undo delete canceled: backend unavailable.")
    );
}

#[test]
fn toasts_stack_bottom_right_above_the_status_bar() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    harness.app.notify("First notice");
    harness.app.notify("Second notice");
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1100.0, 800.0));
    let input = || egui::RawInput {
        screen_rect: Some(screen),
        ..Default::default()
    };
    let _ = run_full_update_with_input(&mut harness.app, &ctx, input());
    let output = run_full_update_with_input(&mut harness.app, &ctx, input());
    // Search back to front: the status bar also shows the latest notice, and
    // toasts paint after it.
    let rect_of = |label: &str| {
        output
            .shapes
            .iter()
            .rev()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.job.text == label => {
                    Some(egui::Rect::from_min_size(text.pos, text.galley.size()))
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing {label}"))
    };
    let first = rect_of("First notice");
    let second = rect_of("Second notice");
    let status_bar_top = rect_of("DB:").top();
    let toolbar_bottom = rect_of("Copy").bottom();
    for toast in [first, second] {
        assert!(toast.top() > toolbar_bottom, "{toast:?} covers the toolbar");
        assert!(
            toast.bottom() < status_bar_top,
            "{toast:?} covers the status bar"
        );
        assert!(
            toast.right() > screen.right() - 120.0,
            "{toast:?} is not right-aligned"
        );
    }
    assert!(
        second.top() > first.top(),
        "the newest toast sits nearest the status bar"
    );
}

/// Renders `count` full frames in the toast test viewport, sending `events` with the first.
fn toast_frames(
    harness: &mut TestHarness,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
    count: usize,
) -> egui::FullOutput {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1100.0, 800.0));
    let mut events = Some(events);
    let mut output = None;
    for _ in 0..count {
        output = Some(run_full_update_with_input(
            &mut harness.app,
            ctx,
            egui::RawInput {
                screen_rect: Some(screen),
                events: events.take().unwrap_or_default(),
                ..Default::default()
            },
        ));
    }
    output.expect("at least one frame")
}

/// Paint-order positions and rects of every text shape reading `label`.
fn text_indices(output: &egui::FullOutput, label: &str) -> Vec<(usize, egui::Rect)> {
    output
        .shapes
        .iter()
        .enumerate()
        .filter_map(|(index, clipped)| match &clipped.shape {
            egui::Shape::Text(text) if text.galley.job.text == label => Some((
                index,
                egui::Rect::from_min_size(text.pos, text.galley.size()),
            )),
            _ => None,
        })
        .collect()
}

/// Presses and releases the primary button at `pos` over two frames.
fn toast_click(harness: &mut TestHarness, ctx: &egui::Context, pos: egui::Pos2) {
    for pressed in [true, false] {
        toast_frames(harness, ctx, primary_pointer_events(pos, pressed), 1);
    }
}

#[test]
fn toasts_stay_above_diff_and_history_and_keep_undo_clickable() {
    for title in ["Diff", "History"] {
        let mut harness = make_app();
        let ctx = egui::Context::default();
        if title == "Diff" {
            harness.app.open_diff_modal();
        } else {
            harness.app.open_history_modal();
        }
        toast_frames(&mut harness, &ctx, vec![], 4);
        harness.app.notify_with_action(
            "Deleted paste.",
            ToastAction::UndoDelete {
                undo_token: "token".into(),
            },
        );
        toast_frames(&mut harness, &ctx, vec![], 2);
        let window_rect = || {
            ctx.memory(|memory| memory.area_rect(egui::Id::new(title)))
                .expect(title)
        };

        // Dragging the title bar raises the window and moves it over the toasts.
        let grab = egui::pos2(window_rect().center().x, window_rect().top() + 12.0);
        let drop = grab + egui::vec2(0.0, 500.0);
        toast_frames(&mut harness, &ctx, primary_pointer_events(grab, true), 1);
        for step in 1..=4 {
            let pos = grab.lerp(drop, step as f32 / 4.0);
            toast_frames(&mut harness, &ctx, vec![egui::Event::PointerMoved(pos)], 1);
        }
        toast_frames(&mut harness, &ctx, primary_pointer_events(drop, false), 1);
        let output = toast_frames(&mut harness, &ctx, vec![], 2);

        let (title_index, _) = *text_indices(&output, title).last().expect(title);
        let (toast_index, _) = *text_indices(&output, "Deleted paste.")
            .last()
            .expect("toast");
        let (_, undo) = *text_indices(&output, "Undo").last().expect("Undo");
        assert!(
            window_rect().contains(undo.center()),
            "{title} lies under Undo"
        );
        assert!(toast_index > title_index, "{title} paints over the toast");
        assert_eq!(
            ctx.layer_id_at(undo.center()).map(|layer| layer.id),
            Some(egui::Id::new("toast_area")),
            "{title}"
        );
        let _ = harness.cmd_rx.try_iter().count();
        toast_click(&mut harness, &ctx, undo.center());
        assert!(
            harness.cmd_rx.try_iter().any(|cmd| matches!(
                cmd,
                CoreCmd::RestoreDeletedPaste { undo_token } if undo_token == "token"
            )),
            "{title}"
        );
    }
}

#[test]
fn open_popups_stay_above_a_toast_raised_after_them() {
    let mut harness = make_app();
    // A language gives the sidebar its language filter combo.
    let rust = test_summary("alpha", "Alpha", Some("rust"), 7);
    harness.app.all_pastes = vec![rust.clone()];
    harness.app.pastes = vec![rust];
    let ctx = egui::Context::default();
    let output = toast_frames(&mut harness, &ctx, vec![], 2);
    let (_, combo) = text_indices(&output, "All languages")[0];
    toast_click(&mut harness, &ctx, combo.center());
    let output = toast_frames(&mut harness, &ctx, vec![], 2);
    assert_eq!(
        text_indices(&output, "All languages").len(),
        2,
        "popup open"
    );

    harness.app.notify("Search failed.");
    let output = toast_frames(&mut harness, &ctx, vec![], 2);

    let options = text_indices(&output, "All languages");
    assert_eq!(options.len(), 2, "popup still open");
    let (toast_index, _) = *text_indices(&output, "Search failed.")
        .last()
        .expect("toast");
    assert!(
        options[1].0 > toast_index,
        "the toast covers the open popup"
    );
}
