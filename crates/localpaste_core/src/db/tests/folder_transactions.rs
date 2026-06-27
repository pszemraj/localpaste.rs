//! Folder transaction behavior tests.

use super::*;
use crate::db::tables::{PASTE_VERSIONS_CONTENT, PASTE_VERSIONS_META};
use crate::env::{env_lock, EnvGuard};
use redb::ReadableDatabase;
use std::time::Duration;

struct FolderMoveFixture {
    _temp: tempfile::TempDir,
    db: Database,
    old_folder_id: String,
    new_folder_id: String,
    paste_id: String,
}

fn setup_folder_move_fixture() -> FolderMoveFixture {
    let (db, temp) = setup_test_db();

    let old_folder = Folder::new("old-folder".to_string());
    let old_folder_id = old_folder.id.clone();
    db.folders.create(&old_folder).expect("create old");

    let new_folder = Folder::new("new-folder".to_string());
    let new_folder_id = new_folder.id.clone();
    db.folders.create(&new_folder).expect("create new");

    let mut paste = Paste::new("content".to_string(), "name".to_string());
    paste.folder_id = Some(old_folder_id.clone());
    let paste_id = paste.id.clone();
    TransactionOps::create_paste_with_folder(&db, &paste, &old_folder_id).expect("create");

    FolderMoveFixture {
        _temp: temp,
        db,
        old_folder_id,
        new_folder_id,
        paste_id,
    }
}

#[test]
fn create_with_folder_rejects_when_folder_is_marked_for_delete() {
    let (db, _temp) = setup_test_db();

    let folder = Folder::new("target-folder".to_string());
    let folder_id = folder.id.clone();
    db.folders.create(&folder).expect("create folder");
    db.folders
        .mark_deleting(std::slice::from_ref(&folder_id))
        .expect("mark deleting");

    let mut paste = Paste::new("content".to_string(), "name".to_string());
    paste.folder_id = Some(folder_id.clone());
    let paste_id = paste.id.clone();

    let result = TransactionOps::create_paste_with_folder(&db, &paste, &folder_id);
    assert!(matches!(result, Err(AppError::BadRequest(_))));
    assert!(db.pastes.get(&paste_id).expect("lookup").is_none());
}

#[test]
fn create_with_folder_rejects_conflicting_paste_folder_id() {
    let (db, _temp) = setup_test_db();

    let folder = Folder::new("target-folder".to_string());
    let folder_id = folder.id.clone();
    db.folders.create(&folder).expect("create folder");

    let other_folder = Folder::new("other-folder".to_string());
    let other_folder_id = other_folder.id.clone();
    db.folders
        .create(&other_folder)
        .expect("create other folder");

    let mut paste = Paste::new("content".to_string(), "name".to_string());
    paste.folder_id = Some(other_folder_id.clone());

    let result = TransactionOps::create_paste_with_folder(&db, &paste, &folder_id);
    assert!(
        matches!(result, Err(AppError::BadRequest(ref message)) if message.contains("does not match")),
        "conflicting create folder assignment should be rejected: {:?}",
        result
    );
    assert!(db.pastes.get(&paste.id).expect("lookup").is_none());
}

#[test]
fn create_with_folder_duplicate_id_keeps_counts_consistent() {
    let fixture = setup_folder_move_fixture();
    let db = &fixture.db;

    let mut duplicate = Paste::new("conflicting".to_string(), "name".to_string());
    duplicate.id = fixture.paste_id.clone();
    duplicate.folder_id = Some(fixture.new_folder_id.clone());

    let result = TransactionOps::create_paste_with_folder(db, &duplicate, &fixture.new_folder_id);
    assert!(
        matches!(result, Err(AppError::StorageMessage(ref message)) if message.contains("already exists")),
        "duplicate id create should fail without count drift: {:?}",
        result
    );

    let old_after = db
        .folders
        .get(&fixture.old_folder_id)
        .expect("old")
        .expect("row");
    let new_after = db
        .folders
        .get(&fixture.new_folder_id)
        .expect("new")
        .expect("row");
    assert_eq!(old_after.paste_count, 1);
    assert_eq!(new_after.paste_count, 0);
}

