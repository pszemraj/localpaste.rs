//! Backend command dispatch and repaint scheduling tests.

use super::*;

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
