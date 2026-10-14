//! Paste-creation tests for selection and sidebar projection behavior.

use super::*;

#[test]
fn paste_created_clean_visible_focuses_editor_after_content_on_next_render() {
    let mut harness = make_app();

    let content = "café 🦀\nnew-content";
    let mut created = Paste::new(content.to_string(), "new-note".to_string());
    created.id = "new-id".to_string();

    harness
        .app
        .apply_event(CoreEvent::PasteCreated { paste: created });

    assert_eq!(harness.app.selected_id.as_deref(), Some("new-id"));
    assert!(
        harness.app.focus_editor_next,
        "clean visible paste creation should queue a virtual-editor focus request"
    );

    let ctx = egui::Context::default();
    configure_virtual_editor_test_ctx(&ctx);
    run_editor_panel_once(&mut harness.app, &ctx, egui::RawInput::default());

    let editor_id = egui::Id::new(VIRTUAL_EDITOR_ID);
    assert!(ctx.memory(|m| m.has_focus(editor_id)));
    assert!(
        !harness.app.focus_editor_next,
        "virtual editor should consume the one-frame focus request"
    );
    assert_eq!(
        harness.app.virtual_editor_state.cursor(),
        content.chars().count()
    );
    run_editor_panel_once(
        &mut harness.app,
        &ctx,
        egui::RawInput {
            events: vec![egui::Event::Paste("next".into())],
            ..Default::default()
        },
    );
    assert_eq!(harness.app.active_snapshot(), format!("{content}next"));
}

/// A harness whose Rust language filter lists only `alpha`, and a created Python paste it hides.
fn rust_filtered_harness() -> (TestHarness, Paste) {
    let mut harness = make_app();
    assert!(harness.app.acquire_paste_lock("alpha"));
    harness.app.all_pastes = vec![test_summary("alpha", "Alpha", Some("rust"), 7)];
    harness.app.pastes = harness.app.all_pastes.clone().into();
    harness
        .app
        .set_active_language_filter(Some("rust".to_string()));

    let mut created = Paste::new("print('hi')".to_string(), "new-note".to_string());
    created.id = "new-id".to_string();
    created.language = Some("python".to_string());
    created.language_is_manual = true;
    (harness, created)
}

fn visible_ids(app: &LocalPasteApp) -> Vec<&str> {
    app.pastes.iter().map(|item| item.id.as_str()).collect()
}

/// Applies a background list refresh carrying the cached rows.
fn refresh_list(app: &mut LocalPasteApp) {
    let items = app.all_pastes.clone();
    app.apply_event(CoreEvent::PasteList { items });
}

fn status_text(app: &LocalPasteApp) -> Option<&str> {
    app.status.as_ref().map(|status| status.text.as_str())
}

#[test]
fn paste_created_hidden_by_active_filter_opens_and_survives_refreshes() {
    let (mut harness, created) = rust_filtered_harness();

    harness
        .app
        .apply_event(CoreEvent::PasteCreated { paste: created });

    assert!(
        harness
            .app
            .all_pastes
            .iter()
            .any(|item| item.id == "new-id"),
        "canonical cache should include the newly created paste"
    );
    assert_eq!(
        visible_ids(&harness.app),
        ["alpha"],
        "visible projection should continue honoring the active language filter"
    );
    assert_eq!(harness.app.selected_id.as_deref(), Some("new-id"));
    assert_eq!(harness.app.active_snapshot(), "print('hi')");
    assert!(
        harness.app.focus_editor_next,
        "typing goes to the new paste, not the one it replaced"
    );
    assert_eq!(
        status_text(&harness.app),
        Some("Created new paste; current filters hide it in the sidebar.")
    );
    assert!(harness.app.toasts.is_empty());
    assert!(matches!(
        harness.cmd_rx.try_recv(),
        Err(TryRecvError::Empty)
    ));

    refresh_list(&mut harness.app);
    assert_eq!(visible_ids(&harness.app), ["alpha"]);
    assert_eq!(harness.app.selected_id.as_deref(), Some("new-id"));
    assert!(matches!(
        harness.cmd_rx.try_recv(),
        Err(TryRecvError::Empty)
    ));
}