#[test]
fn move_between_folders_updates_counts_and_assignment() {
    let fixture = setup_folder_move_fixture();
    let db = &fixture.db;

    let update = UpdatePasteRequest {
        content: None,
        name: None,
        language: None,
        language_is_manual: None,
        folder_id: Some(fixture.new_folder_id.clone()),
        tags: None,
    };

    let moved = TransactionOps::move_paste_between_folders(
        db,
        &fixture.paste_id,
        Some(fixture.new_folder_id.as_str()),
        update,
    )
    .expect("move")
    .expect("paste exists");

    assert_eq!(
        moved.folder_id.as_deref(),
        Some(fixture.new_folder_id.as_str())
    );
    let old_after = db
        .folders
        .get(&fixture.old_folder_id)
        .expect("old")
        .expect("row");
    let new_after = db
        .folders
        .get(&fixture.new_folder_id)
        .expect("new")
        .expect("row");
    assert_eq!(old_after.paste_count, 0);
    assert_eq!(new_after.paste_count, 1);
}

#[test]
fn move_within_same_folder_updates_paste_without_count_drift() {
    let fixture = setup_folder_move_fixture();
    let db = &fixture.db;

    let update = UpdatePasteRequest {
        content: Some("updated content".to_string()),
        name: Some("renamed".to_string()),
        language: None,
        language_is_manual: None,
        folder_id: Some(fixture.old_folder_id.clone()),
        tags: None,
    };

    let moved = TransactionOps::move_paste_between_folders(
        db,
        &fixture.paste_id,
        Some(fixture.old_folder_id.as_str()),
        update,
    )
    .expect("move")
    .expect("paste exists");

    assert_eq!(
        moved.folder_id.as_deref(),
        Some(fixture.old_folder_id.as_str())
    );
    assert_eq!(moved.name, "renamed");
    assert_eq!(moved.content, "updated content");

    let old_after = db
        .folders
        .get(&fixture.old_folder_id)
        .expect("old")
        .expect("row");
    let new_after = db
        .folders
        .get(&fixture.new_folder_id)
        .expect("new")
        .expect("row");
    assert_eq!(old_after.paste_count, 1);
    assert_eq!(new_after.paste_count, 0);
}

#[test]
fn content_change_during_folder_moves_archives_middle_version_after_wait() {
    let _lock = env_lock().lock().expect("env lock");
    let fixture = with_db_init_test_lock(|| {
        let _interval_guard = EnvGuard::set("LOCALPASTE_VERSION_INTERVAL_SECS", "1");
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let db_path = temp_dir.path().join("db");
        let db = Database::new(db_path.to_str().expect("db path")).expect("db");

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

        FolderMoveFixture {
            _temp: temp_dir,
            db,
            old_folder_id,
            new_folder_id,
            paste_id,
        }
    });
    let db = &fixture.db;

    let first_move = UpdatePasteRequest {
        content: Some("v2".to_string()),
        name: None,
        language: None,
        language_is_manual: None,
        folder_id: Some(fixture.new_folder_id.clone()),
        tags: None,
    };
    TransactionOps::move_paste_between_folders(
        db,
        &fixture.paste_id,
        Some(fixture.new_folder_id.as_str()),
        first_move,
    )
    .expect("first move")
    .expect("paste exists");

    std::thread::sleep(Duration::from_millis(1100));

    let second_move = UpdatePasteRequest {
        content: Some("v3".to_string()),
        name: None,
        language: None,
        language_is_manual: None,
        folder_id: Some(fixture.old_folder_id.clone()),
        tags: None,
    };
    TransactionOps::move_paste_between_folders(
        db,
        &fixture.paste_id,
        Some(fixture.old_folder_id.as_str()),
        second_move,
    )
    .expect("second move")
    .expect("paste exists");

    let versions = db
        .pastes
        .list_versions(&fixture.paste_id, None)
        .expect("list versions")
        .expect("paste exists");
    assert_eq!(
        versions.len(),
        2,
        "later content-changing folder move should archive the outgoing middle version"
    );

    let newest = db
        .pastes
        .get_version(&fixture.paste_id, versions[0].version_id_ms)
        .expect("load newest version")
        .expect("newest version exists");
    let oldest = db
        .pastes
        .get_version(&fixture.paste_id, versions[1].version_id_ms)
        .expect("load oldest version")
        .expect("oldest version exists");
    assert_eq!(newest.content, "v2");
    assert_eq!(oldest.content, "v1");
}

