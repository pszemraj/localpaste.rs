//! Independent discovery surfaces and stale scoped-response regressions.

use super::*;
use crate::app::paste_intent::KeyboardFocusState;

#[test]
fn sidebar_and_picker_reject_stale_scopes_and_keep_independent_queries() {
    let mut harness = make_app();
    harness.app.set_search_query("sidebar".into());
    harness.app.set_search_scope(SearchScope::Title);
    harness.app.maybe_dispatch_search();
    assert!(matches!(
        recv_cmd(&harness.cmd_rx),
        CoreCmd::SearchPastes {
            collection: crate::backend::SidebarCollection::All,
            scope: SearchScope::Title,
            ..
        }
    ));
    harness.app.paste_picker_open = true;
    harness.app.set_paste_picker_query("picker".into());
    harness.app.set_paste_picker_scope(SearchScope::Body);
    harness.app.maybe_dispatch_palette_search();
    assert!(matches!(
        recv_cmd(&harness.cmd_rx),
        CoreCmd::SearchPalette {
            scope: SearchScope::Body,
            ..
        }
    ));
    for scope in [SearchScope::All, SearchScope::Body, SearchScope::Metadata] {
        harness.app.apply_event(CoreEvent::SearchResults {
            collection: crate::backend::SidebarCollection::All,
            query: "sidebar".into(),
            scope,
            folder_id: None,
            language: None,
            items: vec![test_summary("stale", "stale", None, 1)],
        });
        assert!(harness.app.pastes.is_empty());
    }
    for scope in [SearchScope::All, SearchScope::Title, SearchScope::Metadata] {
        harness.app.apply_event(CoreEvent::PaletteSearchResults {
            query: "picker".into(),
            scope,
            items: vec![test_summary("stale", "stale", None, 1)],
        });
        assert!(harness.app.palette_search_results.is_empty());
    }
    harness.app.apply_event(CoreEvent::PaletteSearchResults {
        query: "picker".into(),
        scope: SearchScope::Body,
        items: vec![test_summary("valid", "valid", None, 1)],
    });
    assert_eq!(harness.app.palette_search_results[0].id, "valid");
    assert_eq!(harness.app.search_query, "sidebar");
    assert_eq!(harness.app.search_scope, SearchScope::Title);
    // Changing only the field scope must dispatch even with identical query text.
    harness.app.set_search_scope(SearchScope::Metadata);
    harness.app.maybe_dispatch_search();
    assert!(matches!(
        recv_cmd(&harness.cmd_rx),
        CoreCmd::SearchPastes {
            collection: crate::backend::SidebarCollection::All,
            scope: SearchScope::Metadata,
            ..
        }
    ));
    assert_eq!(harness.app.paste_picker_query, "picker");
}

#[test]
fn command_palette_has_toolbar_actions_and_never_dispatches_paste_search() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    run_full_update(
        &mut harness.app,
        &ctx,
        vec![command_key_event(egui::Key::K)],
    );
    assert!(harness.app.command_palette_open);
    for query in [
        "Export",
        "Duplicate",
        "Copy",
        "Copy link",
        "Find",
        "Properties",
        "History",
        "Diff",
    ] {
        harness.app.command_palette_query = query.into();
        assert!(
            !harness.app.command_palette_actions().is_empty(),
            "missing {query}"
        );
    }
    harness.app.command_palette_query.clear();
    run_full_update(
        &mut harness.app,
        &ctx,
        vec![egui::Event::Text("body-only-needle".into())],
    );
    assert_eq!(harness.app.command_palette_query, "body-only-needle");
    assert!(harness.app.command_palette_actions().is_empty());
    assert!(harness.cmd_rx.try_recv().is_err());
}

