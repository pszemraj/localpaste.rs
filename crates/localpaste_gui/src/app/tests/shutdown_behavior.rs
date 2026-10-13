//! Shutdown-flush command emission tests.

use super::*;

#[test]
fn on_exit_dispatches_dirty_content_and_metadata_save_and_drop_releases_selected_lock() {
    let TestHarness {
        _dir: _guard,
        _highlight_requests,
        mut app,
        cmd_rx,
    } = make_app();
    set_active_content(&mut app, "exit-save-content");
    app.save_status = SaveStatus::Dirty;
    app.save_in_flight = false;
    app.last_edit_at = Some(Instant::now());
    app.edit_name = "exit-name".to_string();
    app.edit_language = Some("rust".to_string());
    app.edit_language_is_manual = true;
    app.edit_tags = "one, two".to_string();
    app.metadata_dirty = true;
    app.metadata_save_in_flight = false;
    app.locks
        .acquire("alpha", &app.lock_owner_id)
        .expect("acquire alpha lock");
    let locks = app.locks.clone();

    eframe::App::on_exit(&mut app);

    match recv_cmd(&cmd_rx) {
        CoreCmd::UpdatePasteVirtual { id, content, .. } => {
            assert_eq!(id, "alpha");
            assert_eq!(content.to_string(), "exit-save-content");
        }
        other => panic!("unexpected command: {:?}", other),
    }
    match recv_cmd(&cmd_rx) {
        CoreCmd::UpdatePasteMeta {
            id,
            name,
            language,
            language_is_manual,
            folder_id,
            tags,
        } => {
            assert_eq!(id, "alpha");
            assert_eq!(name.as_deref(), Some("exit-name"));
            assert_eq!(language.as_deref(), Some("rust"));
            assert_eq!(language_is_manual, Some(true));
            assert!(folder_id.is_none());
            assert_eq!(tags, Some(vec!["one".to_string(), "two".to_string()]));
        }
        other => panic!("unexpected command: {:?}", other),
    }
    drop(app);
    assert!(
        !locks.is_locked("alpha").expect("is_locked"),
        "drop should release the selected paste lock"
    );
}

#[test]
fn on_exit_dispatches_dirty_saves_even_with_version_overlay_open() {
    enum OverlayCase {
        History,
        Diff,
    }

    for overlay in [OverlayCase::History, OverlayCase::Diff] {
        let mut harness = make_app();
        set_active_content(&mut harness.app, "overlay-exit-content");
        harness.app.save_status = SaveStatus::Dirty;
        harness.app.save_in_flight = false;
        harness.app.last_edit_at = Some(Instant::now());
        harness.app.edit_name = "overlay-exit-name".to_string();
        harness.app.edit_language = Some("rust".to_string());
        harness.app.edit_language_is_manual = true;
        harness.app.edit_tags = "one, two".to_string();
        harness.app.metadata_dirty = true;
        harness.app.metadata_save_in_flight = false;

        match overlay {
            OverlayCase::History => harness.app.version_ui.history_modal_open = true,
            OverlayCase::Diff => harness.app.version_ui.diff_modal_open = true,
        }

        eframe::App::on_exit(&mut harness.app);

        let first = recv_cmd(&harness.cmd_rx);
        let second = recv_cmd(&harness.cmd_rx);
        let mut saw_content = false;
        let mut saw_metadata = false;

        for cmd in [first, second] {
            match cmd {
                CoreCmd::UpdatePasteVirtual { id, content, .. } => {
                    assert_eq!(id, "alpha");
                    assert_eq!(content.to_string(), "overlay-exit-content");
                    saw_content = true;
                }
                CoreCmd::UpdatePasteMeta {
                    id,
                    name,
                    language,
                    language_is_manual,
                    folder_id,
                    tags,
                } => {
                    assert_eq!(id, "alpha");
                    assert_eq!(name.as_deref(), Some("overlay-exit-name"));
                    assert_eq!(language.as_deref(), Some("rust"));
                    assert_eq!(language_is_manual, Some(true));
                    assert!(folder_id.is_none());
                    assert_eq!(tags, Some(vec!["one".to_string(), "two".to_string()]));
                    saw_metadata = true;
                }
                other => panic!("unexpected command: {:?}", other),
            }
        }

        assert!(saw_content, "expected shutdown to persist dirty content");
        assert!(saw_metadata, "expected shutdown to persist dirty metadata");
    }
}