#[test]
fn content_change_during_folder_moves_prunes_versions_past_retention_limit() {
    let _lock = env_lock().lock().expect("env lock");
    let fixture = with_db_init_test_lock(|| {
        let _interval_guard = EnvGuard::set("LOCALPASTE_VERSION_INTERVAL_SECS", "1");
        let _limit_guard = EnvGuard::set("LOCALPASTE_VERSION_RETENTION_LIMIT", "2");
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let db_path = temp_dir.path().join("db");
        let db = Database::new(db_path.to_str().expect("db path")).expect("db");

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

        FolderMoveFixture {
            _temp: temp_dir,
            db,
            old_folder_id,
            new_folder_id,
            paste_id,
        }
    });
    let db = &fixture.db;

    let first_move = UpdatePasteRequest {
        content: Some("v2".to_string()),
        name: None,
        language: None,
        language_is_manual: None,
        folder_id: Some(fixture.new_folder_id.clone()),
        tags: None,
    };
    TransactionOps::move_paste_between_folders(
        db,
        &fixture.paste_id,
        Some(fixture.new_folder_id.as_str()),
        first_move,
    )
    .expect("first move")
    .expect("paste exists");
    let pruned_version_id = db
        .pastes
        .list_versions(&fixture.paste_id, Some(1))
        .expect("list versions after first move")
        .expect("paste exists")[0]
        .version_id_ms;

    std::thread::sleep(Duration::from_millis(1100));
    let second_move = UpdatePasteRequest {
        content: Some("v3".to_string()),
        name: None,
        language: None,
        language_is_manual: None,
        folder_id: Some(fixture.old_folder_id.clone()),
        tags: None,
    };
    TransactionOps::move_paste_between_folders(
        db,
        &fixture.paste_id,
        Some(fixture.old_folder_id.as_str()),
        second_move,
    )
    .expect("second move")
    .expect("paste exists");

    std::thread::sleep(Duration::from_millis(1100));
    let third_move = UpdatePasteRequest {
        content: Some("v4".to_string()),
        name: None,
        language: None,
        language_is_manual: None,
        folder_id: Some(fixture.new_folder_id.clone()),
        tags: None,
    };
    TransactionOps::move_paste_between_folders(
        db,
        &fixture.paste_id,
        Some(fixture.new_folder_id.as_str()),
        third_move,
    )
    .expect("third move")
    .expect("paste exists");

    let versions = db
        .pastes
        .list_versions(&fixture.paste_id, Some(10))
        .expect("list versions")
        .expect("paste exists");
    assert_eq!(versions.len(), 2);

    let newest = db
        .pastes
        .get_version(&fixture.paste_id, versions[0].version_id_ms)
        .expect("load newest version")
        .expect("newest version exists");
    let older_retained = db
        .pastes
        .get_version(&fixture.paste_id, versions[1].version_id_ms)
        .expect("load older retained version")
        .expect("older retained version exists");
    assert_eq!(newest.content, "v3");
    assert_eq!(older_retained.content, "v2");
    assert!(
        db.pastes
            .get_version(&fixture.paste_id, pruned_version_id)
            .expect("load pruned version")
            .is_none(),
        "pruned folder-move version should no longer be addressable"
    );
}

