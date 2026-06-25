//! Startup behavior and invariant repair tests.

use super::*;
use crate::db::paste::META_SCHEMA_VERSION_KEY;
use crate::db::tables::{PASTES, PASTES_META_STATE, REDB_FILE_NAME};
use redb::ReadableDatabase;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn setup_temp_db_path(name: &str) -> (TempDir, String) {
    let temp_dir = TempDir::new().expect("temp dir");
    let db_path = temp_dir.path().join(name);
    let db_path_str = db_path.to_str().expect("db path").to_string();
    (temp_dir, db_path_str)
}

fn startup_backup_files(db_path: &Path) -> Vec<PathBuf> {
    let Some(parent) = db_path.parent() else {
        return Vec::new();
    };
    let Some(base_name) = db_path.file_name().and_then(|name| name.to_str()) else {
        return Vec::new();
    };
    let prefix = format!("{base_name}.backup.");

    let mut paths = std::fs::read_dir(parent)
        .expect("read backup parent")
        .map(|entry| entry.expect("backup entry").path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(&prefix) && name.ends_with(".redb"))
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

#[test]
fn database_new_reports_error_for_non_directory_db_path() {
    let temp_dir = TempDir::new().expect("temp dir");
    let db_path = temp_dir.path().join("not-a-db-file");
    std::fs::write(&db_path, b"not-a-db").expect("seed file");

    let result = open_test_database_result(db_path.to_str().expect("path"));
    assert!(
        matches!(result, Err(AppError::StorageMessage(_))),
        "opening a non-directory DB_PATH should fail"
    );
}

#[test]
fn database_new_creates_backup_before_existing_schema_repair() {
    let temp_dir = TempDir::new().expect("temp dir");
    let db_path = temp_dir.path().join("db");
    std::fs::create_dir_all(&db_path).expect("create db dir");
    let db_file = db_path.join(REDB_FILE_NAME);

    let raw_db = redb::Database::create(&db_file).expect("create raw old db");
    let paste = Paste::new("important text".to_string(), "saved paste".to_string());
    let encoded = bincode::serialize(&paste).expect("serialize paste");
    let write_txn = raw_db.begin_write().expect("begin write");
    {
        let mut pastes = write_txn.open_table(PASTES).expect("open pastes");
        pastes
            .insert(paste.id.as_str(), encoded.as_slice())
            .expect("insert paste");
    }
    write_txn.commit().expect("commit raw old db");
    drop(raw_db);

    assert!(
        startup_backup_files(&db_path).is_empty(),
        "test setup should start without backup files"
    );

    let db = open_test_database(db_path.to_str().expect("db path"));
    assert!(
        db.pastes
            .get(paste.id.as_str())
            .expect("load paste")
            .is_some(),
        "startup repair must preserve canonical paste rows"
    );
    drop(db);

    let backup_files = startup_backup_files(&db_path);
    assert_eq!(
        backup_files.len(),
        1,
        "existing databases that need schema repair must be snapshotted first"
    );

    let backup_db = redb::Database::create(&backup_files[0]).expect("open backup");
    let read_txn = backup_db.begin_read().expect("begin backup read");
    let pastes = read_txn.open_table(PASTES).expect("open backup pastes");
    assert!(
        pastes
            .get(paste.id.as_str())
            .expect("backup paste lookup")
            .is_some(),
        "startup backup must contain pre-repair paste rows"
    );
}

#[test]
fn database_new_does_not_backup_current_schema_on_normal_reopen() {
    let (_temp_dir, db_path_str) = setup_temp_db_path("db");
    let db_path = Path::new(&db_path_str).to_path_buf();

    let db = open_test_database(&db_path_str);
    drop(db);
    assert!(
        startup_backup_files(&db_path).is_empty(),
        "new database creation should not create a compatibility backup"
    );

    let reopened = open_test_database(&db_path_str);
    let read_txn = reopened.db.begin_read().expect("begin read");
    let meta_state = read_txn
        .open_table(PASTES_META_STATE)
        .expect("open meta state");
    assert!(
        meta_state
            .get(META_SCHEMA_VERSION_KEY)
            .expect("schema lookup")
            .is_some(),
        "schema marker should be current after initial startup"
    );
    drop(read_txn);
    drop(reopened);

    assert!(
        startup_backup_files(&db_path).is_empty(),
        "current-schema reopen should not create a compatibility backup"
    );
}

#[test]
fn database_new_repairs_folder_count_drift_on_restart() {
    let (_temp_dir, db_path_str) = setup_temp_db_path("test.db");

    let db = open_test_database(&db_path_str);
    let folder = Folder::new("count-drift-folder".to_string());
    let folder_id = folder.id.clone();
    db.folders.create(&folder).expect("create folder");

    let mut paste_a = Paste::new("one".to_string(), "one".to_string());
    paste_a.folder_id = Some(folder_id.clone());
    let mut paste_b = Paste::new("two".to_string(), "two".to_string());
    paste_b.folder_id = Some(folder_id.clone());
    TransactionOps::create_paste_with_folder(&db, &paste_a, &folder_id).expect("create");
    TransactionOps::create_paste_with_folder(&db, &paste_b, &folder_id).expect("create");

    db.folders.set_count(&folder_id, 99).expect("drift");
    drop(db);

    let reopened = open_test_database(&db_path_str);
    let folder_after = reopened
        .folders
        .get(&folder_id)
        .expect("get folder")
        .expect("exists");
    let canonical_count = reopened
        .pastes
        .list(100, Some(folder_id.clone()))
        .expect("list")
        .len();
    assert_eq!(folder_after.paste_count, canonical_count);
}

#[test]
fn database_new_repairs_orphan_folder_refs_on_restart() {
    let (_temp_dir, db_path_str) = setup_temp_db_path("test.db");

    let db = open_test_database(&db_path_str);
    let folder = Folder::new("orphan-folder".to_string());
    let folder_id = folder.id.clone();
    db.folders.create(&folder).expect("create folder");

    let mut paste = Paste::new("orphan body".to_string(), "orphan".to_string());
    paste.folder_id = Some(folder_id.clone());
    let paste_id = paste.id.clone();
    TransactionOps::create_paste_with_folder(&db, &paste, &folder_id).expect("create");

    db.folders.delete(&folder_id).expect("delete folder");
    drop(db);

    let reopened = open_test_database(&db_path_str);
    let repaired = reopened
        .pastes
        .get(&paste_id)
        .expect("get paste")
        .expect("paste exists");
    assert!(repaired.folder_id.is_none());
}

#[test]
fn database_new_repairs_orphan_folder_parent_refs_on_restart() {
    let (_temp_dir, db_path_str) = setup_temp_db_path("test.db");

    let db = open_test_database(&db_path_str);
    let root = Folder::new("root".to_string());
    let root_id = root.id.clone();
    db.folders.create(&root).expect("create root");

    let child = Folder::with_parent("child".to_string(), Some(root_id.clone()));
    let child_id = child.id.clone();
    db.folders.create(&child).expect("create child");

    db.folders.delete(&root_id).expect("delete root");
    drop(db);

    let reopened = open_test_database(&db_path_str);
    let repaired_child = reopened
        .folders
        .get(&child_id)
        .expect("get child")
        .expect("child exists");
    assert!(repaired_child.parent_id.is_none());
}

#[test]
fn database_new_clears_stale_folder_delete_markers() {
    let (_temp_dir, db_path_str) = setup_temp_db_path("test.db");

    let db = open_test_database(&db_path_str);
    let folder = Folder::new("marker-folder".to_string());
    let folder_id = folder.id.clone();
    db.folders.create(&folder).expect("create folder");
    db.folders
        .mark_deleting(std::slice::from_ref(&folder_id))
        .expect("mark");
    assert!(db.folders.is_delete_marked(&folder_id).expect("marked"));
    drop(db);

    let reopened = open_test_database(&db_path_str);
    assert!(!reopened
        .folders
        .is_delete_marked(&folder_id)
        .expect("marked"));
}

#[test]
fn database_new_rejects_legacy_sled_layout_when_data_redb_missing() {
    let temp_dir = TempDir::new().expect("temp dir");
    let db_path = temp_dir.path().join("legacy-db");
    std::fs::create_dir_all(&db_path).expect("create dir");
    std::fs::write(db_path.join("pastes"), b"legacy").expect("seed legacy artifact");

    let err = match open_test_database_result(db_path.to_str().expect("path")) {
        Ok(_) => panic!("legacy sled layout without data.redb should fail"),
        Err(err) => err,
    };
    match err {
        AppError::StorageMessage(message) => {
            assert!(
                message.contains("legacy sled"),
                "error should describe legacy sled detection: {}",
                message
            );
            assert!(
                message.contains(REDB_FILE_NAME),
                "error should mention expected redb file: {}",
                message
            );
        }
        other => panic!("unexpected error variant: {:?}", other),
    }
}

#[test]
fn database_new_ignores_unrelated_lock_files_when_data_redb_missing() {
    let temp_dir = TempDir::new().expect("temp dir");
    let db_path = temp_dir.path().join("non-legacy-db");
    std::fs::create_dir_all(&db_path).expect("create dir");
    std::fs::write(db_path.join("random.lock"), b"not-sled").expect("seed lock artifact");

    let db = open_test_database(db_path.to_str().expect("path"));
    drop(db);

    assert!(
        db_path.join(REDB_FILE_NAME).exists(),
        "database should initialize data.redb when no legacy sled markers are present"
    );
}

#[test]
fn database_new_allows_startup_when_data_redb_exists() {
    let (_temp_dir, db_path_str) = setup_temp_db_path("db");
    let db = open_test_database(&db_path_str);
    drop(db);

    // Add legacy-looking artifacts; `data.redb` still exists and should win.
    let db_path = std::path::Path::new(&db_path_str);
    std::fs::write(db_path.join("pastes"), b"legacy").expect("seed legacy artifact");
    open_test_database(&db_path_str);
}
