//! Regression coverage for cached History/Diff modal preview state.

use super::*;

#[test]
fn active_snapshot_cache_reuses_snapshot_until_editor_revision_changes() {
    let mut harness = make_app();

    assert!(harness.app.sync_active_snapshot_cache());
    assert_eq!(harness.app.version_ui.active_snapshot_cache_text, "content");
    assert!(
        !harness.app.sync_active_snapshot_cache(),
        "repeated cache sync should be a no-op when active buffer identity is unchanged"
    );

    harness
        .app
        .selected_content
        .insert_text("!", harness.app.selected_content.len());

    assert!(harness.app.sync_active_snapshot_cache());
    assert_eq!(
        harness.app.version_ui.active_snapshot_cache_text,
        "content!"
    );
}

#[test]
fn history_preview_cache_reuses_loaded_snapshot_until_selected_version_changes() {
    let mut harness = make_app();
    harness.app.version_ui.history_selected_index = 1;
    harness.app.version_ui.history_snapshot =
        Some(localpaste_core::models::paste::VersionSnapshot {
            paste_id: "alpha".to_string(),
            version_id_ms: 41,
            created_at: chrono::Utc::now(),
            content_hash: "hash-41".to_string(),
            len: 4,
            language: None,
            language_is_manual: false,
            content: "text".to_string(),
        });

    assert!(harness.app.sync_history_preview_cache());
    assert_eq!(harness.app.version_ui.history_preview_text, "text");
    assert!(
        !harness.app.sync_history_preview_cache(),
        "identical snapshot selections should reuse the cached preview body"
    );

    harness.app.version_ui.history_snapshot =
        Some(localpaste_core::models::paste::VersionSnapshot {
            paste_id: "alpha".to_string(),
            version_id_ms: 42,
            created_at: chrono::Utc::now(),
            content_hash: "hash-42".to_string(),
            len: 4,
            language: None,
            language_is_manual: false,
            content: "next".to_string(),
        });

    assert!(harness.app.sync_history_preview_cache());
    assert_eq!(harness.app.version_ui.history_preview_text, "next");
}

#[test]
fn diff_preview_cache_waits_for_target_and_reuses_preview_until_inputs_change() {
    let mut harness = make_app();

    assert!(
        !harness.app.sync_diff_preview_cache(),
        "diff cache should stay cold until a comparison target is loaded"
    );
    assert!(harness.app.version_ui.diff_preview.is_none());
    assert!(harness.app.version_ui.active_snapshot_cache_text.is_empty());

    let mut rhs = Paste::new("beta-content".to_string(), "Beta".to_string());
    rhs.id = "beta".to_string();
    harness.app.version_ui.diff_target_paste = Some(rhs);

    assert!(harness.app.sync_diff_preview_cache());
    assert!(matches!(
        harness.app.version_ui.diff_preview.as_ref(),
        Some(crate::app::ui::diff_modal::InlineDiffPreview::Lines(lines)) if !lines.is_empty()
    ));
    assert!(
        !harness.app.sync_diff_preview_cache(),
        "same current snapshot + target paste should reuse cached diff lines"
    );

    harness
        .app
        .selected_content
        .insert_text("!", harness.app.selected_content.len());

    assert!(harness.app.sync_diff_preview_cache());
}
