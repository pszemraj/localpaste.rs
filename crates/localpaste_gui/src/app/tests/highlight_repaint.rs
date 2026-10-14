//! Repaint scheduling tests for debounced, staged, and in-flight highlight work.

use super::super::highlight::{
    HighlightRender, HighlightRequest, HighlightRequestText, HighlightWorker,
};
use super::*;

/// Delay at or above which a settled frame counts as asking for no highlight wake.
const QUIET_REPAINT_DELAY: Duration = Duration::from_secs(20);

/// Offset that places a timestamp far enough ahead to make the wake delay deterministic.
const FAR_FUTURE: Duration = Duration::from_secs(10);

/// Builds an unfocused app whose highlight requests land on a returned channel.
fn make_app_with_request_channel() -> (TestHarness, Receiver<HighlightRequest>) {
    let mut harness = make_app();
    let (req_tx, req_rx) = unbounded();
    let (_evt_tx, evt_rx) = unbounded();
    harness.app.highlight_worker = HighlightWorker::from_channels(req_tx, evt_rx);
    harness.app.virtual_editor_state.has_focus = false;
    harness.app.focus_editor_next = false;
    (harness, req_rx)
}

/// Runs full update frames until egui's own start-up repaints have drained.
///
/// # Returns
/// The repaint delay requested by the last frame.
fn settled_repaint_delay(app: &mut LocalPasteApp, ctx: &egui::Context) -> Duration {
    let mut delay = Duration::ZERO;
    for _ in 0..4 {
        let output = run_full_update_with_input(
            app,
            ctx,
            egui::RawInput {
                screen_rect: Some(virtual_editor_focus_support::screen_rect()),
                ..Default::default()
            },
        );
        delay = output.viewport_output[&egui::ViewportId::ROOT].repaint_delay;
    }
    delay
}

/// Builds a render of `alpha` that the panel never treats as an exact match.
fn render_at(revision: u64, text_len: usize) -> HighlightRender {
    HighlightRender {
        paste_id: "alpha".to_string(),
        revision,
        text_len,
        base_revision: None,
        base_text_len: None,
        language_hint: "py".to_string(),
        theme_key: "base16-mocha.dark".to_string(),
        changed_line_range: None,
        lines: Vec::new(),
    }
}

#[test]
fn idle_and_in_flight_highlight_state_schedules_no_wake() {
    let (mut harness, req_rx) = make_app_with_request_channel();
    let app = &mut harness.app;
    assert!(
        app.highlight_repaint_after(Instant::now()).is_none(),
        "idle highlight state must not schedule repaints"
    );

    let ctx = egui::Context::default();
    let delay = settled_repaint_delay(app, &ctx);
    assert!(
        req_rx.try_recv().is_ok(),
        "the first frames request highlighting"
    );
    assert!(app.highlight_pending.is_some(), "the request is in flight");
    assert!(
        app.highlight_repaint_after(Instant::now()).is_none(),
        "an in-flight request is woken by the worker, not by polling"
    );
    assert!(
        delay >= QUIET_REPAINT_DELAY,
        "an in-flight request must not poll: {delay:?}"
    );
}

#[test]
fn highlight_repaint_after_reports_the_nearest_wake_and_clears_expired_markers() {
    let mut harness = make_app();
    let app = &mut harness.app;
    let now = Instant::now();
    let debounce = Duration::from_millis(30);

    app.highlight_debounce_wake_at = Some(now + debounce);
    assert_eq!(app.highlight_repaint_after(now), Some(debounce));

    app.highlight_staged = Some(render_at(0, 0));
    app.last_interaction_at = Some(now);
    assert_eq!(
        app.highlight_repaint_after(now),
        Some(debounce),
        "the nearer wake wins"
    );

    app.highlight_debounce_wake_at = None;
    assert_eq!(app.highlight_repaint_after(now), Some(HIGHLIGHT_APPLY_IDLE));

    app.highlight_debounce_wake_at = Some(now - Duration::from_millis(1));
    assert_eq!(
        app.highlight_repaint_after(now),
        Some(Duration::ZERO),
        "an expired debounce wake repaints at once"
    );
    assert!(
        app.highlight_debounce_wake_at.is_none(),
        "an expired marker must not keep waking the app"
    );
    assert_eq!(app.highlight_repaint_after(now), Some(HIGHLIGHT_APPLY_IDLE));
}

