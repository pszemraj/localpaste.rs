//! Shared folder tree operations used by API handlers and GUI backend workers.

use crate::{
    models::{folder::Folder, paste::UpdatePasteRequest},
    AppError, Database,
};
use std::collections::{HashMap, HashSet};

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
    let folders = db.folders.list()?;
    if !folders.iter().any(|f| f.id == root_id) {
        return Err(AppError::NotFound);
    }

    let delete_order = folder_delete_order(&folders, root_id);
    for folder_id in &delete_order {
        migrate_folder_pastes_to_unfiled(db, folder_id)?;
        db.folders.delete(folder_id)?;
    }

    Ok(delete_order)
}

fn migrate_folder_pastes_to_unfiled(db: &Database, folder_id: &str) -> Result<(), AppError> {
    loop {
        let metas = db.pastes.list_meta(100, Some(folder_id.to_string()))?;
        if metas.is_empty() {
            break;
        }
        for meta in metas {
            let update = UpdatePasteRequest {
                content: None,
                name: None,
                language: None,
                language_is_manual: None,
                folder_id: Some(String::new()), // normalized to None in PasteDb::update
                tags: None,
            };
            db.pastes.update(&meta.id, update)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::paste::Paste;
    use tempfile::TempDir;

    fn setup_db() -> (Database, TempDir) {
        let dir = TempDir::new().expect("temp dir");
        let db_path = dir.path().join("db");
        let db = Database::new(db_path.to_str().expect("db path")).expect("db");
        (db, dir)
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
}
