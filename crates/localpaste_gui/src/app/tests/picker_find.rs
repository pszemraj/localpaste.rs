//! Picker query handoff across active, asynchronous, and deferred paste opens.

use super::*;

#[test]
fn opening_picker_result_focuses_editor_for_typing_and_paste() {
    for id in ["alpha", "picked"] {
        let (mut harness, _events) = make_app_with_event_tx();
        let ctx = egui::Context::default();
        let input = |events| egui::RawInput {
            screen_rect: Some(super::virtual_editor_focus_support::screen_rect()),
            events,
            ..Default::default()
        };
        harness.app.open_paste_picker();
        run_full_update_with_input(&mut harness.app, &ctx, input(vec![]));
        harness.app.open_palette_selection(id.into());
        if id == "picked" {
            let mut paste = Paste::new("content".into(), "Picked".into());
            paste.id = id.into();
            harness.app.apply_event(CoreEvent::PasteLoaded {
                paste,
                selection_epoch: harness.app.active_buffer_epoch,
            });
        }
        run_full_update_with_input(&mut harness.app, &ctx, input(vec![]));
        super::virtual_editor_focus_support::assert_editor_focus(&ctx);
        while harness.cmd_rx.try_recv().is_ok() {}
        run_full_update_with_input(
            &mut harness.app,
            &ctx,
            input(vec![
                egui::Event::Text("typed ".into()),
                egui::Event::Paste("pasted ".into()),
            ]),
        );
        assert_eq!(harness.app.active_snapshot(), "typed pasted content");
        assert_eq!(harness.app.selected_id.as_deref(), Some(id));
        assert!(!harness
            .cmd_rx
            .try_iter()
            .any(|cmd| matches!(cmd, CoreCmd::CreatePaste { .. })));
    }
}

#[test]
fn picker_find_uses_the_opening_query_and_reveals_a_distant_match() {
    let mut harness = make_app();
    harness.app.search_query = "sidebar".into();
    harness.app.editor_find.query = "previous".into();
    harness.app.editor_find.open = true;
    harness.app.editor_find.case_sensitive = true;
    harness.app.open_paste_picker();
    harness.app.set_paste_picker_query("needle_at_end".into());
    harness.app.set_paste_picker_scope(SearchScope::Body);
    harness.app.open_palette_selection("picked".into());
    assert!(matches!(recv_cmd(&harness.cmd_rx), CoreCmd::GetPaste { id, .. } if id == "picked"));
    harness.app.set_paste_picker_query("next search".into());
    harness.app.set_paste_picker_scope(SearchScope::Title);

    let content = format!(
        "{}NEEDLE_AT_END: relevant context\n",
        "unrelated line\n".repeat(100)
    );
    let match_start = "unrelated line\n".chars().count() * 100;
    let mut paste = Paste::new(content, "Picked".into());
    paste.id = "picked".into();
    harness.app.apply_event(CoreEvent::PasteLoaded {
        paste,
        selection_epoch: harness.app.active_buffer_epoch,
    });

    assert_eq!(harness.app.editor_find.query, "needle_at_end");
    assert!(!harness.app.editor_find.case_sensitive);
    assert!(harness.app.editor_find.open);
    assert!(!harness.app.editor_find.focus_requested);
    assert_eq!(
        harness.app.virtual_editor_state.selection_range(),
        Some(match_start..match_start + 13)
    );
    assert!(matches!(
        harness.app.virtual_cursor_reveal,
        Some(CursorReveal::Center)
    ));

    let ctx = egui::Context::default();
    for _ in 0..3 {
        run_full_update_with_input(
            &mut harness.app,
            &ctx,
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1100.0, 720.0),
                )),
                ..Default::default()
            },
        );
    }
    assert!(harness.app.virtual_viewport.offset_y > 0.0);
    assert!(harness.app.virtual_viewport.caret_visible());
}

