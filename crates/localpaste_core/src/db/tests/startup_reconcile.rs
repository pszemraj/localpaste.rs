//! Startup reconciliation and degraded-mode behavior tests.

use super::*;
use crate::env::{env_lock, EnvGuard};

#[test]
fn test_database_new_reconciles_missing_meta_indexes() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let db_path_str = db_path.to_str().unwrap().to_string();

    let db = Database::new(&db_path_str).unwrap();
    let paste = Paste::new("seed".to_string(), "seed".to_string());
    let paste_id = paste.id.clone();
    db.pastes.create(&paste).unwrap();

    let meta_tree = db.db.open_tree("pastes_meta").unwrap();
    let updated_tree = db.db.open_tree("pastes_by_updated").unwrap();
    meta_tree.clear().unwrap();
    updated_tree.clear().unwrap();
    drop(meta_tree);
    drop(updated_tree);
    drop(db);

    let reopened = Database::new(&db_path_str).unwrap();
    let metas = reopened.pastes.list_meta(10, None).unwrap();
    assert!(metas.into_iter().any(|m| m.id == paste_id));
}

#[test]
fn test_database_new_reconciles_when_meta_marker_missing() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let db_path_str = db_path.to_str().unwrap().to_string();

    let db = Database::new(&db_path_str).unwrap();
    let state_tree = db.db.open_tree("pastes_meta_state").unwrap();
    assert!(state_tree.get("version").unwrap().is_some());
    state_tree.remove("version").unwrap();
    drop(state_tree);
    drop(db);

    let reopened = Database::new(&db_path_str).unwrap();
    let reopened_state = reopened.db.open_tree("pastes_meta_state").unwrap();
    assert!(
        reopened_state.get("version").unwrap().is_some(),
        "missing marker should be recreated during startup reconcile"
    );
}

#[test]
fn test_database_new_force_reindex_repairs_corrupt_meta_rows() {
    let _lock = env_lock().lock().expect("env lock");
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let db_path_str = db_path.to_str().unwrap().to_string();

    let db = Database::new(&db_path_str).unwrap();
    let paste = Paste::new("force reindex".to_string(), "force-reindex".to_string());
    let paste_id = paste.id.clone();
    db.pastes.create(&paste).unwrap();

    let meta_tree = db.db.open_tree("pastes_meta").unwrap();
    meta_tree
        .insert(paste_id.as_bytes(), b"corrupt-meta")
        .unwrap();
    drop(meta_tree);
    drop(db);

    let _reindex_guard = EnvGuard::set("LOCALPASTE_REINDEX", "1");

    let reopened = Database::new(&db_path_str).unwrap();
    let metas = reopened.pastes.list_meta(10, None).unwrap();
    assert!(
        metas.into_iter().any(|meta| meta.id == paste_id),
        "forced reindex should rebuild metadata entries"
    );
}

#[test]
fn test_database_new_reports_error_for_corrupt_storage_path() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("not-a-db-file");
    std::fs::write(&db_path, b"not-a-sled-db").unwrap();

    let result = Database::new(db_path.to_str().unwrap());
    assert!(
        matches!(result, Err(AppError::DatabaseError(_))),
        "opening a non-directory/non-db path should return a database error"
    );
}

#[test]
fn test_database_new_reconciles_derived_only_rows_on_startup() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let db_path_str = db_path.to_str().unwrap().to_string();

    let db = Database::new(&db_path_str).unwrap();
    let paste = Paste::new("ghost".to_string(), "ghost".to_string());
    let paste_id = paste.id.clone();
    db.pastes.create(&paste).unwrap();

    let canonical_tree = db.db.open_tree("pastes").unwrap();
    canonical_tree.remove(paste_id.as_bytes()).unwrap();
    drop(canonical_tree);
    drop(db);

    let reopened = Database::new(&db_path_str).unwrap();
    assert!(
        reopened.pastes.get(&paste_id).unwrap().is_none(),
        "canonical row should remain deleted"
    );
    assert!(
        reopened
            .pastes
            .list_meta(10, None)
            .unwrap()
            .into_iter()
            .all(|meta| meta.id != paste_id),
        "startup reconcile should remove ghost metadata rows"
    );

    let meta_tree = reopened.db.open_tree("pastes_meta").unwrap();
    assert!(
        meta_tree.get(paste_id.as_bytes()).unwrap().is_none(),
        "metadata tree should not retain derived-only row after startup reconcile"
    );
    let updated_tree = reopened.db.open_tree("pastes_by_updated").unwrap();
    let has_updated_ref = updated_tree
        .iter()
        .filter_map(|item| item.ok())
        .any(|(_, value)| value.as_ref() == paste_id.as_bytes());
    assert!(
        !has_updated_ref,
        "recency index should not retain derived-only references after startup reconcile"
    );
}

