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

#[test]
fn sidebar_search_resends_a_query_whose_reply_arrived_while_it_was_edited() {
    for failed in [false, true] {
        let mut harness = make_app();
        let app = &mut harness.app;
        let send = |app: &mut LocalPasteApp, query: &str| {
            app.set_search_query(query.into());
            app.search_last_input_at = Some(Instant::now() - SEARCH_DEBOUNCE);
            app.maybe_dispatch_search();
        };
        let reply = |query: &str| {
            let (collection, scope) = (SidebarCollection::All, SearchScope::All);
            if failed {
                CoreEvent::SearchFailed {
                    collection,
                    scope,
                    query: query.into(),
                    folder_id: None,
                    language: None,
                    message: "Search failed: boom".into(),
                }
            } else {
                CoreEvent::SearchResults {
                    collection,
                    scope,
                    query: query.into(),
                    folder_id: None,
                    language: None,
                    items: vec![test_summary("hit", "Hit", None, 3)],
                }
            }
        };
        send(app, "abc");
        assert!(matches!(
            recv_cmd(&harness.cmd_rx),
            CoreCmd::SearchPastes { query, .. } if query == "abc"
        ));
        // The reply lands while the query reads "abcd", then the edit is undone.
        app.set_search_query("abcd".into());
        app.apply_event(reply("abc"));
        send(app, "abc");

        assert!(
            matches!(
                harness.cmd_rx.try_recv(),
                Ok(CoreCmd::SearchPastes { query, .. }) if query == "abc"
            ),
            "failed={failed}"
        );
        app.apply_event(reply("abc"));
        if failed {
            assert!(app.search_error.is_some());
        } else {
            assert_eq!(app.pastes.len(), 1);
            assert_eq!(app.pastes[0].id, "hit");
        }
    }
}

/// Runs full update frames until egui's start-up repaints and fade-ins drain.
///
/// # Returns
/// The repaint delay requested by the last frame.
fn settled_repaint_delay(app: &mut LocalPasteApp, ctx: &egui::Context) -> Duration {
    let mut delay = Duration::ZERO;
    for _ in 0..10 {
        delay = run_discovery_frame_once(app, ctx, Vec::new()).viewport_output
            [&egui::ViewportId::ROOT]
            .repaint_delay;
    }
    delay
}

#[test]
fn idle_frames_wake_when_the_external_refresh_falls_due() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    harness.app.virtual_editor_state.has_focus = false;
    harness.app.focus_editor_next = false;
    // Longer than every status, toast, and backend-poll wake, so only the
    // refresh deadline can explain a delay this short.
    let remaining = Duration::from_secs(12);
    harness.app.last_refresh_at = Instant::now() - (EXTERNAL_REFRESH_INTERVAL - remaining);
    let delay = settled_repaint_delay(&mut harness.app, &ctx);
    assert!(
        delay <= remaining && delay > remaining - Duration::from_secs(2),
        "the idle wake must land when the refresh falls due: {delay:?}"
    );
    assert!(
        harness.cmd_rx.try_recv().is_err(),
        "the refresh is not due yet"
    );

    // A due refresh that cannot be sent retries later instead of spinning.
    drop(std::mem::replace(&mut harness.cmd_rx, unbounded().1));
    harness.app.last_refresh_at = Instant::now() - EXTERNAL_REFRESH_INTERVAL;
    let delay = settled_repaint_delay(&mut harness.app, &ctx);
    assert!(
        delay > Duration::from_secs(1),
        "an unsent refresh must not request back-to-back frames: {delay:?}"
    );
}