#[test]
fn on_exit_requeues_dirty_tails_after_stale_in_flight_acks() {
    let (mut harness, evt_tx) = make_app_with_event_tx();
    set_active_content(&mut harness.app, "exit-newer-content");
    harness.app.save_status = SaveStatus::Dirty;
    harness.app.save_in_flight = true;
    harness.app.save_request_revision = None;
    harness.app.last_edit_at = Some(Instant::now());
    harness.app.edit_name = "new-name".to_string();
    harness.app.edit_language = Some("rust".to_string());
    harness.app.edit_language_is_manual = true;
    harness.app.edit_tags = "one, two".to_string();
    harness.app.metadata_dirty = true;
    harness.app.metadata_save_in_flight = true;
    harness.app.metadata_save_request = Some(MetadataDraftSnapshot {
        name: "old-name".to_string(),
        language: None,
        language_is_manual: false,
        tags_csv: String::new(),
    });

    std::thread::spawn(move || {
        let mut stale_content = Paste::new("stale-content".to_string(), "Alpha".to_string());
        stale_content.id = "alpha".to_string();
        let _ = evt_tx.send(CoreEvent::PasteSaved {
            paste: stale_content,
        });

        let mut stale_meta = Paste::new("stale-content".to_string(), "old-name".to_string());
        stale_meta.id = "alpha".to_string();
        stale_meta.language = None;
        stale_meta.language_is_manual = false;
        stale_meta.tags = Vec::new();
        let _ = evt_tx.send(CoreEvent::PasteMetaSaved { paste: stale_meta });
    });

    eframe::App::on_exit(&mut harness.app);

    let first = recv_cmd(&harness.cmd_rx);
    let second = recv_cmd(&harness.cmd_rx);
    let mut saw_content = false;
    let mut saw_metadata = false;

    for cmd in [first, second] {
        match cmd {
            CoreCmd::UpdatePasteVirtual { id, content, .. } => {
                assert_eq!(id, "alpha");
                assert_eq!(content.to_string(), "exit-newer-content");
                saw_content = true;
            }
            CoreCmd::UpdatePasteMeta {
                id,
                name,
                language,
                language_is_manual,
                folder_id,
                tags,
            } => {
                assert_eq!(id, "alpha");
                assert_eq!(name.as_deref(), Some("new-name"));
                assert_eq!(language.as_deref(), Some("rust"));
                assert_eq!(language_is_manual, Some(true));
                assert!(folder_id.is_none());
                assert_eq!(tags, Some(vec!["one".to_string(), "two".to_string()]));
                saw_metadata = true;
            }
            other => panic!("unexpected command: {:?}", other),
        }
    }

    assert!(
        saw_content,
        "expected shutdown to queue a final content save"
    );
    assert!(
        saw_metadata,
        "expected shutdown to queue a final metadata save"
    );
}

