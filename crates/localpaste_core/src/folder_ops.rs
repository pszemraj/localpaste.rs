//! Shared folder tree operations used by API handlers and GUI backend workers.

use crate::{
    db::TransactionOps,
    models::{folder::Folder, paste::UpdatePasteRequest},
    AppError, Database,
};
use std::collections::{HashMap, HashSet};

/// Validate that a folder can accept new paste assignments.
///
/// # Arguments
/// - `db`: Open database handle.
/// - `folder_id`: Target folder id.
///
/// # Returns
/// `Ok(())` when the folder exists and is not in an active delete set.
///
/// # Errors
/// Returns [`AppError::NotFound`] when the folder does not exist, or
/// [`AppError::BadRequest`] when delete is in progress for the folder.
pub fn ensure_folder_assignable(db: &Database, folder_id: &str) -> Result<(), AppError> {
    if db.folders.get(folder_id)?.is_none() {
        return Err(AppError::NotFound);
    }
    if db.folders.is_delete_marked(folder_id)? {
        return Err(AppError::BadRequest(format!(
            "Folder with id '{}' is being deleted",
            folder_id
        )));
    }
    Ok(())
}

/// Returns `true` if assigning `folder_id` under `new_parent_id` introduces a cycle.
///
/// # Arguments
/// - `folders`: Full folder list representing the current tree.
/// - `folder_id`: Folder being re-parented.
/// - `new_parent_id`: Proposed parent folder id.
///
/// # Returns
/// `true` when the proposed parent would introduce a cycle.
pub fn introduces_cycle(folders: &[Folder], folder_id: &str, new_parent_id: &str) -> bool {
    let parent_map: HashMap<&str, Option<&str>> = folders
        .iter()
        .map(|f| (f.id.as_str(), f.parent_id.as_deref()))
        .collect();
    let mut current = Some(new_parent_id);
    let mut visited = HashSet::new();

    while let Some(curr) = current {
        if !visited.insert(curr) || curr == folder_id {
            return true;
        }
        current = parent_map.get(curr).copied().flatten();
    }

    false
}

/// Collect descendants (including `root_id`) in child-first delete order.
///
/// # Arguments
/// - `folders`: Full folder list representing the current tree.
/// - `root_id`: Root folder id to delete.
///
/// # Returns
/// Folder ids ordered so children are deleted before parents.
pub fn folder_delete_order(folders: &[Folder], root_id: &str) -> Vec<String> {
    let mut to_visit = vec![root_id.to_string()];
    let mut discovered = Vec::new();
    let mut visited = HashSet::new();

    while let Some(current) = to_visit.pop() {
        if !visited.insert(current.clone()) {
            continue;
        }
        discovered.push(current.clone());
        for child in folders
            .iter()
            .filter(|f| f.parent_id.as_deref() == Some(current.as_str()))
        {
            to_visit.push(child.id.clone());
        }
    }

    discovered.reverse();
    discovered
}

/// Deletes a folder tree and migrates all affected pastes to unfiled.
///
/// Returns the folder ids that were deleted in execution order.
///
/// # Arguments
/// - `db`: Open database handle.
/// - `root_id`: Root folder id to delete.
///
/// # Returns
/// Deleted folder ids in execution order (children first, root last).
///
/// # Errors
/// Returns [`AppError::NotFound`] when `root_id` does not exist, or storage errors when
/// folder/paste updates fail.
pub fn delete_folder_tree_and_migrate(
    db: &Database,
    root_id: &str,
) -> Result<Vec<String>, AppError> {
    delete_folder_tree_and_migrate_guarded(db, root_id, |_| Ok(()))
}

