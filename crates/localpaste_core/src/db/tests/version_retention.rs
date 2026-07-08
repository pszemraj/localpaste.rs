//! Database tests for version retention configuration boundaries.

use super::*;
use crate::env::{env_lock, EnvGuard};
use std::time::Duration;

#[test]
fn default_retention_limit_matches_documented_default() {
    assert_eq!(crate::constants::DEFAULT_PASTE_VERSION_RETENTION_LIMIT, 200);
}

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
fn lowered_retention_limit_prunes_existing_history_on_next_write_without_new_snapshot() {
    let _lock = env_lock().lock().expect("env lock");
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let db_path = temp_dir.path().join("db");
    let db_path_str = db_path.to_str().expect("db path");
    let paste_id = {
        let _interval_guard = EnvGuard::set("LOCALPASTE_VERSION_INTERVAL_SECS", "1");
        let _limit_guard = EnvGuard::set("LOCALPASTE_VERSION_RETENTION_LIMIT", "10");
        let db = with_db_init_test_lock(|| Database::new(db_path_str).expect("db"));
        let paste = Paste::new("v1".to_string(), "retention-lowered".to_string());
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
        std::thread::sleep(Duration::from_millis(1100));
        update_existing_paste(
            &db,
            &paste_id,
            update_request(Some("v4"), None, None, None),
            "update to v4",
        );
        let versions = db
            .pastes
            .list_versions(&paste_id, Some(10))
            .expect("list seeded versions")
            .expect("paste exists");
        assert_eq!(versions.len(), 3);
        paste_id
    };

    let _interval_guard = EnvGuard::set("LOCALPASTE_VERSION_INTERVAL_SECS", "3600");
    let _limit_guard = EnvGuard::set("LOCALPASTE_VERSION_RETENTION_LIMIT", "2");
    let db = with_db_init_test_lock(|| Database::new(db_path_str).expect("reopen db"));
    update_existing_paste(
        &db,
        &paste_id,
        update_request(Some("v5"), None, None, None),
        "update below interval after lowered limit",
    );

    let current = db
        .pastes
        .get(&paste_id)
        .expect("load current")
        .expect("paste exists");
    assert_eq!(current.content, "v5");
    let versions = db
        .pastes
        .list_versions(&paste_id, Some(10))
        .expect("list pruned versions")
        .expect("paste exists");
    assert_eq!(
        versions.len(),
        2,
        "lowered retention should prune even when no new snapshot is recorded"
    );
    let newest = db
        .pastes
        .get_version(&paste_id, versions[0].version_id_ms)
        .expect("load newest retained")
        .expect("newest retained exists");
    assert_eq!(newest.content, "v3");
}