#[test]
fn on_exit_forces_final_dirty_snapshots_even_when_acks_never_arrive() {
    let mut harness = make_app();
    set_active_content(&mut harness.app, "shutdown-final-content");
    harness.app.save_status = SaveStatus::Dirty;
    harness.app.save_in_flight = true;
    harness.app.save_request_revision = None;
    harness.app.last_edit_at = Some(Instant::now());
    harness.app.edit_name = "shutdown-name".to_string();
    harness.app.edit_language = Some("rust".to_string());
    harness.app.edit_language_is_manual = true;
    harness.app.edit_tags = "one, two".to_string();
    harness.app.metadata_dirty = true;
    harness.app.metadata_save_in_flight = true;
    harness.app.metadata_save_request = Some(MetadataDraftSnapshot {
        name: "old-name".to_string(),
        language: None,
        language_is_manual: false,
        tags_csv: String::new(),
    });

    eframe::App::on_exit(&mut harness.app);

    let first = recv_cmd(&harness.cmd_rx);
    let second = recv_cmd(&harness.cmd_rx);
    let mut saw_content = false;
    let mut saw_metadata = false;

    for cmd in [first, second] {
        match cmd {
            CoreCmd::UpdatePasteVirtual { id, content, .. } => {
                assert_eq!(id, "alpha");
                assert_eq!(content.to_string(), "shutdown-final-content");
                saw_content = true;
            }
            CoreCmd::UpdatePasteMeta {
                id,
                name,
                language,
                language_is_manual,
                folder_id,
                tags,
            } => {
                assert_eq!(id, "alpha");
                assert_eq!(name.as_deref(), Some("shutdown-name"));
                assert_eq!(language.as_deref(), Some("rust"));
                assert_eq!(language_is_manual, Some(true));
                assert!(folder_id.is_none());
                assert_eq!(tags, Some(vec!["one".to_string(), "two".to_string()]));
                saw_metadata = true;
            }
            other => panic!("unexpected command: {:?}", other),
        }
    }

    assert!(
        saw_content,
        "expected shutdown to force a final content save without waiting for ack"
    );
    assert!(
        saw_metadata,
        "expected shutdown to force a final metadata save without waiting for ack"
    );
}

#[test]
fn on_exit_saves_input_accepted_while_its_paste_loads() {
    for case in [
        "append",
        "picker text",
        "picker paste",
        "picker cut",
        "append then picker text",
    ] {
        let mut harness = make_app();
        let dir = TempDir::new().expect("temp dir");
        let db_path = dir.path().join("db").to_string_lossy().into_owned();
        let db = Database::new(&db_path).expect("db");
        let mut alpha = Paste::new("existing".into(), "Alpha".into());
        alpha.id = "alpha".into();
        db.pastes.create(&alpha).expect("create alpha");
        harness.app.clear_selection();
        harness.app.backend = crate::backend::spawn_backend_with_locks_and_owner(
            db.share().expect("share db"),
            10 * 1024 * 1024,
            harness.app.locks.clone(),
            harness.app.lock_owner_id.clone(),
        );
        let ctx = egui::Context::default();

        // No frame runs between accepting the input and exit, so the load
        // reply is still queued when the shutdown flush starts.
        let expected = match case {
            "append" => {
                harness.app.paste_into_editor(&ctx, vec!["appended".into()]);
                "existing\nappended"
            }
            _ => {
                // An append queued before the picker opens the same paste applies
                // ahead of the input the picker retains, as it does in a frame.
                if case.starts_with("append") {
                    harness.app.paste_into_editor(&ctx, vec!["A".into()]);
                }
                harness.app.open_palette_selection("alpha".into());
                let (events, expected) = match case {
                    "picker text" => (vec![egui::Event::Text("typed ".into())], "typed existing"),
                    "picker paste" => (
                        vec![egui::Event::Paste("pasted ".into())],
                        "pasted existing",
                    ),
                    "picker cut" => (
                        vec![command_key_event(egui::Key::A), egui::Event::Cut],
                        "existing",
                    ),
                    _ => (vec![egui::Event::Text("B".into())], "existing\nAB"),
                };
                let mut input = egui::RawInput {
                    events,
                    ..Default::default()
                };
                harness.app.stage_discovery_input(&ctx, &mut input);
                assert!(
                    input.events.is_empty(),
                    "{case}: the opening paste holds the input"
                );
                expected
            }
        };
        assert!(
            harness.app.selected_paste.is_none(),
            "{case}: the paste is still loading"
        );

        eframe::App::on_exit(&mut harness.app);
        drop(harness);
        drop(db);
        let reopened = Database::new(&db_path).expect("reopen db");
        let saved = reopened.pastes.get("alpha").expect("read alpha");
        assert_eq!(saved.expect("alpha").content, expected, "{case}");
    }
}