#[test]
fn move_missing_paste_returns_none_without_count_drift() {
    let (db, _temp) = setup_test_db();

    let old_folder = Folder::new("old-folder".to_string());
    let old_folder_id = old_folder.id.clone();
    db.folders.create(&old_folder).expect("create old");

    let new_folder = Folder::new("new-folder".to_string());
    let new_folder_id = new_folder.id.clone();
    db.folders.create(&new_folder).expect("create new");

    let update = UpdatePasteRequest {
        content: None,
        name: None,
        language: None,
        language_is_manual: None,
        folder_id: Some(new_folder_id.clone()),
        tags: None,
    };

    let result = TransactionOps::move_paste_between_folders(
        &db,
        "missing-paste-id",
        Some(new_folder_id.as_str()),
        update,
    )
    .expect("move");
    assert!(result.is_none());

    let old_after = db.folders.get(&old_folder_id).expect("old").expect("row");
    let new_after = db.folders.get(&new_folder_id).expect("new").expect("row");
    assert_eq!(old_after.paste_count, 0);
    assert_eq!(new_after.paste_count, 0);
}

#[test]
fn move_between_folders_rejects_conflicting_update_request_folder_id() {
    let fixture = setup_folder_move_fixture();
    let db = &fixture.db;

    let update = UpdatePasteRequest {
        content: None,
        name: None,
        language: None,
        language_is_manual: None,
        folder_id: Some(fixture.old_folder_id.clone()),
        tags: None,
    };

    let result = TransactionOps::move_paste_between_folders(
        db,
        &fixture.paste_id,
        Some(fixture.new_folder_id.as_str()),
        update,
    );
    assert!(
        matches!(result, Err(AppError::BadRequest(ref message)) if message.contains("does not match")),
        "conflicting move folder assignment should be rejected: {:?}",
        result
    );

    let current = db
        .pastes
        .get(&fixture.paste_id)
        .expect("lookup")
        .expect("paste exists");
    assert_eq!(
        current.folder_id.as_deref(),
        Some(fixture.old_folder_id.as_str())
    );
}

#[test]
fn delete_uses_folder_from_deleted_record_not_stale_context() {
    let fixture = setup_folder_move_fixture();
    let db = &fixture.db;

    let move_req = UpdatePasteRequest {
        content: None,
        name: None,
        language: None,
        language_is_manual: None,
        folder_id: Some(fixture.new_folder_id.clone()),
        tags: None,
    };
    TransactionOps::move_paste_between_folders(
        db,
        &fixture.paste_id,
        Some(fixture.new_folder_id.as_str()),
        move_req,
    )
    .expect("move")
    .expect("paste exists");

    let deleted = TransactionOps::delete_paste_with_folder(db, &fixture.paste_id).expect("delete");
    assert!(deleted);

    let old_after = db
        .folders
        .get(&fixture.old_folder_id)
        .expect("old")
        .expect("row");
    let new_after = db
        .folders
        .get(&fixture.new_folder_id)
        .expect("new")
        .expect("row");
    assert_eq!(old_after.paste_count, 0);
    assert_eq!(new_after.paste_count, 0);
}