#[test]
fn paste_created_during_active_search_opens_and_survives_the_search_reply() {
    let mut harness = make_app();
    assert!(harness.app.acquire_paste_lock("alpha"));
    harness.app.search_query = "alpha".to_string();
    harness.app.search_last_sent = "alpha".to_string();
    harness.app.search_last_input_at = None;
    harness.app.pastes = vec![test_summary("alpha", "Alpha", None, 7)].into();
    harness.app.all_pastes = harness.app.pastes.clone();

    let mut created = Paste::new("new-content".to_string(), "Gamma".to_string());
    created.id = "new-id".to_string();

    harness
        .app
        .apply_event(CoreEvent::PasteCreated { paste: created });

    assert!(
        harness
            .app
            .all_pastes
            .iter()
            .any(|item| item.id == "new-id"),
        "canonical cache should include the newly created paste"
    );
    assert_eq!(
        visible_ids(&harness.app),
        ["alpha"],
        "active search results should remain the visible projection until refreshed"
    );
    assert_eq!(harness.app.selected_id.as_deref(), Some("new-id"));
    assert!(harness.app.focus_editor_next);
    assert!(
        harness.app.search_last_sent.is_empty(),
        "create should invalidate active-search cache"
    );
    assert!(
        harness.app.search_last_input_at.is_some(),
        "search dispatch timestamp should be rewound so refresh happens immediately"
    );
    assert_eq!(
        status_text(&harness.app),
        Some("Created new paste; refreshing search results.")
    );

    harness.app.maybe_dispatch_search();
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::SearchPastes {
            collection: crate::backend::SidebarCollection::All,
            scope: localpaste_core::models::paste::SearchScope::All,
            query,
            ..
        } => assert_eq!(query, "alpha"),
        other => panic!("unexpected command: {:?}", other),
    }

    // The refreshed results still leave the new paste out.
    harness.app.apply_event(CoreEvent::SearchResults {
        collection: SidebarCollection::All,
        scope: localpaste_core::models::paste::SearchScope::All,
        query: "alpha".to_string(),
        folder_id: None,
        language: None,
        items: vec![test_summary("alpha", "Alpha", None, 7)],
    });
    assert_eq!(visible_ids(&harness.app), ["alpha"]);
    assert_eq!(harness.app.selected_id.as_deref(), Some("new-id"));
}

#[test]
fn paste_created_hidden_while_dirty_opens_after_the_save_and_survives_refreshes() {
    let (mut harness, created) = rust_filtered_harness();
    set_active_content(&mut harness.app, "edited-old");
    harness.app.save_status = SaveStatus::Dirty;
    harness.app.last_edit_at = Some(Instant::now());

    harness
        .app
        .apply_event(CoreEvent::PasteCreated { paste: created });

    assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));
    assert_eq!(harness.app.pending_selection_id.as_deref(), Some("new-id"));
    assert_eq!(harness.app.active_snapshot(), "edited-old");
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::UpdatePasteVirtual { id, .. } => assert_eq!(id, "alpha"),
        other => panic!("unexpected command: {:?}", other),
    }

    // A refresh before the save ack keeps the queued switch.
    refresh_list(&mut harness.app);
    assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));
    assert_eq!(harness.app.pending_selection_id.as_deref(), Some("new-id"));

    let mut saved = Paste::new("edited-old".to_string(), "Alpha".to_string());
    saved.id = "alpha".to_string();
    saved.language = Some("rust".to_string());
    saved.language_is_manual = true;
    harness
        .app
        .apply_event(CoreEvent::PasteSaved { paste: saved });

    assert_eq!(harness.app.selected_id.as_deref(), Some("new-id"));
    assert!(harness.app.pending_selection_id.is_none());
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::GetPaste { id, .. } => assert_eq!(id, "new-id"),
        other => panic!("unexpected command: {:?}", other),
    }

    refresh_list(&mut harness.app);
    assert_eq!(visible_ids(&harness.app), ["alpha"]);
    assert_eq!(harness.app.selected_id.as_deref(), Some("new-id"));
    assert!(matches!(
        harness.cmd_rx.try_recv(),
        Err(TryRecvError::Empty)
    ));
}

#[test]
fn paste_created_hidden_during_version_overlay_opens_when_it_closes() {
    let (mut harness, created) = rust_filtered_harness();
    harness.app.version_ui.history_modal_open = true;

    harness
        .app
        .apply_event(CoreEvent::PasteCreated { paste: created });
    assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));
    assert_eq!(harness.app.pending_selection_id.as_deref(), Some("new-id"));

    harness.app.close_history_modal();
    assert_eq!(harness.app.selected_id.as_deref(), Some("new-id"));
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::GetPaste { id, .. } => assert_eq!(id, "new-id"),
        other => panic!("unexpected command: {:?}", other),
    }

    refresh_list(&mut harness.app);
    assert_eq!(harness.app.selected_id.as_deref(), Some("new-id"));
}