/// Deletes a folder tree while holding an external guard for affected paste ids.
///
/// The caller-provided `acquire_guard` callback executes while the folder
/// transaction lock is held and receives the exact canonical paste ids that
/// belong to the delete set. The returned guard is held for the full migration
/// and delete flow.
///
/// # Arguments
/// - `db`: Open database handle.
/// - `root_id`: Root folder id to delete.
/// - `acquire_guard`: Callback that acquires an external guard for affected ids.
///
/// # Returns
/// Deleted folder ids in execution order (children first, root last).
///
/// # Errors
/// Returns [`AppError::NotFound`] when `root_id` does not exist, or any error
/// returned by `acquire_guard` / storage mutations.
pub fn delete_folder_tree_and_migrate_guarded<G, F>(
    db: &Database,
    root_id: &str,
    acquire_guard: F,
) -> Result<Vec<String>, AppError>
where
    F: FnOnce(&[String]) -> Result<G, AppError>,
{
    let _folder_guard = TransactionOps::acquire_folder_txn_lock(db)?;
    let delete_order = folder_delete_order_for_root_locked(db, root_id)?;
    let affected_paste_ids = collect_affected_paste_ids_locked(db, &delete_order)?;
    let _external_guard = acquire_guard(&affected_paste_ids)?;
    delete_folder_tree_and_migrate_with_order_locked(db, delete_order)
}

fn folder_delete_order_for_root_locked(
    db: &Database,
    root_id: &str,
) -> Result<Vec<String>, AppError> {
    let folders = db.folders.list()?;
    if !folders.iter().any(|f| f.id == root_id) {
        return Err(AppError::NotFound);
    }
    Ok(folder_delete_order(&folders, root_id))
}

fn collect_affected_paste_ids_locked(
    db: &Database,
    delete_order: &[String],
) -> Result<Vec<String>, AppError> {
    let delete_set: HashSet<&str> = delete_order.iter().map(|id| id.as_str()).collect();
    let mut affected_paste_ids = Vec::new();
    db.pastes.scan_canonical_meta(|meta| {
        if meta
            .folder_id
            .as_deref()
            .map(|folder_id| delete_set.contains(folder_id))
            .unwrap_or(false)
        {
            affected_paste_ids.push(meta.id);
        }
        Ok(())
    })?;
    affected_paste_ids.sort();
    affected_paste_ids.dedup();
    Ok(affected_paste_ids)
}

fn delete_folder_tree_and_migrate_with_order_locked(
    db: &Database,
    delete_order: Vec<String>,
) -> Result<Vec<String>, AppError> {
    db.folders.mark_deleting(&delete_order)?;
    let result = (|| {
        for folder_id in &delete_order {
            migrate_folder_pastes_to_unfiled(db, folder_id)?;
            db.folders.delete(folder_id)?;
        }

        // list_meta can temporarily mask stale rows by falling back to canonical data.
        // Rebuild indexes from canonical rows so folder deletions do not leave persistent
        // metadata/index ghosts that force repeated runtime fallback scans.
        if let Err(err) = reconcile_meta_indexes_after_folder_delete(db) {
            tracing::error!(
                "Folder delete committed but metadata reconcile failed; leaving reconcile marker for repair: {}",
                err
            );
        }

        Ok(delete_order.clone())
    })();

    if let Err(unmark_err) = db.folders.unmark_deleting(&delete_order) {
        tracing::error!(
            "Failed to clear folder delete markers after delete flow: {}",
            unmark_err
        );
    }

    result
}

fn migrate_folder_pastes_to_unfiled(db: &Database, folder_id: &str) -> Result<(), AppError> {
    loop {
        let paste_ids = db.pastes.list_canonical_ids_batch(100, Some(folder_id))?;
        if paste_ids.is_empty() {
            break;
        }

        for paste_id in paste_ids {
            let update = UpdatePasteRequest {
                content: None,
                name: None,
                language: None,
                language_is_manual: None,
                folder_id: Some(String::new()), // normalized to None in PasteDb::update
                tags: None,
            };
            let moved =
                TransactionOps::move_paste_between_folders_locked(db, &paste_id, None, update)?;
            if moved.is_none() {
                continue;
            }
        }
    }
    Ok(())
}

#[cfg(not(test))]
fn reconcile_meta_indexes_after_folder_delete(db: &Database) -> Result<(), AppError> {
    db.pastes.reconcile_meta_indexes()
}

#[cfg(test)]
fn reconcile_meta_indexes_after_folder_delete(db: &Database) -> Result<(), AppError> {
    maybe_inject_delete_reconcile_failpoint(db)?;
    db.pastes.reconcile_meta_indexes()
}