#[test]
fn delete_with_folder_does_not_require_version_content_payloads() {
    let fixture = setup_folder_move_fixture();
    let db = &fixture.db;

    let update = UpdatePasteRequest {
        content: Some("updated".to_string()),
        name: None,
        language: None,
        language_is_manual: None,
        folder_id: Some(fixture.old_folder_id.clone()),
        tags: None,
    };
    TransactionOps::move_paste_between_folders(
        db,
        &fixture.paste_id,
        Some(fixture.old_folder_id.as_str()),
        update,
    )
    .expect("update")
    .expect("paste exists");

    let version_id = db
        .pastes
        .list_versions(&fixture.paste_id, Some(1))
        .expect("list versions")
        .expect("paste exists")[0]
        .version_id_ms;
    let write_txn = db.db.begin_write().expect("begin write");
    {
        let mut versions_content = write_txn
            .open_table(PASTE_VERSIONS_CONTENT)
            .expect("open versions content");
        let removed = versions_content
            .remove((fixture.paste_id.as_str(), version_id))
            .expect("remove version content");
        assert!(removed.is_some());
    }
    write_txn.commit().expect("commit missing content row");

    let deleted = TransactionOps::delete_paste_with_folder(db, &fixture.paste_id).expect("delete");
    assert!(deleted);
    assert!(db
        .pastes
        .get(&fixture.paste_id)
        .expect("lookup after delete")
        .is_none());

    let folder_after = db
        .folders
        .get(&fixture.old_folder_id)
        .expect("folder")
        .expect("row");
    assert_eq!(folder_after.paste_count, 0);

    let read_txn = db.db.begin_read().expect("begin read");
    let versions_meta = read_txn
        .open_table(PASTE_VERSIONS_META)
        .expect("open versions meta");
    assert!(versions_meta
        .get(fixture.paste_id.as_str())
        .expect("get versions meta")
        .is_none());
}

#[test]
fn delete_bundle_restore_preserves_folder_and_versions() {
    let fixture = setup_folder_move_fixture();
    let db = &fixture.db;

    let update = UpdatePasteRequest {
        content: Some("updated".to_string()),
        name: None,
        language: None,
        language_is_manual: None,
        folder_id: Some(fixture.old_folder_id.clone()),
        tags: None,
    };
    TransactionOps::move_paste_between_folders(
        db,
        &fixture.paste_id,
        Some(fixture.old_folder_id.as_str()),
        update,
    )
    .expect("update")
    .expect("paste exists");

    let bundle = TransactionOps::delete_paste_with_folder_bundle(db, &fixture.paste_id)
        .expect("delete bundle")
        .expect("paste deleted");
    assert_eq!(bundle.versions.len(), 1);
    assert!(
        db.pastes
            .get(&fixture.paste_id)
            .expect("lookup after delete")
            .is_none(),
        "delete should remove live row before undo"
    );

    let restored =
        TransactionOps::restore_deleted_paste(db, bundle).expect("restore deleted paste");
    assert_eq!(restored.id, fixture.paste_id);
    assert_eq!(restored.content, "updated");
    assert_eq!(
        restored.folder_id.as_deref(),
        Some(fixture.old_folder_id.as_str())
    );
    let folder_after = db
        .folders
        .get(&fixture.old_folder_id)
        .expect("folder")
        .expect("row");
    assert_eq!(folder_after.paste_count, 1);

    let versions = db
        .pastes
        .list_versions(&fixture.paste_id, None)
        .expect("list versions")
        .expect("versions exist");
    assert_eq!(versions.len(), 1);
    let snapshot = db
        .pastes
        .get_version(&fixture.paste_id, versions[0].version_id_ms)
        .expect("get restored version")
        .expect("version exists");
    assert_eq!(snapshot.content, "content");
}

