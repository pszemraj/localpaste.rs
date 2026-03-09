//! Regression coverage for mutually exclusive version-overlay ownership.

use super::*;
use crossbeam_channel::TryRecvError;

const REENTRY_STATUS: &str = "Close the open version window before opening another one.";

#[test]
fn history_modal_rejects_opening_diff_modal_while_it_owns_version_workflow() {
    let mut harness = make_app();
    harness.app.version_ui.history_modal_open = true;
    harness.app.version_ui.history_selected_index = 1;

    harness.app.open_diff_modal();

    assert!(harness.app.version_ui.history_modal_open);
    assert_eq!(harness.app.version_ui.history_selected_index, 1);
    assert!(
        !harness.app.version_ui.diff_modal_open,
        "history should remain the sole active version overlay"
    );
    assert_eq!(
        harness
            .app
            .status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some(REENTRY_STATUS)
    );
    assert!(matches!(
        harness.cmd_rx.try_recv(),
        Err(TryRecvError::Empty)
    ));
}

#[test]
fn diff_modal_rejects_opening_history_modal_while_it_owns_version_workflow() {
    let mut harness = make_app();
    harness.app.version_ui.diff_modal_open = true;
    harness.app.version_ui.diff_query = "beta".to_string();
    harness.app.version_ui.diff_target_id = Some("beta".to_string());

    harness.app.open_history_modal();

    assert!(harness.app.version_ui.diff_modal_open);
    assert_eq!(harness.app.version_ui.diff_query, "beta");
    assert_eq!(
        harness.app.version_ui.diff_target_id.as_deref(),
        Some("beta")
    );
    assert!(
        !harness.app.version_ui.history_modal_open,
        "diff should remain the sole active version overlay"
    );
    assert_eq!(
        harness
            .app
            .status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some(REENTRY_STATUS)
    );
    assert!(matches!(
        harness.cmd_rx.try_recv(),
        Err(TryRecvError::Empty)
    ));
}

#[test]
fn history_reset_confirm_rejects_opening_diff_modal() {
    let mut harness = make_app();
    harness.app.version_ui.history_modal_open = true;
    harness.app.version_ui.history_reset_confirm_open = true;
    harness.app.version_ui.history_reset_confirm_target = Some(42);

    harness.app.open_diff_modal();

    assert!(harness.app.version_ui.history_modal_open);
    assert!(harness.app.version_ui.history_reset_confirm_open);
    assert_eq!(
        harness.app.version_ui.history_reset_confirm_target,
        Some(42)
    );
    assert!(
        !harness.app.version_ui.diff_modal_open,
        "history reset confirm should keep exclusive modal ownership"
    );
    assert_eq!(
        harness
            .app
            .status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some(REENTRY_STATUS)
    );
    assert!(matches!(
        harness.cmd_rx.try_recv(),
        Err(TryRecvError::Empty)
    ));
}