/// Reconcile folder invariants from canonical paste rows.
///
/// Repairs two classes of drift:
/// 1. Canonical pastes referencing missing folders are moved to unfiled.
/// 2. `Folder.paste_count` is reset to exact canonical ownership counts.
///
/// # Returns
/// `Ok(())` after canonical references and counts are repaired.
///
/// # Errors
/// Returns storage/serialization errors when reconciliation cannot complete.
pub fn reconcile_folder_invariants(db: &Database) -> Result<(), AppError> {
    // Keep folder-invariant repair serialized with folder-affecting transactions even if this
    // helper is called outside startup in the future.
    let _guard = TransactionOps::acquire_folder_txn_lock(db)?;
    let folders = db.folders.list()?;
    let folder_id_set: HashSet<String> = folders.iter().map(|folder| folder.id.clone()).collect();
    let mut orphan_ids = Vec::new();
    let mut exact_counts: HashMap<String, usize> = HashMap::new();

    db.pastes.scan_canonical_meta(|meta| {
        let Some(folder_id) = meta.folder_id.as_deref() else {
            return Ok(());
        };
        if folder_id_set.contains(folder_id) {
            *exact_counts.entry(folder_id.to_string()).or_insert(0) += 1;
        } else {
            orphan_ids.push(meta.id);
        }
        Ok(())
    })?;

    const ORPHAN_REPAIR_BATCH: usize = 1024;
    for chunk in orphan_ids.chunks(ORPHAN_REPAIR_BATCH) {
        for paste_id in chunk {
            let update = UpdatePasteRequest {
                content: None,
                name: None,
                language: None,
                language_is_manual: None,
                folder_id: Some(String::new()),
                tags: None,
            };
            let _ = db.pastes.update(paste_id, update)?;
        }
    }

    for folder in db.folders.list()? {
        let count = exact_counts.get(folder.id.as_str()).copied().unwrap_or(0);
        db.folders.set_count(folder.id.as_str(), count)?;
    }

    Ok(())
}