#[test]
fn staged_delete_undo_preserves_versions_without_payload_cap() {
    let fixture = setup_folder_move_fixture();
    let db = &fixture.db;

    let update = UpdatePasteRequest {
        content: Some("updated".to_string()),
        name: None,
        language: None,
        language_is_manual: None,
        folder_id: Some(fixture.old_folder_id.clone()),
        tags: None,
    };
    TransactionOps::move_paste_between_folders(
        db,
        &fixture.paste_id,
        Some(fixture.old_folder_id.as_str()),
        update,
    )
    .expect("update")
    .expect("paste exists");

    let token = "undo-token-alpha";
    assert!(
        TransactionOps::delete_paste_with_folder_staged_undo(
            db,
            &fixture.paste_id,
            token,
            i64::MAX
        )
        .expect("delete with staged undo"),
        "paste should be staged for undo"
    );
    assert!(db
        .pastes
        .get(&fixture.paste_id)
        .expect("lookup after delete")
        .is_none());

    let read_txn = db.db.begin_read().expect("begin read");
    let versions_meta = read_txn
        .open_table(PASTE_VERSIONS_META)
        .expect("open versions meta");
    assert!(versions_meta
        .get(fixture.paste_id.as_str())
        .expect("get versions meta")
        .is_none());
    drop(versions_meta);
    drop(read_txn);

    let restored = TransactionOps::restore_deleted_paste_by_token(db, token)
        .expect("restore staged paste")
        .expect("staged token exists");
    assert_eq!(restored.id, fixture.paste_id);
    assert_eq!(restored.content, "updated");
    assert_staged_undo_token(db, token, false);

    let versions = db
        .pastes
        .list_versions(&fixture.paste_id, None)
        .expect("list versions")
        .expect("versions exist");
    assert_eq!(versions.len(), 1);
    let snapshot = db
        .pastes
        .get_version(&fixture.paste_id, versions[0].version_id_ms)
        .expect("get restored version")
        .expect("version exists");
    assert_eq!(snapshot.content, "content");
}

#[test]
fn prune_expired_staged_delete_undo_removes_all_staged_rows() {
    let (db, _temp) = setup_test_db();
    let expired_paste_id = seed_versioned_paste(&db, "expired undo");
    let live_paste_id = seed_versioned_paste(&db, "live undo");

    assert!(TransactionOps::delete_paste_with_folder_staged_undo(
        &db,
        &expired_paste_id,
        "expired-token",
        10
    )
    .expect("stage expired paste"));
    assert!(TransactionOps::delete_paste_with_folder_staged_undo(
        &db,
        &live_paste_id,
        "live-token",
        i64::MAX
    )
    .expect("stage live paste"));

    let pruned = TransactionOps::prune_expired_deleted_paste_undo(&db, 10)
        .expect("prune expired staged rows");
    assert_eq!(pruned, vec!["expired-token".to_string()]);
    assert_staged_undo_token(&db, "expired-token", false);
    assert_staged_undo_token(&db, "live-token", true);
}

#[test]
fn staged_delete_undo_fails_atomically_when_version_content_is_missing() {
    let fixture = setup_folder_move_fixture();
    let db = &fixture.db;

    let update = UpdatePasteRequest {
        content: Some("updated".to_string()),
        name: None,
        language: None,
        language_is_manual: None,
        folder_id: Some(fixture.old_folder_id.clone()),
        tags: None,
    };
    TransactionOps::move_paste_between_folders(
        db,
        &fixture.paste_id,
        Some(fixture.old_folder_id.as_str()),
        update,
    )
    .expect("update")
    .expect("paste exists");

    let version_id = db
        .pastes
        .list_versions(&fixture.paste_id, Some(1))
        .expect("list versions")
        .expect("paste exists")[0]
        .version_id_ms;
    let write_txn = db.db.begin_write().expect("begin write");
    {
        let mut versions_content = write_txn
            .open_table(PASTE_VERSIONS_CONTENT)
            .expect("open versions content");
        let removed = versions_content
            .remove((fixture.paste_id.as_str(), version_id))
            .expect("remove version content");
        assert!(removed.is_some());
    }
    write_txn.commit().expect("commit missing content row");

    let err = TransactionOps::delete_paste_with_folder_staged_undo(
        db,
        &fixture.paste_id,
        "undo-token-missing-content",
        i64::MAX,
    )
    .expect_err("missing historical content should abort staged delete");
    assert!(
        err.to_string().contains("Missing version content"),
        "unexpected staged delete error: {}",
        err
    );
    assert!(db
        .pastes
        .get(&fixture.paste_id)
        .expect("lookup after delete")
        .is_some());

    let folder_after = db
        .folders
        .get(&fixture.old_folder_id)
        .expect("folder")
        .expect("row");
    assert_eq!(folder_after.paste_count, 1);

    let read_txn = db.db.begin_read().expect("begin read");
    let versions_meta = read_txn
        .open_table(PASTE_VERSIONS_META)
        .expect("open versions meta");
    assert!(versions_meta
        .get(fixture.paste_id.as_str())
        .expect("get versions meta")
        .is_some());
}

