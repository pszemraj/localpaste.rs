//! Database tests for version retention configuration boundaries.

use super::*;
use crate::env::{env_lock, EnvGuard};
use std::time::Duration;

#[test]
fn content_update_prunes_versions_to_single_retained_snapshot() {
    let _lock = env_lock().lock().expect("env lock");
    let (db, _temp) = with_db_init_test_lock(|| {
        let _interval_guard = EnvGuard::set("LOCALPASTE_VERSION_INTERVAL_SECS", "1");
        let _limit_guard = EnvGuard::set("LOCALPASTE_VERSION_RETENTION_LIMIT", "1");
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let db_path = temp_dir.path().join("db");
        let db = Database::new(db_path.to_str().expect("db path")).expect("db");
        (db, temp_dir)
    });

    let paste = Paste::new("v1".to_string(), "retention-single".to_string());
    let paste_id = paste.id.clone();
    db.pastes.create(&paste).expect("create");

    update_existing_paste(
        &db,
        &paste_id,
        update_request(Some("v2"), None, None, None),
        "update to v2",
    );
    std::thread::sleep(Duration::from_millis(1100));
    update_existing_paste(
        &db,
        &paste_id,
        update_request(Some("v3"), None, None, None),
        "update to v3",
    );

    let versions = db
        .pastes
        .list_versions(&paste_id, Some(10))
        .expect("list versions")
        .expect("paste exists");
    assert_eq!(versions.len(), 1);

    let retained = db
        .pastes
        .get_version(&paste_id, versions[0].version_id_ms)
        .expect("load retained version")
        .expect("retained version exists");
    assert_eq!(retained.content, "v2");
}

#[test]
fn protected_content_update_preserves_reset_target_past_retention_limit() {
    let _lock = env_lock().lock().expect("env lock");
    let (db, _temp) = with_db_init_test_lock(|| {
        let _interval_guard = EnvGuard::set("LOCALPASTE_VERSION_INTERVAL_SECS", "1");
        let _limit_guard = EnvGuard::set("LOCALPASTE_VERSION_RETENTION_LIMIT", "1");
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let db_path = temp_dir.path().join("db");
        let db = Database::new(db_path.to_str().expect("db path")).expect("db");
        (db, temp_dir)
    });

    let paste = Paste::new("v1".to_string(), "protected-reset-target".to_string());
    let paste_id = paste.id.clone();
    db.pastes.create(&paste).expect("create");
    update_existing_paste(
        &db,
        &paste_id,
        update_request(Some("v2"), None, None, None),
        "update to v2",
    );
    let reset_target = db
        .pastes
        .list_versions(&paste_id, Some(1))
        .expect("list versions")
        .expect("paste exists")[0]
        .version_id_ms;

    std::thread::sleep(Duration::from_millis(1100));
    db.pastes
        .update_preserving_version(
            &paste_id,
            update_request(Some("v3"), None, None, None),
            reset_target,
        )
        .expect("protected update")
        .expect("paste exists");

    let versions = db
        .pastes
        .list_versions(&paste_id, Some(10))
        .expect("list versions after protected update")
        .expect("paste exists");
    assert_eq!(
        versions.len(),
        2,
        "protected reset target should survive alongside the newest retained snapshot"
    );
    assert!(versions
        .iter()
        .any(|version| version.version_id_ms == reset_target));

    let reset = db
        .pastes
        .reset_hard_to_version(&paste_id, reset_target, usize::MAX)
        .expect("reset hard")
        .expect("paste exists");
    assert_eq!(reset.content, "v1");
}