#[test]
fn paste_picker_shortcut_and_modal_input_leave_editor_unchanged() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    harness.app.focus_editor_next = true;
    run_full_update(&mut harness.app, &ctx, vec![]);
    let modifiers = egui::Modifiers {
        shift: true,
        ..primary_command_modifiers()
    };
    run_full_update(
        &mut harness.app,
        &ctx,
        vec![key_event(egui::Key::K, modifiers)],
    );
    assert!(harness.app.paste_picker_open);
    assert!(!harness.app.command_palette_open);
    assert!(harness.app.keyboard_overlay_open());
    run_full_update(
        &mut harness.app,
        &ctx,
        vec![egui::Event::Text("query".into())],
    );
    assert_eq!(harness.app.paste_picker_query, "query");
    assert_eq!(harness.app.active_snapshot(), "content");
    run_full_update(
        &mut harness.app,
        &ctx,
        vec![command_key_event(egui::Key::K)],
    );
    assert!(!harness.app.paste_picker_open);
    assert!(harness.app.command_palette_open);
    assert_eq!(harness.app.paste_picker_query, "query");
}

#[test]
fn help_focus_and_unfocused_modals_never_create_background_pastes() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    harness.app.open_shortcut_help(&ctx);
    run_full_update(&mut harness.app, &ctx, vec![]);
    run_full_update(
        &mut harness.app,
        &ctx,
        vec![egui::Event::Paste("undo".into())],
    );
    assert_eq!(harness.app.shortcut_help_query, "undo");
    assert_eq!(harness.app.active_snapshot(), "content");
    assert!(!harness.app.maybe_route_implicit_global_clipboard_create(
        Some("background".into()),
        false,
        false,
        false
    ));
    assert_eq!(
        harness
            .app
            .route_plain_paste_shortcut(KeyboardFocusState::Unfocused, false),
        (false, false)
    );
    assert!(!harness
        .cmd_rx
        .try_iter()
        .any(|cmd| matches!(cmd, CoreCmd::CreatePaste { .. })));
}

#[test]
fn discovery_overlays_transfer_keyboard_ownership_when_switching() {
    for picker in [false, true] {
        let mut harness = make_app();
        let ctx = egui::Context::default();
        run_full_update(
            &mut harness.app,
            &ctx,
            vec![key_event(egui::Key::F1, egui::Modifiers::NONE)],
        );
        run_full_update(
            &mut harness.app,
            &ctx,
            vec![egui::Event::Text("undo".into())],
        );
        assert_eq!(harness.app.shortcut_help_query, "undo");

        let modifiers = egui::Modifiers {
            shift: picker,
            ..primary_command_modifiers()
        };
        run_full_update(
            &mut harness.app,
            &ctx,
            vec![key_event(egui::Key::K, modifiers)],
        );
        assert!(!harness.app.shortcut_help_open);
        assert_eq!(harness.app.paste_picker_open, picker);
        assert_eq!(harness.app.command_palette_open, !picker);
        run_full_update(
            &mut harness.app,
            &ctx,
            vec![egui::Event::Text("needle".into())],
        );
        let query = if picker {
            &harness.app.paste_picker_query
        } else {
            &harness.app.command_palette_query
        };
        assert_eq!(query, "needle");

        run_full_update(
            &mut harness.app,
            &ctx,
            vec![key_event(egui::Key::F1, egui::Modifiers::NONE)],
        );
        assert!(harness.app.shortcut_help_open);
        assert!(!harness.app.paste_picker_open);
        assert!(!harness.app.command_palette_open);
        run_full_update(
            &mut harness.app,
            &ctx,
            vec![egui::Event::Text(" again".into())],
        );
        assert_eq!(harness.app.shortcut_help_query, "undo again");
        assert_eq!(harness.app.active_snapshot(), "content");
    }
}