#[test]
fn restore_deleted_paste_clears_missing_original_folder() {
    let fixture = setup_folder_move_fixture();
    let db = &fixture.db;
    let bundle = TransactionOps::delete_paste_with_folder_bundle(db, &fixture.paste_id)
        .expect("delete bundle")
        .expect("paste deleted");
    assert!(db
        .folders
        .delete(&fixture.old_folder_id)
        .expect("delete source folder"));

    let restored =
        TransactionOps::restore_deleted_paste(db, bundle).expect("restore deleted paste");
    assert_eq!(restored.id, fixture.paste_id);
    assert_eq!(restored.folder_id, None);
    assert_eq!(
        db.pastes
            .get(&fixture.paste_id)
            .expect("restored lookup")
            .expect("restored row")
            .folder_id,
        None
    );
}

#[test]
fn restore_deleted_paste_clears_original_folder_marked_for_delete() {
    let fixture = setup_folder_move_fixture();
    let db = &fixture.db;
    let bundle = TransactionOps::delete_paste_with_folder_bundle(db, &fixture.paste_id)
        .expect("delete bundle")
        .expect("paste deleted");
    db.folders
        .mark_deleting(std::slice::from_ref(&fixture.old_folder_id))
        .expect("mark folder deleting");

    let restored =
        TransactionOps::restore_deleted_paste(db, bundle).expect("restore deleted paste");
    assert_eq!(restored.id, fixture.paste_id);
    assert_eq!(restored.folder_id, None);
    let folder_after = db
        .folders
        .get(&fixture.old_folder_id)
        .expect("folder lookup")
        .expect("folder still exists while deleting");
    assert_eq!(
        folder_after.paste_count, 0,
        "restore should not increment a folder already in the delete marker table"
    );
}

#[test]
fn direct_folder_affecting_paste_ops_are_rejected() {
    let (db, _temp) = setup_test_db();

    let folder = Folder::new("folder".to_string());
    let folder_id = folder.id.clone();
    db.folders.create(&folder).expect("create folder");

    let mut direct_create = Paste::new("content".to_string(), "direct-create".to_string());
    direct_create.folder_id = Some(folder_id.clone());
    let create_err = db
        .pastes
        .create(&direct_create)
        .expect_err("direct folder create should be rejected");
    assert!(matches!(create_err, AppError::BadRequest(_)));
    assert!(
        db.pastes.get(&direct_create.id).expect("lookup").is_none(),
        "rejected direct create must not persist rows"
    );

    let mut transactional = Paste::new("content".to_string(), "managed".to_string());
    transactional.folder_id = Some(folder_id.clone());
    let paste_id = transactional.id.clone();
    TransactionOps::create_paste_with_folder(&db, &transactional, &folder_id).expect("create");

    let update_err = db
        .pastes
        .update(
            &paste_id,
            UpdatePasteRequest {
                content: None,
                name: None,
                language: None,
                language_is_manual: None,
                folder_id: Some(String::new()),
                tags: None,
            },
        )
        .expect_err("direct folder update should be rejected");
    assert!(matches!(update_err, AppError::BadRequest(_)));

    let delete_err = db
        .pastes
        .delete(&paste_id)
        .expect_err("direct folder delete should be rejected");
    assert!(matches!(delete_err, AppError::BadRequest(_)));

    let current = db
        .pastes
        .get(&paste_id)
        .expect("lookup")
        .expect("paste should still exist");
    assert_eq!(current.folder_id.as_deref(), Some(folder_id.as_str()));
    let folder_after = db.folders.get(&folder_id).expect("folder").expect("exists");
    assert_eq!(folder_after.paste_count, 1);
}
