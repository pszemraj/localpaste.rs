//! Database integration tests.

#[cfg(test)]
mod db_tests {
    use super::super::*;
    use chrono::Duration;
    use crate::error::AppError;
    use crate::models::{folder::*, paste::*};
    use std::sync::{Arc, Barrier};
    use std::thread;
    use tempfile::TempDir;

    fn setup_test_db() -> (Database, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let db = Database::new(db_path.to_str().unwrap()).unwrap();
        (db, temp_dir)
    }

    #[test]
    fn test_create_database() {
        let (db, _temp) = setup_test_db();
        assert!(db.flush().is_ok());
    }

    #[test]
    fn test_paste_create_and_get() {
        let (db, _temp) = setup_test_db();

        let paste = Paste::new("Test content".to_string(), "test-paste".to_string());
        let paste_id = paste.id.clone();

        // Create
        assert!(db.pastes.create(&paste).is_ok());

        // Get
        let retrieved = db.pastes.get(&paste_id).unwrap();
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.content, "Test content");
        assert_eq!(retrieved.id, paste_id);
    }

    #[test]
    fn test_paste_update() {
        let (db, _temp) = setup_test_db();

        let paste = Paste::new("Original".to_string(), "test".to_string());
        let paste_id = paste.id.clone();
        db.pastes.create(&paste).unwrap();

        // Update
        let update = UpdatePasteRequest {
            content: Some("Updated".to_string()),
            name: Some("updated-name".to_string()),
            language: None,
            language_is_manual: None,
            folder_id: None,
            tags: None,
        };

        let _updated = db.pastes.update(&paste_id, update).unwrap();
        assert!(_updated.is_some(), "Update should return Some");

        // Verify the update by retrieving the paste
        let retrieved = db.pastes.get(&paste_id).unwrap().unwrap();
        assert_eq!(retrieved.content, "Updated");
        assert_eq!(retrieved.name, "updated-name");
    }

    #[test]
    fn test_manual_language_switches_to_auto_and_redetects() {
        let (db, _temp) = setup_test_db();

        let mut paste = Paste::new(
            "def main():\n    print('hello')".to_string(),
            "script".to_string(),
        );
        paste.language = Some("rust".to_string());
        paste.language_is_manual = true;
        let paste_id = paste.id.clone();
        db.pastes.create(&paste).unwrap();

        let to_auto = UpdatePasteRequest {
            content: None,
            name: None,
            language: None,
            language_is_manual: Some(false),
            folder_id: None,
            tags: None,
        };
        let updated = db.pastes.update(&paste_id, to_auto).unwrap().unwrap();

        assert!(!updated.language_is_manual);
        assert_eq!(updated.language.as_deref(), Some("python"));
    }

    #[test]
    fn test_content_update_redetects_language_in_auto_mode() {
        let (db, _temp) = setup_test_db();

        let paste = Paste::new(
            "fn main() {\n    let x = 1;\n}".to_string(),
            "script".to_string(),
        );
        let paste_id = paste.id.clone();
        db.pastes.create(&paste).unwrap();

        let update = UpdatePasteRequest {
            content: Some("def main():\n    print('hello')".to_string()),
            name: None,
            language: None,
            language_is_manual: None,
            folder_id: None,
            tags: None,
        };
        let updated = db.pastes.update(&paste_id, update).unwrap().unwrap();

        assert!(!updated.language_is_manual);
        assert_eq!(updated.language.as_deref(), Some("python"));
    }

    #[test]
    fn test_language_update_without_manual_flag_sets_manual_override() {
        let (db, _temp) = setup_test_db();

        let paste = Paste::new(
            "def main():\n    print('hello')".to_string(),
            "script".to_string(),
        );
        let paste_id = paste.id.clone();
        db.pastes.create(&paste).unwrap();

        let set_language = UpdatePasteRequest {
            content: None,
            name: None,
            language: Some("rust".to_string()),
            language_is_manual: None,
            folder_id: None,
            tags: None,
        };
        let updated = db.pastes.update(&paste_id, set_language).unwrap().unwrap();
        assert_eq!(updated.language.as_deref(), Some("rust"));
        assert!(updated.language_is_manual);

        let content_update = UpdatePasteRequest {
            content: Some("def another():\n    print('world')".to_string()),
            name: None,
            language: None,
            language_is_manual: None,
            folder_id: None,
            tags: None,
        };
        let after_content_update = db
            .pastes
            .update(&paste_id, content_update)
            .unwrap()
            .unwrap();
        assert_eq!(after_content_update.language.as_deref(), Some("rust"));
        assert!(after_content_update.language_is_manual);
    }

    #[test]
    fn test_paste_delete() {
        let (db, _temp) = setup_test_db();

        let paste = Paste::new("To delete".to_string(), "test".to_string());
        let paste_id = paste.id.clone();
        db.pastes.create(&paste).unwrap();

        // Delete
        assert!(db.pastes.delete(&paste_id).is_ok());

        // Verify deleted
        let result = db.pastes.get(&paste_id).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_paste_list() {
        let (db, _temp) = setup_test_db();

        // Create multiple pastes
        for i in 0..5 {
            let paste = Paste::new(format!("Content {}", i), format!("paste-{}", i));
            db.pastes.create(&paste).unwrap();
        }

        // List
        let list = db.pastes.list(10, None).unwrap();
        assert_eq!(list.len(), 5);
    }

    #[test]
    fn test_paste_list_meta_orders_by_updated_and_honors_limit() {
        let (db, _temp) = setup_test_db();
        let now = chrono::Utc::now();

        let mut older = Paste::new("old".to_string(), "old".to_string());
        older.updated_at = now - Duration::minutes(10);
        let older_id = older.id.clone();

        let mut newer = Paste::new("new".to_string(), "new".to_string());
        newer.updated_at = now;
        let newer_id = newer.id.clone();

        db.pastes.create(&older).unwrap();
        db.pastes.create(&newer).unwrap();

        let metas = db.pastes.list_meta(1, None).unwrap();
        assert_eq!(metas.len(), 1);
        assert_eq!(metas[0].id, newer_id);
        assert_ne!(metas[0].id, older_id);
    }

    #[test]
    fn test_paste_search() {
        let (db, _temp) = setup_test_db();

        let paste1 = Paste::new("Rust is awesome".to_string(), "p1".to_string());
        let paste2 = Paste::new("Python is great".to_string(), "p2".to_string());
        let paste3 = Paste::new("JavaScript rocks".to_string(), "p3".to_string());

        db.pastes.create(&paste1).unwrap();
        db.pastes.create(&paste2).unwrap();
        db.pastes.create(&paste3).unwrap();

        // Search
        let results = db.pastes.search("rust", 10, None, None).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].content.to_lowercase().contains("rust"));
    }

    #[test]
    fn test_paste_search_meta_is_metadata_only() {
        let (db, _temp) = setup_test_db();

        let mut by_name = Paste::new("hello".to_string(), "rust-note".to_string());
        by_name.language = None;
        by_name.tags = vec![];

        let mut by_tag = Paste::new("hello".to_string(), "misc".to_string());
        by_tag.language = None;
        by_tag.tags = vec!["rusty".to_string()];

        let mut content_only = Paste::new("rust appears in content".to_string(), "plain".to_string());
        content_only.language = None;
        content_only.tags = vec![];

        db.pastes.create(&by_name).unwrap();
        db.pastes.create(&by_tag).unwrap();
        db.pastes.create(&content_only).unwrap();

        let results = db.pastes.search_meta("rust", 10, None, None).unwrap();
        let ids: Vec<String> = results.into_iter().map(|m| m.id).collect();
        assert!(ids.contains(&by_name.id));
        assert!(ids.contains(&by_tag.id));
        assert!(!ids.contains(&content_only.id));
    }

    #[test]
    fn test_meta_indexes_stay_consistent_after_update_and_delete() {
        let (db, _temp) = setup_test_db();
        let paste = Paste::new("one".to_string(), "alpha".to_string());
        let paste_id = paste.id.clone();
        db.pastes.create(&paste).unwrap();

        let update = UpdatePasteRequest {
            content: Some("updated body".to_string()),
            name: Some("beta".to_string()),
            language: None,
            language_is_manual: None,
            folder_id: None,
            tags: Some(vec!["tag".to_string()]),
        };
        db.pastes.update(&paste_id, update).unwrap().unwrap();

        let metas = db.pastes.list_meta(10, None).unwrap();
        let updated = metas.into_iter().find(|m| m.id == paste_id).unwrap();
        assert_eq!(updated.name, "beta");
        assert_eq!(updated.content_len, "updated body".len());
        assert_eq!(updated.tags, vec!["tag".to_string()]);

        db.pastes.delete(&paste_id).unwrap();
        let metas_after_delete = db.pastes.list_meta(10, None).unwrap();
        assert!(!metas_after_delete.into_iter().any(|m| m.id == paste_id));
    }

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
        drop(db);

        let reopened = Database::new(&db_path_str).unwrap();
        let metas = reopened.pastes.list_meta(10, None).unwrap();
        assert!(metas.into_iter().any(|m| m.id == paste_id));
    }

    #[test]
    fn test_folder_crud() {
        let (db, _temp) = setup_test_db();

        let folder = Folder::new("Test Folder".to_string());
        let folder_id = folder.id.clone();

        // Create
        assert!(db.folders.create(&folder).is_ok());

        // Get
        let retrieved = db.folders.get(&folder_id).unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "Test Folder");

        // List
        let list = db.folders.list().unwrap();
        assert_eq!(list.len(), 1);

        // Delete
        assert!(db.folders.delete(&folder_id).is_ok());
        assert!(db.folders.get(&folder_id).unwrap().is_none());
    }

    #[test]
    fn test_database_flush() {
        let (db, _temp) = setup_test_db();

        let paste = Paste::new("Test".to_string(), "test".to_string());
        db.pastes.create(&paste).unwrap();

        // Flush should succeed
        assert!(db.flush().is_ok());
    }

    #[test]
    fn test_move_between_folders_rolls_back_counts_when_paste_missing() {
        let (db, _temp) = setup_test_db();

        let old_folder = Folder::new("old-folder".to_string());
        let old_folder_id = old_folder.id.clone();
        db.folders.create(&old_folder).unwrap();

        let new_folder = Folder::new("new-folder".to_string());
        let new_folder_id = new_folder.id.clone();
        db.folders.create(&new_folder).unwrap();

        db.folders.update_count(&old_folder_id, 1).unwrap();
        db.folders.update_count(&new_folder_id, 1).unwrap();

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
        .unwrap();

        assert!(result.is_none(), "missing paste should return None");

        let old_after = db.folders.get(&old_folder_id).unwrap().unwrap();
        let new_after = db.folders.get(&new_folder_id).unwrap().unwrap();
        assert_eq!(
            old_after.paste_count, 1,
            "old folder count should rollback when paste update returns None"
        );
        assert_eq!(
            new_after.paste_count, 1,
            "new folder count should rollback when paste update returns None"
        );
    }

    #[test]
    fn test_delete_uses_folder_from_deleted_record_not_stale_context() {
        let (db, _temp) = setup_test_db();

        let old_folder = Folder::new("old-folder".to_string());
        let old_folder_id = old_folder.id.clone();
        db.folders.create(&old_folder).unwrap();

        let new_folder = Folder::new("new-folder".to_string());
        let new_folder_id = new_folder.id.clone();
        db.folders.create(&new_folder).unwrap();

        let mut paste = Paste::new("content".to_string(), "name".to_string());
        paste.folder_id = Some(old_folder_id.clone());
        let paste_id = paste.id.clone();
        TransactionOps::create_paste_with_folder(&db, &paste, &old_folder_id).unwrap();

        let stale_folder_id = paste.folder_id.clone().unwrap();
        assert_eq!(stale_folder_id, old_folder_id);

        let move_req = UpdatePasteRequest {
            content: None,
            name: None,
            language: None,
            language_is_manual: None,
            folder_id: Some(new_folder_id.clone()),
            tags: None,
        };
        TransactionOps::move_paste_between_folders(
            &db,
            &paste_id,
            Some(new_folder_id.as_str()),
            move_req,
        )
        .unwrap()
        .expect("paste should exist when moving");

        let deleted = TransactionOps::delete_paste_with_folder(&db, &paste_id).unwrap();
        assert!(deleted, "delete should remove existing paste");

        let old_after = db.folders.get(&old_folder_id).unwrap().unwrap();
        let new_after = db.folders.get(&new_folder_id).unwrap().unwrap();
        assert_eq!(
            old_after.paste_count, 0,
            "stale folder should not be decremented twice"
        );
        assert_eq!(
            new_after.paste_count, 0,
            "current folder should be decremented on delete"
        );
    }

    #[test]
    fn test_update_count_returns_not_found_for_missing_folder() {
        let (db, _temp) = setup_test_db();

        let result = db.folders.update_count("missing-folder-id", 1);
        assert!(
            matches!(result, Err(AppError::NotFound)),
            "missing folder should return NotFound"
        );
    }

    #[test]
    fn test_folder_update_count_preserves_corrupt_record_on_error() {
        let (db, _temp) = setup_test_db();
        let tree = db.db.open_tree("folders").unwrap();
        let folder_id = "corrupt-folder-id";
        tree.insert(folder_id.as_bytes(), b"not-a-folder").unwrap();

        let result = db.folders.update_count(folder_id, 1);
        assert!(
            matches!(result, Err(AppError::Serialization(_))),
            "corrupt folder value should surface serialization error"
        );
        assert!(
            tree.get(folder_id.as_bytes()).unwrap().is_some(),
            "corrupt record should not be deleted by failed update_count"
        );
    }

    #[test]
    fn test_paste_update_preserves_corrupt_record_on_error() {
        let (db, _temp) = setup_test_db();
        let tree = db.db.open_tree("pastes").unwrap();
        let paste_id = "corrupt-paste-id";
        tree.insert(paste_id.as_bytes(), b"not-a-paste").unwrap();

        let update = UpdatePasteRequest {
            content: Some("new".to_string()),
            name: None,
            language: None,
            language_is_manual: None,
            folder_id: None,
            tags: None,
        };

        let result = db.pastes.update(paste_id, update);
        assert!(
            matches!(result, Err(AppError::Serialization(_))),
            "corrupt paste value should surface serialization error"
        );
        assert!(
            tree.get(paste_id.as_bytes()).unwrap().is_some(),
            "corrupt record should not be deleted by failed update"
        );
    }

    #[test]
    fn test_concurrent_moves_keep_folder_counts_consistent() {
        let (db, _temp) = setup_test_db();

        let old_folder = Folder::new("old-folder".to_string());
        let old_folder_id = old_folder.id.clone();
        db.folders.create(&old_folder).unwrap();

        let folder_a = Folder::new("folder-a".to_string());
        let folder_a_id = folder_a.id.clone();
        db.folders.create(&folder_a).unwrap();

        let folder_b = Folder::new("folder-b".to_string());
        let folder_b_id = folder_b.id.clone();
        db.folders.create(&folder_b).unwrap();

        let mut paste = Paste::new("concurrent".to_string(), "move".to_string());
        paste.folder_id = Some(old_folder_id.clone());
        let paste_id = paste.id.clone();
        TransactionOps::create_paste_with_folder(&db, &paste, &old_folder_id).unwrap();

        let worker_a = db.share().unwrap();
        let worker_b = db.share().unwrap();
        let barrier = Arc::new(Barrier::new(2));

        let paste_for_a = paste_id.clone();
        let folder_for_a = folder_a_id.clone();
        let barrier_a = barrier.clone();
        let handle_a = thread::spawn(move || {
            barrier_a.wait();
            let update = UpdatePasteRequest {
                content: None,
                name: None,
                language: None,
                language_is_manual: None,
                folder_id: Some(folder_for_a.clone()),
                tags: None,
            };
            TransactionOps::move_paste_between_folders(
                &worker_a,
                &paste_for_a,
                Some(folder_for_a.as_str()),
                update,
            )
            .expect("move to folder-a should not fail");
        });

        let paste_for_b = paste_id.clone();
        let folder_for_b = folder_b_id.clone();
        let barrier_b = barrier;
        let handle_b = thread::spawn(move || {
            barrier_b.wait();
            let update = UpdatePasteRequest {
                content: None,
                name: None,
                language: None,
                language_is_manual: None,
                folder_id: Some(folder_for_b.clone()),
                tags: None,
            };
            TransactionOps::move_paste_between_folders(
                &worker_b,
                &paste_for_b,
                Some(folder_for_b.as_str()),
                update,
            )
            .expect("move to folder-b should not fail");
        });

        handle_a.join().expect("worker-a join");
        handle_b.join().expect("worker-b join");

        let moved = db
            .pastes
            .get(&paste_id)
            .unwrap()
            .expect("paste should exist");
        let old_after = db.folders.get(&old_folder_id).unwrap().unwrap();
        let a_after = db.folders.get(&folder_a_id).unwrap().unwrap();
        let b_after = db.folders.get(&folder_b_id).unwrap().unwrap();

        assert_eq!(
            old_after.paste_count, 0,
            "old folder should be empty after concurrent moves"
        );
        assert_eq!(
            a_after.paste_count + b_after.paste_count,
            1,
            "exactly one destination folder should own the paste"
        );
        match moved.folder_id.as_deref() {
            Some(fid) if fid == folder_a_id.as_str() => {
                assert_eq!(a_after.paste_count, 1);
                assert_eq!(b_after.paste_count, 0);
            }
            Some(fid) if fid == folder_b_id.as_str() => {
                assert_eq!(a_after.paste_count, 0);
                assert_eq!(b_after.paste_count, 1);
            }
            other => panic!("paste moved to unexpected folder: {:?}", other),
        }
    }
}