#[test]
fn reopening_picker_retries_search_discarded_while_closed() {
    for from_command in [true, false] {
        let mut harness = make_app();
        let ctx = egui::Context::default();
        let picker_shortcut = || {
            key_event(
                egui::Key::K,
                egui::Modifiers {
                    shift: true,
                    ..primary_command_modifiers()
                },
            )
        };
        run_full_update(&mut harness.app, &ctx, vec![picker_shortcut()]);
        harness.app.set_paste_picker_query("needle".into());
        harness.app.set_paste_picker_scope(SearchScope::Body);
        harness.app.maybe_dispatch_palette_search();
        assert!(matches!(
            recv_cmd(&harness.cmd_rx),
            CoreCmd::SearchPalette { .. }
        ));
        run_full_update(
            &mut harness.app,
            &ctx,
            vec![key_event(egui::Key::Escape, egui::Modifiers::NONE)],
        );
        let response = || CoreEvent::PaletteSearchResults {
            query: "needle".into(),
            scope: SearchScope::Body,
            items: vec![test_summary("match", "needle", None, 10)],
        };
        harness.app.apply_event(response());
        assert!(!harness.app.paste_picker_open);
        assert!(harness.app.palette_search_results.is_empty());

        if from_command {
            run_full_update(
                &mut harness.app,
                &ctx,
                vec![command_key_event(egui::Key::K)],
            );
            run_full_update(
                &mut harness.app,
                &ctx,
                vec![egui::Event::Text("Open paste picker".into())],
            );
            run_full_update(
                &mut harness.app,
                &ctx,
                vec![key_event(egui::Key::Enter, egui::Modifiers::NONE)],
            );
        } else {
            run_full_update(&mut harness.app, &ctx, vec![picker_shortcut()]);
        }
        assert!(harness.app.paste_picker_open);
        assert!(
            harness.cmd_rx.try_iter().any(|cmd| matches!(
                cmd,
                CoreCmd::SearchPalette { query, scope: SearchScope::Body, .. } if query == "needle"
            )),
            "reopening from command={from_command} must retry the retained query and scope"
        );
        harness.app.apply_event(response());
        assert_eq!(harness.app.palette_search_results[0].id, "match");
    }
}

#[test]
fn command_palette_arrows_keep_first_and_last_commands_visible() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    harness.app.command_palette_open = true;
    let mut time = 0.0;
    let mut render = |app: &mut LocalPasteApp, events| {
        time += 0.5; // Advance egui's scrolling animation without wall-clock sleeps.
        run_full_update_with_input(
            app,
            &ctx,
            egui::RawInput {
                time: Some(time),
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1100.0, 800.0),
                )),
                events,
                ..Default::default()
            },
        )
    };
    for _ in 0..3 {
        render(&mut harness.app, vec![]);
    }
    let count = harness.app.command_palette_actions().len();
    for (key, selected, label) in [
        (egui::Key::ArrowDown, count - 1, "Refresh list  "),
        (egui::Key::ArrowUp, 0, "New paste  "),
    ] {
        for _ in 0..count {
            render(
                &mut harness.app,
                vec![key_event(key, egui::Modifiers::NONE)],
            );
        }
        render(&mut harness.app, vec![]);
        let output = render(&mut harness.app, vec![]);
        assert_eq!(harness.app.command_palette_selected, selected);
        assert!(
            output.shapes.iter().any(|clipped| {
                if let egui::Shape::Text(text) = &clipped.shape {
                    text.galley.job.text.starts_with(label)
                        && clipped
                            .clip_rect
                            .contains_rect(egui::Rect::from_min_size(text.pos, text.galley.size()))
                } else {
                    false
                }
            }),
            "keyboard-selected command {label:?} must be fully visible"
        );
    }
}

