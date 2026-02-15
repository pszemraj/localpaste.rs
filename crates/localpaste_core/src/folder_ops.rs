//! Shared folder tree operations used by API handlers and GUI backend workers.

use crate::{
    db::TransactionOps,
    models::{folder::Folder, paste::UpdatePasteRequest},
    AppError, Database,
};
use redb::ReadableTable;
use std::collections::{HashMap, HashSet};

/// Validate that a folder can accept new paste assignments.
///
/// # Arguments
/// - `db`: Open database handle.
/// - `folder_id`: Candidate folder id.
///
/// # Returns
/// `Ok(())` when the folder exists and is not delete-marked.
///
/// # Errors
/// Returns [`AppError::NotFound`] for missing folders or
/// [`AppError::BadRequest`] when delete is in progress.
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
/// - `folders`: Full folder list.
/// - `folder_id`: Folder being re-parented.
/// - `new_parent_id`: Proposed parent id.
///
/// # Returns
/// `true` when the proposed parent would create a cycle.
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
/// - `folders`: Full folder list.
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
/// # Arguments
/// - `db`: Open database handle.
/// - `root_id`: Root folder id to delete.
///
/// # Returns
/// Deleted folder ids in execution order (children first, root last).
///
/// # Errors
/// Returns [`AppError::NotFound`] when `root_id` does not exist, or storage
/// errors when folder/paste mutations fail.
pub fn delete_folder_tree_and_migrate(
    db: &Database,
    root_id: &str,
) -> Result<Vec<String>, AppError> {
    delete_folder_tree_and_migrate_guarded(db, root_id, |_| Ok(()))
}

/// Deletes a folder tree while holding an external guard for affected paste ids.
///
/// # Arguments
/// - `db`: Open database handle.
/// - `root_id`: Root folder id to delete.
/// - `acquire_guard`: Callback that receives affected paste ids and returns a guard.
///
/// # Returns
/// Deleted folder ids in execution order (children first, root last).
///
/// # Errors
/// Returns [`AppError::NotFound`] when `root_id` does not exist, or any error
/// produced by `acquire_guard` / storage mutations.
pub fn delete_folder_tree_and_migrate_guarded<G, F>(
    db: &Database,
    root_id: &str,
    acquire_guard: F,
) -> Result<Vec<String>, AppError>
where
    F: FnOnce(&[String]) -> Result<G, AppError>,
{
    let folder_guard = TransactionOps::acquire_folder_txn_guard(db)?;
    let delete_order = folder_delete_order_for_root_locked(db, root_id)?;
    let affected_paste_ids = collect_affected_paste_ids_locked(db, &delete_order)?;
    let _external_guard = acquire_guard(&affected_paste_ids)?;
    delete_folder_tree_and_migrate_with_order_locked(db, &folder_guard, delete_order)
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
    _folder_guard: &crate::db::FolderTxnGuard<'_>,
    delete_order: Vec<String>,
) -> Result<Vec<String>, AppError> {
    use crate::db::paste::{apply_update_request, deserialize_paste, reverse_timestamp_key};
    use crate::db::tables::{FOLDERS, FOLDERS_DELETING, PASTES, PASTES_BY_UPDATED, PASTES_META};
    use crate::models::paste::PasteMeta;

    let delete_set: HashSet<&str> = delete_order.iter().map(|id| id.as_str()).collect();
    let clear_folder_update = UpdatePasteRequest {
        content: None,
        name: None,
        language: None,
        language_is_manual: None,
        folder_id: Some(String::new()),
        tags: None,
    };

    let write_txn = db.db.begin_write()?;
    {
        let mut pastes = write_txn.open_table(PASTES)?;
        let mut metas = write_txn.open_table(PASTES_META)?;
        let mut updated = write_txn.open_table(PASTES_BY_UPDATED)?;
        let mut folders = write_txn.open_table(FOLDERS)?;
        let mut deleting = write_txn.open_table(FOLDERS_DELETING)?;

        for folder_id in &delete_order {
            deleting.insert(folder_id.as_str(), ())?;
        }

        let mut updates = Vec::new();
        for entry in pastes.iter()? {
            let (id_guard, value_guard) = entry?;
            let mut paste = deserialize_paste(value_guard.value())?;
            let in_delete_tree = paste
                .folder_id
                .as_deref()
                .map(|folder_id| delete_set.contains(folder_id))
                .unwrap_or(false);
            if !in_delete_tree {
                continue;
            }
            let paste_id = id_guard.value().to_string();
            let old_recency_key = reverse_timestamp_key(paste.updated_at);
            apply_update_request(&mut paste, &clear_folder_update);
            let encoded_paste = bincode::serialize(&paste)?;
            let encoded_meta = bincode::serialize(&PasteMeta::from(&paste))?;
            let new_recency_key = reverse_timestamp_key(paste.updated_at);
            updates.push((
                paste_id,
                old_recency_key,
                new_recency_key,
                encoded_paste,
                encoded_meta,
            ));
        }

        for (paste_id, old_recency_key, new_recency_key, encoded_paste, encoded_meta) in updates {
            pastes.insert(paste_id.as_str(), encoded_paste.as_slice())?;
            metas.insert(paste_id.as_str(), encoded_meta.as_slice())?;
            let _ = updated.remove((old_recency_key, paste_id.as_str()))?;
            updated.insert((new_recency_key, paste_id.as_str()), ())?;
        }

        for folder_id in &delete_order {
            let _ = folders.remove(folder_id.as_str())?;
            let _ = deleting.remove(folder_id.as_str())?;
        }
    }

    write_txn.commit()?;
    Ok(delete_order)
}