#[test]
fn native_close_drains_ordered_discovery_input_before_persisting() {
    for (title_focus, query_text) in [(false, ""), (true, ""), (false, "query"), (true, "query")] {
        let mut harness = make_app();
        let dir = TempDir::new().expect("temp dir");
        let db_path = dir.path().join("db").to_string_lossy().into_owned();
        let db = Database::new(&db_path).expect("db");
        let mut alpha = Paste::new("existing".into(), "Alpha".into());
        alpha.id = "alpha".into();
        db.pastes.create(&alpha).expect("create alpha");
        harness.app.backend = crate::backend::spawn_backend_with_locks_and_owner(
            db.share().expect("share db"),
            10 * 1024 * 1024,
            harness.app.locks.clone(),
            harness.app.lock_owner_id.clone(),
        );
        harness.app.apply_event(CoreEvent::PasteLoaded {
            paste: alpha,
            selection_epoch: harness.app.active_buffer_epoch,
        });
        let ctx = egui::Context::default();
        harness.app.focus_editor_next = !title_focus;
        let _ = run_discovery_frame_once(&mut harness.app, &ctx, vec![]);
        if title_focus {
            ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(TITLE_INPUT_ID)));
            let _ = run_discovery_frame_once(&mut harness.app, &ctx, vec![]);
        } else {
            let end = harness.app.virtual_editor_buffer.len_chars();
            harness.app.virtual_editor_state.set_cursor(end, end);
        }

        // eframe renders one frame for a native Close, then exits unless that
        // frame emits CancelClose. Repaints alone cannot drain an input suffix.
        let mut events = vec![key_event(egui::Key::F1, egui::Modifiers::NONE)];
        if !query_text.is_empty() {
            events.push(egui::Event::Text(query_text.into()));
        }
        events.extend([
            key_event(egui::Key::F1, egui::Modifiers::NONE),
            egui::Event::Text("tail".into()),
        ]);
        let mut input = egui::RawInput {
            screen_rect: Some(virtual_editor_focus_support::screen_rect()),
            events,
            ..Default::default()
        };
        input
            .viewports
            .get_mut(&egui::ViewportId::ROOT)
            .expect("root viewport")
            .events
            .push(egui::ViewportEvent::Close);
        let output = run_full_update_with_input(&mut harness.app, &ctx, input);
        assert!(
            output.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .contains(&egui::ViewportCommand::CancelClose),
            "native close must wait for accepted input; title_focus={title_focus}"
        );
        assert!(!harness.app.deferred_discovery_events.is_empty());

        let mut closed = false;
        for _ in 0..5 {
            let output = run_discovery_frame_once(&mut harness.app, &ctx, vec![]);
            if output.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .contains(&egui::ViewportCommand::Close)
            {
                closed = true;
                break;
            }
        }
        assert!(closed, "the original close request must resume");
        assert!(harness.app.deferred_discovery_events.is_empty());
        assert_eq!(harness.app.shortcut_help_query, query_text);
        eframe::App::on_exit(&mut harness.app);
        drop(harness);
        drop(db);
        let reopened = Database::new(&db_path).expect("reopen db");
        let saved = reopened
            .pastes
            .get("alpha")
            .expect("read alpha")
            .expect("alpha");
        assert_eq!(
            saved.content,
            if title_focus {
                "existing"
            } else {
                "existingtail"
            }
        );
        assert_eq!(saved.name, if title_focus { "Alphatail" } else { "Alpha" });
    }
}