#[test]
fn paste_picker_reports_pending_searches_and_discards_closed_results() {
    let (mut harness, _event_tx) = make_app_with_event_tx();
    let ctx = egui::Context::default();
    harness.app.open_paste_picker();
    harness.app.set_paste_picker_query("needle".into());
    harness.app.palette_search_last_sent = "needle".into();
    harness.app.palette_search_pending = true;

    let picker_input = || egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1100.0, 800.0),
        )),
        ..Default::default()
    };
    let _ = run_full_update_with_input(&mut harness.app, &ctx, picker_input());
    let output = run_full_update_with_input(&mut harness.app, &ctx, picker_input());
    assert!(output.shapes.iter().any(|clipped| {
        matches!(&clipped.shape, egui::Shape::Text(text) if text.galley.job.text.contains("Searching..."))
    }));

    harness.app.apply_event(CoreEvent::PaletteSearchFailed {
        query: "stale".into(),
        scope: SearchScope::All,
        message: "Palette search failed: stale request".into(),
    });
    assert!(harness.app.palette_search_pending);
    assert!(harness.app.status.is_none());

    harness.app.apply_event(CoreEvent::PaletteSearchFailed {
        query: "needle".into(),
        scope: SearchScope::All,
        message: "Palette search failed: disk unavailable".into(),
    });
    assert!(!harness.app.palette_search_pending);
    assert_eq!(
        harness
            .app
            .status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some("Palette search failed: disk unavailable")
    );
    assert!(harness.app.palette_search_last_input_at.is_none());
    harness.app.maybe_dispatch_palette_search();
    assert!(!harness
        .cmd_rx
        .try_iter()
        .any(|cmd| matches!(cmd, CoreCmd::SearchPalette { .. })));
    // The Retry affordance explicitly arms a new request for the same query.
    harness.app.palette_search_last_input_at = Some(Instant::now() - SEARCH_DEBOUNCE);
    harness.app.maybe_dispatch_palette_search();
    assert!(harness
        .cmd_rx
        .try_iter()
        .any(|cmd| matches!(cmd, CoreCmd::SearchPalette { .. })));

    harness.app.palette_search_pending = true;
    harness.app.handle_backend_event_channel_disconnected();
    assert!(!harness.app.palette_search_pending);
    assert_eq!(
        harness
            .app
            .status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some("Paste picker search canceled: backend unavailable.")
    );

    harness.app.pending_copy_action = Some(PaletteCopyAction::Raw("picked".into()));
    harness.app.handle_backend_event_channel_disconnected();
    assert!(harness.app.pending_copy_action.is_none());
    assert_eq!(
        harness
            .app
            .status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some("Paste copy canceled: backend unavailable.")
    );

    harness.app.apply_event(CoreEvent::PaletteSearchResults {
        query: "needle".into(),
        scope: SearchScope::All,
        items: Vec::new(),
    });
    assert!(!harness.app.palette_search_pending);
    let _ = run_full_update_with_input(&mut harness.app, &ctx, picker_input());
    let output = run_full_update_with_input(&mut harness.app, &ctx, picker_input());
    assert!(output.shapes.iter().any(|clipped| {
        matches!(&clipped.shape, egui::Shape::Text(text) if text.galley.job.text.contains("No matching pastes"))
    }));

    harness.app.selected_id = Some("picked".into());
    harness.app.all_pastes = vec![test_summary("picked", "Fresh", Some("rust"), 10)];
    harness.app.palette_search_results = vec![test_summary("picked", "Stale", Some("python"), 10)];
    harness.app.paste_picker_selected = 1;
    run_full_update_with_input(
        &mut harness.app,
        &ctx,
        egui::RawInput {
            events: vec![key_event(egui::Key::Escape, egui::Modifiers::NONE)],
            ..Default::default()
        },
    );
    assert!(!harness.app.paste_picker_open);
    assert_eq!(harness.app.paste_picker_selected, 0);
    assert!(harness.app.palette_search_results.is_empty());
    assert_eq!(
        harness
            .app
            .selected_paste_summary()
            .map(|summary| summary.name.as_str()),
        Some("Fresh")
    );
}