fn reconcile_folder_parent_invariants_locked(
    db: &Database,
    folders: &[Folder],
) -> Result<(), AppError> {
    let folder_ids: HashSet<&str> = folders.iter().map(|folder| folder.id.as_str()).collect();
    let mut clear_parent_ids = HashSet::new();

    for folder in folders {
        let Some(parent_id) = folder.parent_id.as_deref() else {
            continue;
        };
        let missing_parent = !folder_ids.contains(parent_id);
        let self_parent = folder.id == parent_id;
        let cyclic_parent = introduces_cycle(folders, folder.id.as_str(), parent_id);
        if missing_parent || self_parent || cyclic_parent {
            clear_parent_ids.insert(folder.id.clone());
        }
    }

    if clear_parent_ids.is_empty() {
        return Ok(());
    }

    for folder in folders {
        if !clear_parent_ids.contains(folder.id.as_str()) {
            continue;
        }
        let _ = db
            .folders
            .update(folder.id.as_str(), folder.name.clone(), Some(String::new()))?;
    }

    Ok(())
}

/// Reconcile folder invariants from canonical paste rows.
///
/// # Returns
/// `Ok(())` when parent references, orphan folder references, and exact counts
/// are repaired.
///
/// # Errors
/// Returns storage and serialization errors when reconciliation cannot complete.
pub fn reconcile_folder_invariants(db: &Database) -> Result<(), AppError> {
    let folder_guard = TransactionOps::acquire_folder_txn_guard(db)?;
    let initial_folders = db.folders.list()?;
    reconcile_folder_parent_invariants_locked(db, &initial_folders)?;

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
            let _ = TransactionOps::move_paste_between_folders_locked(
                db,
                &folder_guard,
                paste_id,
                None,
                update,
            )?;
        }
    }

    for folder in db.folders.list()? {
        let count = exact_counts.get(folder.id.as_str()).copied().unwrap_or(0);
        db.folders.set_count(folder.id.as_str(), count)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db::tables::PASTES, db::TransactionOps, models::paste::Paste};
    use std::collections::HashSet;
    use std::sync::{Arc, Barrier};
    use std::thread;

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
        let (db, _dir) = crate::test_support::setup_temp_db();

        let root = Folder::with_parent("root".to_string(), None);
        let child = Folder::with_parent("child".to_string(), Some(root.id.clone()));
        db.folders.create(&root).expect("create root");
        db.folders.create(&child).expect("create child");

        let mut paste = Paste::new("content".to_string(), "name".to_string());
        paste.folder_id = Some(child.id.clone());
        TransactionOps::create_paste_with_folder(&db, &paste, &child.id).expect("create paste");

        let deleted = delete_folder_tree_and_migrate(&db, &root.id).expect("delete tree");
        assert_eq!(deleted.last(), Some(&root.id));

        let moved = db.pastes.get(&paste.id).expect("get").expect("exists");
        assert_eq!(moved.folder_id, None);
    }

    #[test]
    fn delete_tree_guarded_rejects_locked_descendant() {
        let (db, _dir) = crate::test_support::setup_temp_db();

        let root = Folder::with_parent("root".to_string(), None);
        db.folders.create(&root).expect("create root");

        let mut paste = Paste::new("content".to_string(), "name".to_string());
        paste.folder_id = Some(root.id.clone());
        let paste_id = paste.id.clone();
        let guarded_paste_id = paste_id.clone();
        TransactionOps::create_paste_with_folder(&db, &paste, &root.id).expect("create paste");

        let locked_ids = HashSet::from([paste.id.clone()]);
        let err = delete_folder_tree_and_migrate_guarded(&db, &root.id, |affected_paste_ids| {
            assert_eq!(
                affected_paste_ids,
                std::slice::from_ref(&guarded_paste_id),
                "guard should receive the exact affected paste ids"
            );
            if let Some(locked_id) = affected_paste_ids
                .iter()
                .find(|id| locked_ids.contains(id.as_str()))
            {
                return Err::<(), AppError>(AppError::Locked(format!(
                    "Folder delete would migrate locked paste '{}'; close it first.",
                    locked_id
                )));
            }
            Ok(())
        })
        .expect_err("locked descendant should block delete");
        assert!(matches!(err, AppError::Locked(_)));

        let current = db
            .pastes
            .get(&paste_id)
            .expect("lookup")
            .expect("paste should still exist");
        assert_eq!(current.folder_id.as_deref(), Some(root.id.as_str()));
        assert!(
            db.folders.get(&root.id).expect("folder lookup").is_some(),
            "folder should remain when guarded delete is rejected"
        );
    }

    #[test]
    fn delete_tree_rolls_back_if_migration_scan_fails() {
        let (db, _dir) = crate::test_support::setup_temp_db();

        let root = Folder::with_parent("root".to_string(), None);
        db.folders.create(&root).expect("create root");

        let mut paste = Paste::new("content".to_string(), "name".to_string());
        paste.folder_id = Some(root.id.clone());
        let paste_id = paste.id.clone();
        TransactionOps::create_paste_with_folder(&db, &paste, &root.id).expect("create paste");

        let write_txn = db.db.begin_write().expect("begin write");
        {
            let mut pastes = write_txn.open_table(PASTES).expect("open pastes");
            pastes
                .insert("corrupt-row", b"not-a-paste".as_slice())
                .expect("insert corrupt row");
        }
        write_txn.commit().expect("commit");

        let err = delete_folder_tree_and_migrate(&db, &root.id)
            .expect_err("corrupt migration scan should abort delete");
        assert!(matches!(err, AppError::Serialization(_)));

        let current = db
            .pastes
            .get(&paste_id)
            .expect("lookup")
            .expect("paste should still exist");
        assert_eq!(current.folder_id.as_deref(), Some(root.id.as_str()));
        assert!(
            db.folders.get(&root.id).expect("folder lookup").is_some(),
            "folder should remain when delete transaction aborts"
        );
        assert!(
            !db.folders
                .is_delete_marked(&root.id)
                .expect("marker lookup"),
            "delete markers must not leak on failed atomic delete"
        );
    }

    #[test]
    fn ensure_folder_assignable_rejects_missing_and_marked_folders() {
        let (db, _dir) = crate::test_support::setup_temp_db();

        assert!(matches!(
            ensure_folder_assignable(&db, "missing-folder"),
            Err(AppError::NotFound)
        ));

        let folder = Folder::new("root".to_string());
        let folder_id = folder.id.clone();
        db.folders.create(&folder).expect("create folder");
        db.folders
            .mark_deleting(std::slice::from_ref(&folder_id))
            .expect("mark deleting");
        assert!(matches!(
            ensure_folder_assignable(&db, folder_id.as_str()),
            Err(AppError::BadRequest(_))
        ));
    }

    #[test]
    fn delete_folder_tree_and_concurrent_move_preserve_no_orphan_and_counts() {
        let (db, _dir) = crate::test_support::setup_temp_db();

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