#[test]
fn lowered_retention_limit_prunes_folder_move_history_without_new_snapshot() {
    let _lock = env_lock().lock().expect("env lock");
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let db_path = temp_dir.path().join("db");
    let db_path_str = db_path.to_str().expect("db path");
    let (paste_id, old_folder_id, new_folder_id) = {
        let _interval_guard = EnvGuard::set("LOCALPASTE_VERSION_INTERVAL_SECS", "1");
        let _limit_guard = EnvGuard::set("LOCALPASTE_VERSION_RETENTION_LIMIT", "10");
        let db = with_db_init_test_lock(|| Database::new(db_path_str).expect("db"));

        let old_folder = Folder::new("old-folder".to_string());
        let old_folder_id = old_folder.id.clone();
        db.folders.create(&old_folder).expect("create old");

        let new_folder = Folder::new("new-folder".to_string());
        let new_folder_id = new_folder.id.clone();
        db.folders.create(&new_folder).expect("create new");

        let mut paste = Paste::new("v1".to_string(), "name".to_string());
        paste.folder_id = Some(old_folder_id.clone());
        let paste_id = paste.id.clone();
        TransactionOps::create_paste_with_folder(&db, &paste, &old_folder_id).expect("create");

        TransactionOps::move_paste_between_folders(
            &db,
            &paste_id,
            Some(new_folder_id.as_str()),
            UpdatePasteRequest {
                content: Some("v2".to_string()),
                name: None,
                language: None,
                language_is_manual: None,
                folder_id: Some(new_folder_id.clone()),
                tags: None,
            },
        )
        .expect("first move")
        .expect("paste exists");
        std::thread::sleep(Duration::from_millis(1100));
        TransactionOps::move_paste_between_folders(
            &db,
            &paste_id,
            Some(old_folder_id.as_str()),
            UpdatePasteRequest {
                content: Some("v3".to_string()),
                name: None,
                language: None,
                language_is_manual: None,
                folder_id: Some(old_folder_id.clone()),
                tags: None,
            },
        )
        .expect("second move")
        .expect("paste exists");
        std::thread::sleep(Duration::from_millis(1100));
        TransactionOps::move_paste_between_folders(
            &db,
            &paste_id,
            Some(new_folder_id.as_str()),
            UpdatePasteRequest {
                content: Some("v4".to_string()),
                name: None,
                language: None,
                language_is_manual: None,
                folder_id: Some(new_folder_id.clone()),
                tags: None,
            },
        )
        .expect("third move")
        .expect("paste exists");
        assert_eq!(
            db.pastes
                .list_versions(&paste_id, Some(10))
                .expect("list seeded versions")
                .expect("paste exists")
                .len(),
            3
        );
        (paste_id, old_folder_id, new_folder_id)
    };

    let _interval_guard = EnvGuard::set("LOCALPASTE_VERSION_INTERVAL_SECS", "3600");
    let _limit_guard = EnvGuard::set("LOCALPASTE_VERSION_RETENTION_LIMIT", "2");
    let db = with_db_init_test_lock(|| Database::new(db_path_str).expect("reopen db"));
    TransactionOps::move_paste_between_folders(
        &db,
        &paste_id,
        Some(old_folder_id.as_str()),
        UpdatePasteRequest {
            content: Some("v5".to_string()),
            name: None,
            language: None,
            language_is_manual: None,
            folder_id: Some(old_folder_id.clone()),
            tags: None,
        },
    )
    .expect("move below interval after lowered limit")
    .expect("paste exists");

    let versions = db
        .pastes
        .list_versions(&paste_id, Some(10))
        .expect("list pruned versions")
        .expect("paste exists");
    assert_eq!(
        versions.len(),
        2,
        "lowered retention should prune during folder moves even when no new snapshot is recorded"
    );
    let paste = db
        .pastes
        .get(&paste_id)
        .expect("load current")
        .expect("paste exists");
    assert_eq!(paste.content, "v5");
    assert_eq!(paste.folder_id.as_deref(), Some(old_folder_id.as_str()));
    assert_eq!(
        db.folders
            .get(&old_folder_id)
            .expect("old folder")
            .expect("old folder exists")
            .paste_count,
        1
    );
    assert_eq!(
        db.folders
            .get(&new_folder_id)
            .expect("new folder")
            .expect("new folder exists")
            .paste_count,
        0
    );
}

