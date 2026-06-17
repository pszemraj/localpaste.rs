//! History reset save-and-flush regression tests.

use super::*;

#[test]
fn queued_history_reset_redispatches_dirty_metadata_after_stale_ack() {
    let mut harness = make_app();
    harness.app.edit_name = "First name".to_string();
    harness.app.metadata_dirty = true;
    harness.app.save_metadata_now();
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::UpdatePasteMeta { id, name, .. } => {
            assert_eq!(id, "alpha");
            assert_eq!(name.as_deref(), Some("First name"));
        }
        other => panic!("expected initial metadata save, got {:?}", other),
    }
    assert!(harness.app.metadata_save_in_flight);

    harness.app.edit_name = "Second name".to_string();
    harness.app.metadata_dirty = true;
    harness.app.version_ui.history_reset_confirm_target = Some(42);
    harness.app.reset_selected_history_version();

    assert!(harness.app.history_reset_flush_active());
    assert!(matches!(
        harness.cmd_rx.try_recv(),
        Err(TryRecvError::Empty)
    ));

    let mut stale_ack = Paste::new("content".to_string(), "First name".to_string());
    stale_ack.id = "alpha".to_string();
    harness
        .app
        .apply_event(CoreEvent::PasteMetaSaved { paste: stale_ack });

    assert!(harness.app.history_reset_flush_active());
    assert!(harness.app.metadata_dirty);
    assert!(harness.app.metadata_save_in_flight);
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::UpdatePasteMeta { id, name, .. } => {
            assert_eq!(id, "alpha");
            assert_eq!(name.as_deref(), Some("Second name"));
        }
        other => panic!("expected redispatched metadata save, got {:?}", other),
    }

    let mut fresh_ack = Paste::new("content".to_string(), "Second name".to_string());
    fresh_ack.id = "alpha".to_string();
    harness
        .app
        .apply_event(CoreEvent::PasteMetaSaved { paste: fresh_ack });

    assert!(!harness.app.history_reset_flush_active());
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::ResetPasteHardToVersion {
            id,
            version_id_ms,
            preserve_current_head,
        } => {
            assert_eq!(id, "alpha");
            assert_eq!(version_id_ms, 42);
            assert!(preserve_current_head);
        }
        other => panic!("expected reset after fresh metadata ack, got {:?}", other),
    }
}