#[test]
fn test_database_new_reconciles_folder_count_drift() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let db_path_str = db_path.to_str().unwrap().to_string();

    let db = Database::new(&db_path_str).unwrap();
    let folder = Folder::new("count-drift-folder".to_string());
    let folder_id = folder.id.clone();
    db.folders.create(&folder).unwrap();

    let mut paste_a = Paste::new("one".to_string(), "one".to_string());
    paste_a.folder_id = Some(folder_id.clone());
    let mut paste_b = Paste::new("two".to_string(), "two".to_string());
    paste_b.folder_id = Some(folder_id.clone());
    TransactionOps::create_paste_with_folder(&db, &paste_a, &folder_id).unwrap();
    TransactionOps::create_paste_with_folder(&db, &paste_b, &folder_id).unwrap();

    db.folders.set_count(&folder_id, 99).unwrap();
    drop(db);

    let reopened = Database::new(&db_path_str).unwrap();
    let folder_after = reopened
        .folders
        .get(&folder_id)
        .unwrap()
        .expect("folder exists");
    let canonical_count = reopened
        .pastes
        .list(100, Some(folder_id.clone()))
        .unwrap()
        .len();
    assert_eq!(
        folder_after.paste_count, canonical_count,
        "startup reconcile must repair folder paste_count drift"
    );
}

#[test]
fn test_database_new_reconciles_orphan_folder_refs() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let db_path_str = db_path.to_str().unwrap().to_string();

    let db = Database::new(&db_path_str).unwrap();
    let folder = Folder::new("orphan-folder".to_string());
    let folder_id = folder.id.clone();
    db.folders.create(&folder).unwrap();

    let mut paste = Paste::new("orphan body".to_string(), "orphan".to_string());
    paste.folder_id = Some(folder_id.clone());
    let paste_id = paste.id.clone();
    TransactionOps::create_paste_with_folder(&db, &paste, &folder_id).unwrap();

    db.folders.delete(&folder_id).unwrap();
    drop(db);

    let reopened = Database::new(&db_path_str).unwrap();
    let repaired = reopened
        .pastes
        .get(&paste_id)
        .unwrap()
        .expect("paste should still exist");
    assert!(
        repaired.folder_id.is_none(),
        "startup reconcile must clear folder_id references to missing folders"
    );
}

#[test]
fn test_database_new_continues_in_degraded_mode_when_meta_reconcile_fails() {
    let _lock = reconcile_failpoint_test_lock()
        .lock()
        .expect("reconcile failpoint lock");
    let _guard = ReconcileFailpointGuard;

    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let db_path_str = db_path.to_str().unwrap().to_string();

    set_reconcile_failpoint(true);
    let db =
        Database::new(&db_path_str).expect("startup should continue when metadata reconcile fails");
    set_reconcile_failpoint(false);

    let state_tree = db.db.open_tree("pastes_meta_state").unwrap();
    let in_progress = state_tree
        .get("in_progress_count")
        .unwrap()
        .expect("in-progress marker");
    assert_eq!(
        u64::from_be_bytes(in_progress.as_ref().try_into().expect("u64 marker bytes")),
        0,
        "failed startup reconcile must not leave in-progress stuck"
    );
    let faulted = state_tree.get("faulted").unwrap().expect("faulted marker");
    assert_eq!(
        faulted.as_ref(),
        &[1u8],
        "failed startup reconcile should mark metadata indexes faulted"
    );
    assert!(
        db.pastes
            .needs_reconcile_meta_indexes(false)
            .expect("needs reconcile"),
        "runtime should keep reconcile-needed marker in degraded startup mode"
    );

    let paste = Paste::new(
        "degraded mode body".to_string(),
        "degraded-mode".to_string(),
    );
    let paste_id = paste.id.clone();
    db.pastes
        .create(&paste)
        .expect("create paste in degraded mode");
    let listed = db.pastes.list_meta(10, None).expect("list meta fallback");
    assert!(
        listed.iter().any(|meta| meta.id == paste_id),
        "degraded mode must keep canonical rows visible via fallback"
    );
}

#[test]
fn test_database_new_clears_stale_folder_delete_markers() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let db_path_str = db_path.to_str().unwrap().to_string();

    let db = Database::new(&db_path_str).unwrap();
    let folder = Folder::new("marker-folder".to_string());
    let folder_id = folder.id.clone();
    db.folders.create(&folder).unwrap();
    db.folders
        .mark_deleting(std::slice::from_ref(&folder_id))
        .unwrap();
    assert!(db.folders.is_delete_marked(&folder_id).unwrap());
    drop(db);

    let reopened = Database::new(&db_path_str).unwrap();
    assert!(
        !reopened.folders.is_delete_marked(&folder_id).unwrap(),
        "startup should clear stale folder delete markers"
    );
}