#[test]
fn paste_picker_uses_sidebar_language_guardrail_and_reveals_keyboard_selection() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    harness.app.open_paste_picker();
    harness.app.set_paste_picker_query("all".into());
    harness.app.palette_search_results = (0..40)
        .map(|index| {
            test_summary(
                &format!("paste-{index}"),
                &format!("Paste {index}"),
                Some("rust"),
                if index == 0 {
                    HIGHLIGHT_PLAIN_THRESHOLD
                } else {
                    1
                },
            )
        })
        .collect();
    harness.app.palette_search_last_sent = "all".into();
    let mut time = 0.0;
    let mut render = |app: &mut LocalPasteApp, events| {
        time += 0.5;
        run_full_update_with_input(
            app,
            &ctx,
            egui::RawInput {
                time: Some(time),
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1100.0, 800.0),
                )),
                events,
                ..Default::default()
            },
        )
    };

    let _ = render(&mut harness.app, vec![]);
    let initial = render(&mut harness.app, vec![]);
    assert!(initial.shapes.iter().any(|clipped| {
        matches!(&clipped.shape, egui::Shape::Text(text) if text.galley.job.text.starts_with("plain"))
    }));
    for _ in 0..39 {
        render(
            &mut harness.app,
            vec![key_event(egui::Key::ArrowDown, egui::Modifiers::NONE)],
        );
    }
    render(&mut harness.app, vec![]);
    let output = render(&mut harness.app, vec![]);
    assert_eq!(harness.app.paste_picker_selected, 39);
    assert!(
        output.shapes.iter().any(|clipped| {
            if let egui::Shape::Text(text) = &clipped.shape {
                text.galley.job.text.starts_with("Paste 39")
                    && clipped
                        .clip_rect
                        .contains_rect(egui::Rect::from_min_size(text.pos, text.galley.size()))
            } else {
                false
            }
        }),
        "keyboard-selected picker row must be fully visible"
    );
    render(
        &mut harness.app,
        vec![key_event(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert!(!harness.app.paste_picker_open);
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::GetPaste { id, .. } => assert_eq!(id, "paste-39"),
        other => panic!("unexpected picker action: {other:?}"),
    }
}

#[test]
fn paste_picker_renders_body_match_excerpt() {
    let (mut harness, _events) = make_app_with_event_tx();
    let ctx = egui::Context::default();
    harness.app.open_paste_picker();
    harness.app.set_paste_picker_query("needle".into());
    harness.app.set_paste_picker_scope(SearchScope::Body);
    harness.app.palette_search_results = vec![PasteSummary {
        match_excerpt: Some(format!("{}Needle", "context ".repeat(18))),
        ..test_summary("body-hit", "Body hit", Some("text"), 150)
    }];
    harness.app.palette_search_last_sent = "needle".into();
    harness.app.paste_picker_sent_scope = SearchScope::Body;

    let input = || egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1100.0, 800.0),
        )),
        ..Default::default()
    };
    let _ = run_full_update_with_input(&mut harness.app, &ctx, input());
    let output = run_full_update_with_input(&mut harness.app, &ctx, input());
    assert!(output.shapes.iter().any(|clipped| {
        matches!(&clipped.shape, egui::Shape::Text(text) if text.galley.job.text.contains("Needle"))
    }));
    assert!(output.shapes.iter().any(|clipped| {
        matches!(&clipped.shape, egui::Shape::Text(text) if text.galley.job.text.starts_with("Copy Fenced")
            && clipped.clip_rect.contains_rect(egui::Rect::from_min_size(text.pos, text.galley.size())))
    }));
}