#[test]
fn retention_max_matches_serialized_metadata_model() {
    assert_eq!(crate::constants::MAX_PASTE_VERSION_RETENTION_LIMIT, 1_000);
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
fn hard_reset_prunes_surviving_older_versions_to_retention_limit() {
    let _lock = env_lock().lock().expect("env lock");
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let db_path = temp_dir.path().join("db");
    let db_path_str = db_path.to_str().expect("db path");
    let (paste_id, reset_target) = {
        let _interval_guard = EnvGuard::set("LOCALPASTE_VERSION_INTERVAL_SECS", "1");
        let _limit_guard = EnvGuard::set("LOCALPASTE_VERSION_RETENTION_LIMIT", "10");
        let db = with_db_init_test_lock(|| Database::new(db_path_str).expect("db"));

        let paste = Paste::new("v1".to_string(), "reset-retention".to_string());
        let paste_id = paste.id.clone();
        db.pastes.create(&paste).expect("create");
        for content in ["v2", "v3", "v4", "v5"] {
            update_existing_paste(
                &db,
                &paste_id,
                update_request(Some(content), None, None, None),
                "update seeded version",
            );
            std::thread::sleep(Duration::from_millis(1100));
        }

        let versions_before = db
            .pastes
            .list_versions(&paste_id, Some(10))
            .expect("list versions before reset")
            .expect("paste exists");
        assert_eq!(versions_before.len(), 4);
        let reset_target = versions_before[0].version_id_ms;
        (paste_id, reset_target)
    };

    let _interval_guard = EnvGuard::set("LOCALPASTE_VERSION_INTERVAL_SECS", "1");
    let _limit_guard = EnvGuard::set("LOCALPASTE_VERSION_RETENTION_LIMIT", "2");
    let db = with_db_init_test_lock(|| Database::new(db_path_str).expect("reopen db"));
    let reset = db
        .pastes
        .reset_hard_to_version(&paste_id, reset_target, usize::MAX)
        .expect("reset hard")
        .expect("paste exists");
    assert_eq!(reset.content, "v4");

    let versions_after = db
        .pastes
        .list_versions(&paste_id, Some(10))
        .expect("list versions after reset")
        .expect("paste exists");
    assert_eq!(
        versions_after.len(),
        2,
        "reset target becomes current head, and older survivors are pruned to the lowered cap"
    );
    let retained = db
        .pastes
        .get_version(&paste_id, versions_after[0].version_id_ms)
        .expect("load retained older version")
        .expect("retained older version exists");
    assert_eq!(retained.content, "v3");
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
fn reset_preserving_current_head_keeps_language_only_head_recoverable() {
    let _lock = env_lock().lock().expect("env lock");
    let (db, _temp) = with_db_init_test_lock(|| {
        let _interval_guard = EnvGuard::set("LOCALPASTE_VERSION_INTERVAL_SECS", "3600");
        let _limit_guard = EnvGuard::set("LOCALPASTE_VERSION_RETENTION_LIMIT", "10");
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let db_path = temp_dir.path().join("db");
        let db = Database::new(db_path.to_str().expect("db path")).expect("db");
        (db, temp_dir)
    });

    let paste = Paste::new_with_language(
        "same text".to_string(),
        "language-only-reset".to_string(),
        Some("python".to_string()),
        true,
    );
    let paste_id = paste.id.clone();
    db.pastes.create(&paste).expect("create");

    update_existing_paste(
        &db,
        &paste_id,
        update_request(Some("temporary text"), None, None, None),
        "create reset target snapshot",
    );
    let reset_target = db
        .pastes
        .list_versions(&paste_id, Some(1))
        .expect("list versions")
        .expect("paste exists")[0]
        .version_id_ms;

    update_existing_paste(
        &db,
        &paste_id,
        update_request(Some("same text"), None, Some("rust"), Some(true)),
        "cycle content back with different language state",
    );

    let reset = db
        .pastes
        .reset_hard_to_version_preserving_current_head(&paste_id, reset_target, usize::MAX)
        .expect("reset hard preserving current head")
        .expect("paste exists");
    assert_eq!(reset.content, "same text");
    assert_eq!(reset.language.as_deref(), Some("python"));
    assert!(reset.language_is_manual);

    let versions_after_reset = db
        .pastes
        .list_versions(&paste_id, Some(10))
        .expect("list versions after reset")
        .expect("paste exists");
    assert_eq!(
        versions_after_reset.len(),
        1,
        "language-only outgoing head should remain recoverable after reset"
    );

    let preserved_head = db
        .pastes
        .get_version(&paste_id, versions_after_reset[0].version_id_ms)
        .expect("load preserved language-only head")
        .expect("preserved language-only head exists");
    assert_eq!(preserved_head.content, "same text");
    assert_eq!(preserved_head.language.as_deref(), Some("rust"));
    assert!(preserved_head.language_is_manual);
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