#[test]
fn picker_find_reopens_the_active_unsaved_buffer_without_a_load() {
    let mut harness = make_app();
    set_active_content(&mut harness.app, "unsaved needle here");
    harness.app.save_status = SaveStatus::Dirty;
    harness.app.open_paste_picker();
    harness.app.set_paste_picker_query("needle".into());
    harness.app.set_paste_picker_scope(SearchScope::Body);

    harness.app.open_palette_selection("alpha".into());

    assert!(harness.cmd_rx.try_recv().is_err());
    assert_eq!(harness.app.active_snapshot(), "unsaved needle here");
    assert_eq!(harness.app.save_status, SaveStatus::Dirty);
    assert_eq!(harness.app.editor_find.query, "needle");
    assert_eq!(
        harness.app.virtual_editor_state.selection_range(),
        Some(8..14)
    );
    assert!(harness.app.editor_find.open);
}

#[test]
fn picker_find_survives_the_save_before_switching() {
    let mut harness = make_app();
    set_active_content(&mut harness.app, "dirty alpha");
    harness.app.save_status = SaveStatus::Dirty;
    harness.app.open_paste_picker();
    harness.app.set_paste_picker_query("needle".into());
    harness.app.set_paste_picker_scope(SearchScope::All);
    harness.app.open_palette_selection("picked".into());
    assert_eq!(harness.app.pending_selection_id.as_deref(), Some("picked"));
    assert!(matches!(
        recv_cmd(&harness.cmd_rx),
        CoreCmd::UpdatePasteVirtual { .. }
    ));
    harness.app.set_paste_picker_query("different".into());

    let mut saved = Paste::new("dirty alpha".into(), "Alpha".into());
    saved.id = "alpha".into();
    harness
        .app
        .apply_event(CoreEvent::PasteSaved { paste: saved });
    assert!(matches!(recv_cmd(&harness.cmd_rx), CoreCmd::GetPaste { id, .. } if id == "picked"));
    let mut picked = Paste::new("before needle after".into(), "Picked".into());
    picked.id = "picked".into();
    harness.app.apply_event(CoreEvent::PasteLoaded {
        paste: picked,
        selection_epoch: harness.app.active_buffer_epoch,
    });

    assert_eq!(harness.app.editor_find.query, "needle");
    assert_eq!(
        harness.app.virtual_editor_state.selection_range(),
        Some(7..13)
    );
    assert_eq!(harness.app.selected_id.as_deref(), Some("picked"));
}

#[test]
fn picker_find_keeps_existing_find_for_metadata_only_hits() {
    for (scope, body) in [
        (SearchScope::Title, "needle in body; sidebar context"),
        (SearchScope::Metadata, "needle in body; sidebar context"),
        (SearchScope::All, "sidebar context only"),
    ] {
        let mut harness = make_app();
        harness.app.search_query = "sidebar".into();
        harness.app.editor_find.query = "retained find".into();
        harness.app.open_paste_picker();
        harness.app.set_paste_picker_query("needle".into());
        harness.app.set_paste_picker_scope(scope);
        harness.app.open_palette_selection("picked".into());
        let _ = recv_cmd(&harness.cmd_rx);
        let mut picked = Paste::new(body.into(), "Needle title".into());
        picked.id = "picked".into();
        harness.app.apply_event(CoreEvent::PasteLoaded {
            paste: picked,
            selection_epoch: harness.app.active_buffer_epoch,
        });

        assert_eq!(
            harness.app.editor_find.query, "retained find",
            "scope={scope:?}"
        );
        assert!(!harness.app.editor_find.open, "scope={scope:?}");
        assert!(harness.app.virtual_editor_state.selection_range().is_none());
    }
}

#[test]
fn picker_find_discards_superseded_opens() {
    let mut harness = make_app();
    harness.app.editor_find.query = "retained find".into();
    harness.app.open_paste_picker();
    harness.app.set_paste_picker_query("needle".into());
    harness.app.open_palette_selection("picked".into());
    let _ = recv_cmd(&harness.cmd_rx);
    assert!(harness.app.pending_picker_open.is_some());
    assert!(harness.app.select_paste("other".into()));
    let _ = recv_cmd(&harness.cmd_rx);
    assert!(harness.app.pending_picker_open.is_none());

    let mut stale = Paste::new("needle".into(), "Picked".into());
    stale.id = "picked".into();
    harness.app.apply_event(CoreEvent::PasteLoaded {
        paste: stale,
        selection_epoch: harness.app.active_buffer_epoch,
    });
    assert_eq!(harness.app.selected_id.as_deref(), Some("other"));
    assert_eq!(harness.app.editor_find.query, "retained find");
}