#[test]
fn native_close_drains_loading_editor_input_before_saving() {
    for (stalled_load, cut) in [(false, true), (true, true), (false, false), (true, false)] {
        let (mut harness, _event_tx) = make_app_with_event_tx();
        let dir = TempDir::new().expect("temp dir");
        let db_path = dir.path().join("db").to_string_lossy().into_owned();
        let db = Database::new(&db_path).expect("db");
        let mut alpha = Paste::new("existing".into(), "Alpha".into());
        alpha.id = "alpha".into();
        db.pastes.create(&alpha).expect("create alpha");
        harness.app.clear_selection();
        let ctx = egui::Context::default();
        let events = if cut {
            harness.app.open_palette_selection("alpha".into());
            vec![command_key_event(egui::Key::A), egui::Event::Cut]
        } else {
            harness.app.paste_into_editor(&ctx, vec!["tail".into()]);
            vec![]
        };
        let mut input = egui::RawInput {
            screen_rect: Some(virtual_editor_focus_support::screen_rect()),
            events,
            ..Default::default()
        };
        input
            .viewports
            .get_mut(&egui::ViewportId::ROOT)
            .expect("root viewport")
            .events
            .push(egui::ViewportEvent::Close);
        let output = run_full_update_with_input(&mut harness.app, &ctx, input);
        assert!(
            output.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .contains(&egui::ViewportCommand::CancelClose),
            "native close must wait for input retained with an outstanding load"
        );
        assert!(harness.app.selected_paste.is_none());

        if stalled_load {
            harness.app.deferred_close_started_at =
                Some(Instant::now() - SHUTDOWN_SAVE_FLUSH_TIMEOUT - Duration::from_millis(1));
            let output = run_discovery_frame_once(&mut harness.app, &ctx, vec![]);
            assert!(output.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .contains(&egui::ViewportCommand::CancelClose));
            assert!(harness.app.deferred_close_started_at.is_none());
            if cut {
                assert_eq!(
                    harness
                        .app
                        .pending_picker_open
                        .as_ref()
                        .expect("retained input")
                        .input_events
                        .len(),
                    2,
                    "canceling close must preserve the accepted commands"
                );
            } else {
                assert!(harness.app.pending_editor_paste.is_some());
            }
        }

        harness.app.backend = crate::backend::spawn_backend_with_locks_and_owner(
            db.share().expect("share db"),
            10 * 1024 * 1024,
            harness.app.locks.clone(),
            harness.app.lock_owner_id.clone(),
        );
        harness.app.apply_event(CoreEvent::PasteLoaded {
            paste: alpha,
            selection_epoch: harness.app.active_buffer_epoch,
        });
        let output = run_discovery_frame_once(&mut harness.app, &ctx, vec![]);
        if cut {
            assert!(output.platform_output.commands.iter().any(|command| {
                matches!(command, egui::OutputCommand::CopyText(text) if text == "existing")
            }));
        }
        let close_commands = &output.viewport_output[&egui::ViewportId::ROOT].commands;
        if stalled_load {
            assert!(!close_commands.contains(&egui::ViewportCommand::Close));
            // Once a stalled close was canceled, loading finishes without an
            // unexpected later exit. The user can issue a fresh close normally.
            let mut input = egui::RawInput {
                screen_rect: Some(virtual_editor_focus_support::screen_rect()),
                ..Default::default()
            };
            input
                .viewports
                .get_mut(&egui::ViewportId::ROOT)
                .expect("root viewport")
                .events
                .push(egui::ViewportEvent::Close);
            let output = run_full_update_with_input(&mut harness.app, &ctx, input);
            assert!(!output.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .contains(&egui::ViewportCommand::CancelClose));
        } else {
            assert!(close_commands.contains(&egui::ViewportCommand::Close));
        }
        eframe::App::on_exit(&mut harness.app);
        drop(harness);
        drop(db);
        let reopened = Database::new(&db_path).expect("reopen db");
        assert_eq!(
            reopened
                .pastes
                .get("alpha")
                .expect("read alpha")
                .expect("alpha")
                .content,
            if cut { "" } else { "existing\ntail" }
        );
    }
}