#[test]
fn picker_open_keeps_off_sidebar_selection_until_sidebar_context_changes() {
    let mut harness = make_app();
    harness.app.search_query = "sidebar".into();
    harness.app.search_last_sent = "sidebar".into();
    harness.app.all_pastes = vec![test_summary("alpha", "Alpha", None, 10)];
    harness.app.pastes = vec![test_summary("alpha", "Alpha", None, 10)];
    harness.app.open_paste_picker();
    harness.app.open_palette_selection("picked".into());
    assert_eq!(harness.app.picker_selection_pin.as_deref(), Some("picked"));
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::GetPaste { id, .. } => assert_eq!(id, "picked"),
        other => panic!("unexpected picker action: {other:?}"),
    }

    let mut picked = Paste::new("picked content".into(), "Picked".into());
    picked.id = "picked".into();
    picked.language = Some("rust".into());
    harness.app.apply_event(CoreEvent::PasteLoaded {
        paste: picked,
        selection_epoch: harness.app.active_buffer_epoch,
    });
    let mut autosaved = Paste::new("picked content".into(), "Picked".into());
    autosaved.id = "picked".into();
    autosaved.language = Some("rust".into());
    harness
        .app
        .apply_event(CoreEvent::PasteSaved { paste: autosaved });
    harness.app.apply_event(CoreEvent::PasteList {
        items: vec![test_summary("alpha", "Alpha", None, 10)],
    });
    assert!(
        harness
            .app
            .all_pastes
            .iter()
            .all(|summary| summary.id != "picked"),
        "bounded sidebar refresh must not revoke a picker selection"
    );
    harness.app.search_last_sent = "sidebar".into();
    harness.app.apply_event(CoreEvent::SearchResults {
        collection: crate::backend::SidebarCollection::All,
        query: "sidebar".into(),
        scope: SearchScope::All,
        folder_id: None,
        language: None,
        items: vec![test_summary("alpha", "Alpha", None, 10)],
    });
    assert_eq!(harness.app.selected_id.as_deref(), Some("picked"));

    harness.app.set_search_query("changed".into());
    assert!(harness.app.picker_selection_pin.is_none());
    harness.app.search_last_sent = "changed".into();
    harness.app.apply_event(CoreEvent::SearchResults {
        collection: crate::backend::SidebarCollection::All,
        query: "changed".into(),
        scope: SearchScope::All,
        folder_id: None,
        language: None,
        items: vec![test_summary("alpha", "Alpha", None, 10)],
    });
    assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));
}

#[test]
fn picker_selection_pin_survives_deferred_open_and_clears_after_target_failure() {
    let mut harness = make_app();
    harness.app.search_query = "sidebar".into();
    harness
        .app
        .all_pastes
        .push(test_summary("picked", "Picked", None, 10));
    harness.app.save_status = SaveStatus::Dirty;
    harness.app.open_paste_picker();
    harness.app.open_palette_selection("picked".into());
    assert_eq!(harness.app.pending_selection_id.as_deref(), Some("picked"));
    assert_eq!(harness.app.picker_selection_pin.as_deref(), Some("picked"));

    let mut saved = Paste::new("saved alpha".into(), "Alpha".into());
    saved.id = "alpha".into();
    harness
        .app
        .apply_event(CoreEvent::PasteSaved { paste: saved });
    assert_eq!(harness.app.selected_id.as_deref(), Some("picked"));
    assert_eq!(harness.app.picker_selection_pin.as_deref(), Some("picked"));
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::UpdatePasteVirtual { id, .. } => assert_eq!(id, "alpha"),
        other => panic!("unexpected deferred save command: {other:?}"),
    }
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::GetPaste { id, .. } => assert_eq!(id, "picked"),
        other => panic!("unexpected deferred picker command: {other:?}"),
    }

    harness.app.apply_event(CoreEvent::PasteLoadFailed {
        selection_epoch: harness.app.active_buffer_epoch,
        id: "picked".into(),
        message: "picked no longer exists".into(),
    });
    assert!(harness.app.picker_selection_pin.is_none());
    assert!(harness.app.selected_id.is_none());
}

#[test]
fn picker_selection_pin_clears_when_the_target_is_deleted() {
    let mut harness = make_app();
    harness
        .app
        .all_pastes
        .push(test_summary("picked", "Picked", None, 10));
    harness.app.open_paste_picker();
    harness.app.open_palette_selection("picked".into());
    assert_eq!(harness.app.picker_selection_pin.as_deref(), Some("picked"));
    let _ = recv_cmd(&harness.cmd_rx);

    harness.app.apply_event(CoreEvent::PasteDeleted {
        id: "picked".into(),
        undo_token: None,
    });
    assert!(harness.app.picker_selection_pin.is_none());
    assert!(harness.app.selected_id.is_none());
}