#[cfg(test)]
thread_local! {
    static DELETE_RECONCILE_FAILPOINT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
pub(crate) fn set_delete_reconcile_failpoint(enabled: bool) {
    DELETE_RECONCILE_FAILPOINT.with(|slot| slot.set(enabled));
}

#[cfg(test)]
fn maybe_inject_delete_reconcile_failpoint(db: &Database) -> Result<(), AppError> {
    let enabled = DELETE_RECONCILE_FAILPOINT.with(|slot| {
        let enabled = slot.get();
        if enabled {
            slot.set(false);
        }
        enabled
    });
    if !enabled {
        return Ok(());
    }
    let tree = db.db.open_tree("pastes")?;
    tree.insert(
        b"folder-delete-reconcile-failpoint-row",
        b"corrupt-canonical-row",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db::TransactionOps, models::paste::Paste};
    use std::sync::{Arc, Barrier};
    use std::thread;
    use tempfile::TempDir;

    fn setup_db() -> (Database, TempDir) {
        let dir = TempDir::new().expect("temp dir");
        let db_path = dir.path().join("db");
        let db = Database::new(db_path.to_str().expect("db path")).expect("db");
        (db, dir)
    }

    struct DeleteReconcileFailpointGuard;

    impl Drop for DeleteReconcileFailpointGuard {
        fn drop(&mut self) {
            set_delete_reconcile_failpoint(false);
        }
    }

    #[test]
    fn detects_folder_cycle() {
        let root = Folder::with_parent("root".to_string(), None);
        let child = Folder::with_parent("child".to_string(), Some(root.id.clone()));
        let leaf = Folder::with_parent("leaf".to_string(), Some(child.id.clone()));
        let folders = vec![root.clone(), child.clone(), leaf];

        assert!(introduces_cycle(&folders, &root.id, &child.id));
        assert!(!introduces_cycle(&folders, &child.id, &root.id));
    }

    #[test]
    fn delete_order_is_children_first() {
        let root = Folder::with_parent("root".to_string(), None);
        let child = Folder::with_parent("child".to_string(), Some(root.id.clone()));
        let leaf = Folder::with_parent("leaf".to_string(), Some(child.id.clone()));
        let order = folder_delete_order(&[root.clone(), child.clone(), leaf.clone()], &root.id);

        assert_eq!(order.len(), 3);
        assert_eq!(order.last(), Some(&root.id));
        assert!(order.contains(&child.id));
        assert!(order.contains(&leaf.id));
    }

    #[test]
    fn delete_tree_migrates_pastes() {
        let (db, _dir) = setup_db();

        let root = Folder::with_parent("root".to_string(), None);
        let child = Folder::with_parent("child".to_string(), Some(root.id.clone()));
        db.folders.create(&root).expect("create root");
        db.folders.create(&child).expect("create child");

        let mut paste = Paste::new("content".to_string(), "name".to_string());
        paste.folder_id = Some(child.id.clone());
        db.pastes.create(&paste).expect("create paste");

        let deleted = delete_folder_tree_and_migrate(&db, &root.id).expect("delete tree");
        assert_eq!(deleted.last(), Some(&root.id));

        let moved = db.pastes.get(&paste.id).expect("get").expect("exists");
        assert_eq!(moved.folder_id, None);
    }

    #[test]
    fn delete_tree_guarded_rejects_locked_descendant() {
        let (db, _dir) = setup_db();

        let root = Folder::with_parent("root".to_string(), None);
        db.folders.create(&root).expect("create root");

        let mut paste = Paste::new("content".to_string(), "name".to_string());
        paste.folder_id = Some(root.id.clone());
        db.pastes.create(&paste).expect("create paste");

        let locked_ids = HashSet::from([paste.id.clone()]);
        let err = delete_folder_tree_and_migrate_guarded(&db, &root.id, |affected_paste_ids| {
            if let Some(locked_id) = affected_paste_ids
                .iter()
                .find(|paste_id| locked_ids.contains(paste_id.as_str()))
            {
                return Err::<(), AppError>(AppError::Locked(format!(
                    "Folder delete would migrate locked paste '{}'; close it first.",
                    locked_id
                )));
            }
            Ok(())
        })
        .expect_err("locked descendant should block delete");
        assert!(
            matches!(err, AppError::Locked(_)),
            "expected locked error, got {err}"
        );

        let still_foldered = db
            .pastes
            .get(&paste.id)
            .expect("lookup")
            .expect("paste should still exist");
        assert_eq!(still_foldered.folder_id.as_deref(), Some(root.id.as_str()));
        assert!(
            db.folders.get(&root.id).expect("folder lookup").is_some(),
            "folder should remain when delete is blocked"
        );
    }

    #[test]
    fn delete_tree_guarded_can_reject_using_affected_paste_ids() {
        let (db, _dir) = setup_db();

        let root = Folder::with_parent("root".to_string(), None);
        let child = Folder::with_parent("child".to_string(), Some(root.id.clone()));
        db.folders.create(&root).expect("create root");
        db.folders.create(&child).expect("create child");

        let mut in_scope = Paste::new("in-scope".to_string(), "name".to_string());
        in_scope.folder_id = Some(child.id.clone());
        db.pastes.create(&in_scope).expect("create in-scope paste");

        let out_of_scope = Paste::new("out-of-scope".to_string(), "name".to_string());
        db.pastes
            .create(&out_of_scope)
            .expect("create out-of-scope paste");

        let err = delete_folder_tree_and_migrate_guarded(&db, &root.id, |affected_paste_ids| {
            assert!(
                affected_paste_ids.contains(&in_scope.id),
                "guard should receive descendant paste ids"
            );
            assert!(
                !affected_paste_ids.contains(&out_of_scope.id),
                "guard should exclude unrelated paste ids"
            );
            Err::<(), AppError>(AppError::Locked("guard rejected delete".to_string()))
        })
        .expect_err("guard should reject delete");

        assert!(matches!(err, AppError::Locked(_)));
        assert!(
            db.folders.get(&root.id).expect("root lookup").is_some(),
            "root folder should remain after guarded rejection"
        );
        assert!(
            db.folders.get(&child.id).expect("child lookup").is_some(),
            "child folder should remain after guarded rejection"
        );
        let in_scope_after = db
            .pastes
            .get(&in_scope.id)
            .expect("in-scope lookup")
            .expect("in-scope paste still exists");
        assert_eq!(in_scope_after.folder_id.as_deref(), Some(child.id.as_str()));
    }

    #[test]
    fn delete_tree_handles_orphaned_meta_rows() {
        let (db, _dir) = setup_db();

        let root = Folder::with_parent("root".to_string(), None);
        db.folders.create(&root).expect("create root");

        let mut paste = Paste::new("content".to_string(), "name".to_string());
        paste.folder_id = Some(root.id.clone());
        db.pastes.create(&paste).expect("create paste");

        // Simulate interrupted write: canonical row removed while metadata/index rows remain.
        db.db
            .open_tree("pastes")
            .expect("pastes tree")
            .remove(paste.id.as_bytes())
            .expect("remove canonical");
        let stale = db
            .pastes
            .list_meta(10, Some(root.id.clone()))
            .expect("list stale meta");
        assert_eq!(
            stale.len(),
            0,
            "metadata listing should fall back to canonical rows and hide stale ghost entries"
        );

        let deleted = delete_folder_tree_and_migrate(&db, &root.id).expect("delete tree");
        assert_eq!(deleted, vec![root.id.clone()]);
        assert!(
            db.folders.get(&root.id).expect("folder lookup").is_none(),
            "folder should be deleted despite stale metadata row"
        );
        assert!(
            db.pastes
                .list_meta(10, Some(root.id.clone()))
                .expect("list after delete")
                .is_empty(),
            "metadata index should be reconciled to remove orphan row"
        );
        let meta_tree = db.db.open_tree("pastes_meta").expect("meta tree");
        assert!(
            meta_tree
                .get(paste.id.as_bytes())
                .expect("meta lookup")
                .is_none(),
            "reconcile should remove orphaned metadata row"
        );
        let updated_tree = db.db.open_tree("pastes_by_updated").expect("updated tree");
        let stale_updated_ref = updated_tree
            .iter()
            .filter_map(|item| item.ok())
            .any(|(_, value)| value.as_ref() == paste.id.as_bytes());
        assert!(
            !stale_updated_ref,
            "reconcile should remove orphaned recency-index references"
        );
    }

    #[test]
    fn delete_tree_migrates_when_metadata_row_is_missing() {
        let (db, _dir) = setup_db();

        let root = Folder::with_parent("root".to_string(), None);
        db.folders.create(&root).expect("create root");

        let mut paste = Paste::new("content".to_string(), "name".to_string());
        paste.folder_id = Some(root.id.clone());
        db.pastes.create(&paste).expect("create paste");

        db.db
            .open_tree("pastes_meta")
            .expect("meta tree")
            .remove(paste.id.as_bytes())
            .expect("remove canonical");

        let deleted = delete_folder_tree_and_migrate(&db, &root.id).expect("delete tree");
        assert_eq!(deleted, vec![root.id.clone()]);
        let moved = db
            .pastes
            .get(&paste.id)
            .expect("get paste")
            .expect("paste should still exist");
        assert_eq!(moved.folder_id, None);
        assert!(
            db.pastes
                .list(10, Some(root.id.clone()))
                .expect("list")
                .is_empty(),
            "canonical rows for deleted folder should be migrated"
        );
    }

    #[test]
    fn ensure_folder_assignable_rejects_missing_and_marked_folders() {
        let (db, _dir) = setup_db();

        assert!(
            matches!(
                ensure_folder_assignable(&db, "missing-folder"),
                Err(AppError::NotFound)
            ),
            "missing folder must be rejected"
        );

        let folder = Folder::new("root".to_string());
        let folder_id = folder.id.clone();
        db.folders.create(&folder).expect("create folder");
        db.folders
            .mark_deleting(std::slice::from_ref(&folder_id))
            .expect("mark deleting");
        assert!(
            matches!(
                ensure_folder_assignable(&db, folder_id.as_str()),
                Err(AppError::BadRequest(_))
            ),
            "delete-marked folder must be rejected for assignment"
        );
    }

    #[test]
    fn create_with_folder_rejects_when_folder_is_marked_for_delete() {
        let (db, _dir) = setup_db();

        let folder = Folder::new("root".to_string());
        let folder_id = folder.id.clone();
        db.folders.create(&folder).expect("create folder");
        db.folders
            .mark_deleting(std::slice::from_ref(&folder_id))
            .expect("mark deleting");

        let mut paste = Paste::new("content".to_string(), "name".to_string());
        paste.folder_id = Some(folder_id.clone());
        let paste_id = paste.id.clone();

        let result = TransactionOps::create_paste_with_folder(&db, &paste, &folder_id);
        assert!(
            matches!(result, Err(AppError::BadRequest(_))),
            "assignment into delete-marked folder must be blocked"
        );
        assert!(
            db.pastes.get(&paste_id).expect("lookup").is_none(),
            "failed create must not leave canonical row"
        );
    }

    #[test]
    fn delete_tree_returns_success_when_post_commit_reconcile_fails() {
        let _guard = DeleteReconcileFailpointGuard;
        let (db, _dir) = setup_db();

        let root = Folder::with_parent("root".to_string(), None);
        db.folders.create(&root).expect("create root");

        let mut paste = Paste::new("content".to_string(), "name".to_string());
        paste.folder_id = Some(root.id.clone());
        let paste_id = paste.id.clone();
        db.pastes.create(&paste).expect("create paste");

        set_delete_reconcile_failpoint(true);
        let deleted = delete_folder_tree_and_migrate(&db, &root.id).expect("delete tree");
        assert_eq!(deleted, vec![root.id.clone()]);
        assert!(
            db.folders.get(&root.id).expect("folder lookup").is_none(),
            "folder deletion should commit even when reconcile fails"
        );
        let moved = db
            .pastes
            .get(&paste_id)
            .expect("paste lookup")
            .expect("paste should still exist");
        assert_eq!(moved.folder_id, None);
        assert!(
            db.pastes
                .needs_reconcile_meta_indexes(false)
                .expect("needs reconcile"),
            "failed reconcile should leave recovery markers for follow-up repair"
        );
    }

    #[test]
    fn delete_folder_tree_and_concurrent_move_preserve_no_orphan_and_counts() {
        let (db, _dir) = setup_db();

        let root = Folder::with_parent("root".to_string(), None);
        let child = Folder::with_parent("child".to_string(), Some(root.id.clone()));
        let target = Folder::with_parent("target".to_string(), None);
        db.folders.create(&root).expect("create root");
        db.folders.create(&child).expect("create child");
        db.folders.create(&target).expect("create target");

        let mut paste = Paste::new("content".to_string(), "name".to_string());
        paste.folder_id = Some(child.id.clone());
        let paste_id = paste.id.clone();
        TransactionOps::create_paste_with_folder(&db, &paste, &child.id).expect("create paste");

        let delete_db = db.share().expect("share db");
        let move_db = db.share().expect("share db");
        let barrier = Arc::new(Barrier::new(2));

        let delete_root = root.id.clone();
        let barrier_delete = barrier.clone();
        let delete_thread = thread::spawn(move || {
            barrier_delete.wait();
            delete_folder_tree_and_migrate(&delete_db, &delete_root)
        });

        let barrier_move = barrier;
        let move_target = target.id.clone();
        let move_paste = paste_id.clone();
        let move_thread = thread::spawn(move || {
            barrier_move.wait();
            let update = UpdatePasteRequest {
                content: None,
                name: None,
                language: None,
                language_is_manual: None,
                folder_id: Some(move_target.clone()),
                tags: None,
            };
            TransactionOps::move_paste_between_folders(
                &move_db,
                &move_paste,
                Some(move_target.as_str()),
                update,
            )
        });

        let delete_result = delete_thread.join().expect("delete join");
        assert!(
            delete_result.is_ok(),
            "delete flow should complete under concurrent move: {:?}",
            delete_result
        );
        let move_result = move_thread.join().expect("move join");
        assert!(
            move_result.is_ok()
                || matches!(
                    move_result,
                    Err(AppError::NotFound | AppError::BadRequest(_))
                ),
            "move should either succeed or fail with assignability rejection: {:?}",
            move_result
        );

        assert!(
            db.folders.get(&root.id).expect("root lookup").is_none(),
            "root should be deleted"
        );
        assert!(
            db.folders.get(&child.id).expect("child lookup").is_none(),
            "child should be deleted"
        );

        let current = db
            .pastes
            .get(&paste_id)
            .expect("paste lookup")
            .expect("paste should still exist");
        if let Some(folder_id) = current.folder_id.as_deref() {
            assert!(
                db.folders.get(folder_id).expect("folder lookup").is_some(),
                "paste must not reference a deleted folder"
            );
        }
        crate::test_support::assert_folder_counts_match_canonical(&db);
    }
}
