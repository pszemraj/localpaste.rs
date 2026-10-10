//! Backend command dispatch and repaint scheduling tests.

use super::*;

#[test]
fn sidebar_search_wakes_at_debounce_without_other_input() {
    let mut harness = make_app();
    let app = &mut harness.app;
    let ctx = egui::Context::default();
    app.virtual_editor_state.has_focus = false;
    app.focus_editor_next = false;
    for _ in 0..4 {
        run_discovery_frame_once(app, &ctx, Vec::new());
    }
    app.set_search_query("quiet search".into());
    let mut output = run_discovery_frame_once(app, &ctx, Vec::new());
    for _ in 0..3 {
        output = run_discovery_frame_once(app, &ctx, Vec::new());
    }
    let repaint = output.viewport_output[&egui::ViewportId::ROOT].repaint_delay;
    assert!(
        repaint <= SEARCH_DEBOUNCE,
        "search must schedule its own wakeup: {repaint:?}"
    );
    assert!(
        harness.cmd_rx.try_recv().is_err(),
        "search must remain debounced"
    );
    app.search_last_input_at = Some(Instant::now() - SEARCH_DEBOUNCE);
    run_discovery_frame_once(app, &ctx, Vec::new());
    assert!(matches!(
        recv_cmd(&harness.cmd_rx),
        CoreCmd::SearchPastes { .. }
    ));
    assert!(
        app.search_repaint_after(Instant::now()).is_none(),
        "sent queries must stop waking"
    );
    app.apply_event(CoreEvent::SearchFailed {
        collection: SidebarCollection::All,
        scope: app.search_scope,
        query: "quiet search".into(),
        folder_id: None,
        language: None,
        message: "search failed".into(),
    });
    assert!(
        app.search_repaint_after(Instant::now()).is_none(),
        "retained errors must stay idle"
    );
    app.set_search_query("retry query".into());
    assert!(
        app.search_repaint_after(Instant::now()).is_some(),
        "changed input must wake again"
    );
}

#[test]
fn backend_command_dispatch_arms_bounded_event_polling() {
    let mut harness = make_app();
    assert!(
        harness
            .app
            .backend_event_poll_repaint_after(Instant::now())
            .is_none(),
        "idle app should not request short backend polling"
    );

    assert!(harness.app.dispatch_backend_cmd(CoreCmd::GetPaste {
        selection_epoch: harness.app.active_buffer_epoch,
        id: "alpha".to_string(),
    }));
    assert!(matches!(
        recv_cmd(&harness.cmd_rx),
        CoreCmd::GetPaste { .. }
    ));

    let repaint_after = harness
        .app
        .backend_event_poll_repaint_after(Instant::now())
        .expect("successful dispatch should arm backend event polling");
    assert!(
        repaint_after <= BACKEND_EVENT_POLL_INTERVAL,
        "event polling should use the short interval, got {repaint_after:?}"
    );

    assert!(
        harness
            .app
            .backend_event_poll_repaint_after(
                Instant::now() + BACKEND_EVENT_POLL_WINDOW + Duration::from_millis(1),
            )
            .is_none(),
        "short polling should expire after the bounded window"
    );
}
