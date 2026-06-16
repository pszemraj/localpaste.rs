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