#[test]
fn save_and_reset_preserves_just_saved_dirty_head_as_recoverable_version() {
    let _lock = env_lock().lock().expect("env lock");
    let (db, _temp) = with_db_init_test_lock(|| {
        let _interval_guard = EnvGuard::set("LOCALPASTE_VERSION_INTERVAL_SECS", "3600");
        let _limit_guard = EnvGuard::set("LOCALPASTE_VERSION_RETENTION_LIMIT", "10");
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let db_path = temp_dir.path().join("db");
        let db = Database::new(db_path.to_str().expect("db path")).expect("db");
        (db, temp_dir)
    });

    let paste = Paste::new("v1".to_string(), "dirty-save-reset".to_string());
    let paste_id = paste.id.clone();
    db.pastes.create(&paste).expect("create");
    update_existing_paste(
        &db,
        &paste_id,
        update_request(Some("v2"), None, None, None),
        "update to v2",
    );
    let reset_target = db
        .pastes
        .list_versions(&paste_id, Some(1))
        .expect("list versions")
        .expect("paste exists")[0]
        .version_id_ms;

    db.pastes
        .update_preserving_version(
            &paste_id,
            update_request(Some("dirty v3"), None, None, None),
            reset_target,
        )
        .expect("save dirty content")
        .expect("paste exists");

    let reset = db
        .pastes
        .reset_hard_to_version_preserving_current_head(&paste_id, reset_target, usize::MAX)
        .expect("reset hard preserving current head")
        .expect("paste exists");
    assert_eq!(reset.content, "v1");

    let versions_after_reset = db
        .pastes
        .list_versions(&paste_id, Some(10))
        .expect("list versions after reset")
        .expect("paste exists");
    assert_eq!(
        versions_after_reset.len(),
        1,
        "dirty head should remain as the only recoverable post-reset snapshot"
    );

    let preserved_dirty_head = db
        .pastes
        .get_version(&paste_id, versions_after_reset[0].version_id_ms)
        .expect("load preserved dirty head")
        .expect("preserved dirty head exists");
    assert_eq!(preserved_dirty_head.content, "dirty v3");
}

#[test]
fn reset_preserving_current_head_keeps_clean_head_recoverable() {
    let _lock = env_lock().lock().expect("env lock");
    let (db, _temp) = with_db_init_test_lock(|| {
        let _interval_guard = EnvGuard::set("LOCALPASTE_VERSION_INTERVAL_SECS", "3600");
        let _limit_guard = EnvGuard::set("LOCALPASTE_VERSION_RETENTION_LIMIT", "10");
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let db_path = temp_dir.path().join("db");
        let db = Database::new(db_path.to_str().expect("db path")).expect("db");
        (db, temp_dir)
    });

    let paste = Paste::new("v1".to_string(), "clean-reset".to_string());
    let paste_id = paste.id.clone();
    db.pastes.create(&paste).expect("create");
    update_existing_paste(
        &db,
        &paste_id,
        update_request(Some("v2"), None, None, None),
        "update to v2",
    );
    let reset_target = db
        .pastes
        .list_versions(&paste_id, Some(1))
        .expect("list versions")
        .expect("paste exists")[0]
        .version_id_ms;

    let reset = db
        .pastes
        .reset_hard_to_version_preserving_current_head(&paste_id, reset_target, usize::MAX)
        .expect("reset hard preserving current head")
        .expect("paste exists");
    assert_eq!(reset.content, "v1");

    let versions_after_reset = db
        .pastes
        .list_versions(&paste_id, Some(10))
        .expect("list versions after reset")
        .expect("paste exists");
    assert_eq!(versions_after_reset.len(), 1);

    let preserved_head = db
        .pastes
        .get_version(&paste_id, versions_after_reset[0].version_id_ms)
        .expect("load preserved head")
        .expect("preserved head exists");
    assert_eq!(preserved_head.content, "v2");
}

#[test]
fn primary_version_interval_env_wins_over_legacy_alias_at_db_init() {
    let _lock = env_lock().lock().expect("env lock");
    let (db, _temp) = with_db_init_test_lock(|| {
        let _primary = EnvGuard::set("LOCALPASTE_VERSION_INTERVAL_SECS", "3600");
        let _legacy = EnvGuard::set("LOCALPASTE_PASTE_VERSION_INTERVAL_SECS", "1");
        let _limit_guard = EnvGuard::set("LOCALPASTE_VERSION_RETENTION_LIMIT", "10");
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let db_path = temp_dir.path().join("db");
        let db = Database::new(db_path.to_str().expect("db path")).expect("db");
        (db, temp_dir)
    });

    let paste = Paste::new("v1".to_string(), "interval-precedence".to_string());
    let paste_id = paste.id.clone();
    db.pastes.create(&paste).expect("create");

    update_existing_paste(
        &db,
        &paste_id,
        update_request(Some("v2"), None, None, None),
        "update to v2",
    );
    std::thread::sleep(Duration::from_millis(1100));
    update_existing_paste(
        &db,
        &paste_id,
        update_request(Some("v3"), None, None, None),
        "update to v3",
    );

    let versions = db
        .pastes
        .list_versions(&paste_id, Some(10))
        .expect("list versions")
        .expect("paste exists");
    assert_eq!(
        versions.len(),
        1,
        "primary interval should prevent the second snapshot even when legacy alias is shorter"
    );
}