#[test]
fn picker_reopen_selects_the_retained_query_for_replacement() {
    let (mut harness, _event_tx) = make_app_with_event_tx();
    let ctx = egui::Context::default();
    harness.app.set_paste_picker_query("fence-delete".into());
    harness.app.open_paste_picker();
    run_full_update(&mut harness.app, &ctx, vec![]);
    run_full_update(
        &mut harness.app,
        &ctx,
        vec![egui::Event::Text("fresh".into())],
    );
    assert_eq!(harness.app.paste_picker_query, "fresh");
    harness.app.close_paste_picker();
    harness.app.open_paste_picker();
    run_full_update(&mut harness.app, &ctx, vec![]);
    run_full_update(
        &mut harness.app,
        &ctx,
        vec![egui::Event::Text("again".into())],
    );
    assert_eq!(harness.app.paste_picker_query, "again");
    harness.app.close_paste_picker();
    run_full_update(&mut harness.app, &ctx, vec![]);
    run_full_update(
        &mut harness.app,
        &ctx,
        vec![key_event(
            egui::Key::K,
            primary_command_modifiers() | egui::Modifiers::SHIFT,
        )],
    );
    assert!(harness.app.paste_picker_open);
    run_full_update(
        &mut harness.app,
        &ctx,
        vec![egui::Event::Text("shortcut".into())],
    );
    assert_eq!(harness.app.paste_picker_query, "shortcut");
}

#[test]
fn sidebar_scope_changes_keep_the_loaded_document_and_reading_position() {
    for query in ["", "body-only"] {
        let (mut harness, _event_tx) = make_app_with_event_tx();
        let ctx = egui::Context::default();
        let content = "long document body-only line\n".repeat(180);
        harness.app.reset_virtual_editor(&content);
        harness
            .app
            .virtual_editor_state
            .restore_selection(606, Some(602), content.len());
        harness.app.picker_selection_pin = Some("alpha".into());
        // The open document is intentionally hidden by the sidebar filter.
        harness.app.active_language_filter = Some("rust".into());
        harness.app.all_pastes = vec![test_summary("beta", "Beta", Some("rust"), 7)];
        harness.app.set_search_query(query.into());
        harness.app.picker_selection_pin = Some("alpha".into());
        let render = |app: &mut LocalPasteApp| {
            run_full_update_with_input(
                app,
                &ctx,
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1200.0, 900.0),
                    )),
                    ..Default::default()
                },
            );
        };
        for _ in 0..3 {
            render(&mut harness.app);
        }
        harness.app.virtual_pending_scroll_offset_y = Some(700.0);
        for _ in 0..3 {
            render(&mut harness.app);
        }
        let offset = harness.app.virtual_viewport.offset_y;
        assert!(offset > 600.0);
        let epoch = harness.app.active_buffer_epoch;
        for scope in [SearchScope::Title, SearchScope::Body] {
            harness.app.set_search_scope(scope);
            if !query.is_empty() {
                harness.app.maybe_dispatch_search();
                let applied_before = harness.app.query_perf.search_results_applied;
                harness.app.apply_event(CoreEvent::SearchResults {
                    collection: crate::backend::SidebarCollection::All,
                    query: query.into(),
                    scope,
                    folder_id: None,
                    language: Some("rust".into()),
                    items: vec![],
                });
                assert_eq!(
                    harness.app.query_perf.search_results_applied,
                    applied_before + 1
                );
            }
            for _ in 0..3 {
                render(&mut harness.app);
            }
            assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));
            assert_eq!(harness.app.active_buffer_epoch, epoch);
            assert_eq!(harness.app.active_snapshot(), content);
            assert_eq!(
                (
                    harness.app.virtual_editor_state.cursor(),
                    harness.app.virtual_editor_state.anchor()
                ),
                (606, Some(602))
            );
            assert!((harness.app.virtual_viewport.offset_y - offset).abs() < 1.0);
            assert!(!harness
                .cmd_rx
                .try_iter()
                .any(|cmd| matches!(cmd, CoreCmd::GetPaste { .. })));
        }
    }
}