#[test]
fn debounced_highlight_request_wakes_ui_when_the_window_ends() {
    let (mut harness, req_rx) = make_app_with_request_channel();
    let app = &mut harness.app;
    let ctx = egui::Context::default();
    settled_repaint_delay(app, &ctx);
    while req_rx.try_recv().is_ok() {}

    // An edit lands while a render exists, so the next request waits out the window.
    insert_active_text(app, "x", 0);
    app.highlight_render = Some(render_at(0, 1));
    app.last_edit_at = Some(Instant::now() + FAR_FUTURE);
    let delay = settled_repaint_delay(app, &ctx);
    assert!(
        req_rx.try_recv().is_err(),
        "the request stays inside the debounce window"
    );
    assert!(
        delay > FAR_FUTURE / 2 && delay <= FAR_FUTURE + HIGHLIGHT_DEBOUNCE_MEDIUM,
        "the frame must wake when the debounce window ends: {delay:?}"
    );

    app.last_edit_at = Some(Instant::now() - HIGHLIGHT_DEBOUNCE_MEDIUM);
    let delay = settled_repaint_delay(app, &ctx);
    assert!(
        req_rx.try_recv().is_ok(),
        "the request goes out once the window has ended"
    );
    assert!(app.highlight_debounce_wake_at.is_none());
    assert!(
        delay >= QUIET_REPAINT_DELAY,
        "a dispatched request needs no further wake: {delay:?}"
    );
}

#[test]
fn staged_highlight_wakes_ui_when_the_apply_idle_wait_ends() {
    let (mut harness, _req_rx) = make_app_with_request_channel();
    let app = &mut harness.app;
    let ctx = egui::Context::default();
    settled_repaint_delay(app, &ctx);

    app.highlight_render = Some(render_at(app.active_revision(), 1));
    insert_active_text(app, "x", 0);
    let staged_revision = app.active_revision();
    app.highlight_staged = Some(render_at(staged_revision, app.active_text_len_bytes()));
    app.last_interaction_at = Some(Instant::now() + FAR_FUTURE);
    let delay = settled_repaint_delay(app, &ctx);
    assert!(
        app.highlight_staged.is_some(),
        "colors stay staged while the editor was just used"
    );
    assert!(
        delay > FAR_FUTURE / 2 && delay <= FAR_FUTURE + HIGHLIGHT_APPLY_IDLE,
        "the frame must wake when the idle wait ends: {delay:?}"
    );

    app.last_interaction_at = Some(Instant::now() - HIGHLIGHT_APPLY_IDLE);
    let delay = settled_repaint_delay(app, &ctx);
    assert!(app.highlight_staged.is_none(), "idle editor applies colors");
    assert_eq!(
        app.highlight_render.as_ref().map(|render| render.revision),
        Some(staged_revision)
    );
    assert!(
        delay >= QUIET_REPAINT_DELAY,
        "applied colors need no further wake: {delay:?}"
    );
}

#[test]
fn highlight_worker_repaints_attached_context_after_each_reply() {
    let worker = spawn_highlight_worker();
    let ctx = egui::Context::default();
    // egui reports a wake only after passes have cleared its start-up repaints.
    let settle = || {
        for _ in 0..4 {
            let _ = run_ui(&ctx, egui::RawInput::default(), |_| {});
        }
    };
    settle();
    let (wake_tx, wake_rx) = unbounded();
    ctx.set_request_repaint_callback(move |_| {
        let _ = wake_tx.send(());
    });
    worker.attach_repaint_context(&ctx);

    for revision in 0..2 {
        worker
            .tx
            .send(HighlightRequest {
                buffer_epoch: 0,
                paste_id: "alpha".to_string(),
                revision,
                text: HighlightRequestText::Rope(ropey::Rope::from_str("fn main() {}\n")),
                language_hint: "rust".to_string(),
                theme_key: "base16-mocha.dark".to_string(),
                edit_hint: None,
                patch_base_revision: None,
                patch_base_text_len: None,
            })
            .expect("worker accepts requests");
        wake_rx
            .recv_timeout(Duration::from_secs(30))
            .expect("worker must repaint the window after replying");
        assert!(
            worker.rx.try_recv().is_ok(),
            "the reply must be readable by the time the window is woken"
        );
        // Passes that serve the wake also report it through the callback.
        settle();
        while wake_rx.try_recv().is_ok() {}
    }
}
