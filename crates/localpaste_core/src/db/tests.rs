//! Database integration tests.

#[cfg(test)]
mod db_tests {
    use super::super::*;
    use crate::error::AppError;
    use crate::models::{folder::*, paste::*};
    use chrono::Duration;
    use std::sync::{Arc, Barrier, Mutex, OnceLock};
    use std::thread;
    use tempfile::TempDir;

    fn setup_test_db() -> (Database, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let db = Database::new(db_path.to_str().unwrap()).unwrap();
        (db, temp_dir)
    }

    fn transaction_failpoint_test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct FailpointGuard;

    impl Drop for FailpointGuard {
        fn drop(&mut self) {
            set_transaction_failpoint(None);
        }
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
    fn test_paste_search_limit_keeps_best_ranked_matches() {
        let (db, _temp) = setup_test_db();

        let strongest = Paste::new(
            "no query term in body".to_string(),
            "needle-name".to_string(),
        );
        let medium = Paste::new(
            "contains needle in content".to_string(),
            "plain-a".to_string(),
        );
        let weak = Paste::new("also needle in content".to_string(), "plain-b".to_string());

        db.pastes.create(&strongest).unwrap();
        db.pastes.create(&medium).unwrap();
        db.pastes.create(&weak).unwrap();

        let results = db.pastes.search("needle", 1, None, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].id, strongest.id,
            "name/tag hits must outrank content-only hits under top-k limiting"
        );
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

        let mut content_only =
            Paste::new("rust appears in content".to_string(), "plain".to_string());
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
    fn test_paste_search_language_filter_is_case_insensitive_and_trimmed() {
        let (db, _temp) = setup_test_db();

        let mut python = Paste::new("def run():\n    pass".to_string(), "py".to_string());
        python.language = Some("python".to_string());
        python.language_is_manual = true;

        let mut rust = Paste::new("fn run() {}".to_string(), "rs".to_string());
        rust.language = Some("rust".to_string());
        rust.language_is_manual = true;

        db.pastes.create(&python).unwrap();
        db.pastes.create(&rust).unwrap();

        let results = db
            .pastes
            .search("run", 10, None, Some("  PyThOn  ".to_string()))
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, python.id);
    }

    #[test]
    fn test_paste_search_meta_language_filter_is_case_insensitive_and_trimmed() {
        let (db, _temp) = setup_test_db();

        let mut python = Paste::new("hello".to_string(), "python-note".to_string());
        python.language = Some("python".to_string());
        python.language_is_manual = true;
        python.tags = vec!["tips".to_string()];

        let mut rust = Paste::new("hello".to_string(), "rust-note".to_string());
        rust.language = Some("rust".to_string());
        rust.language_is_manual = true;
        rust.tags = vec!["tips".to_string()];

        db.pastes.create(&python).unwrap();
        db.pastes.create(&rust).unwrap();

        let results = db
            .pastes
            .search_meta("tips", 10, None, Some(" PYTHON ".to_string()))
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, python.id);
    }

    #[test]
    fn test_paste_search_ignores_empty_or_whitespace_queries() {
        let (db, _temp) = setup_test_db();

        let paste = Paste::new("hello world".to_string(), "note".to_string());
        db.pastes.create(&paste).unwrap();

        let empty = db.pastes.search("", 10, None, None).unwrap();
        assert!(empty.is_empty());

        let whitespace = db.pastes.search("   ", 10, None, None).unwrap();
        assert!(whitespace.is_empty());

        let meta_empty = db.pastes.search_meta("", 10, None, None).unwrap();
        assert!(meta_empty.is_empty());

        let meta_whitespace = db.pastes.search_meta("   ", 10, None, None).unwrap();
        assert!(meta_whitespace.is_empty());
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

        struct EnvGuard {
            key: &'static str,
            previous: Option<String>,
        }
        impl Drop for EnvGuard {
            fn drop(&mut self) {
                if let Some(value) = self.previous.take() {
                    std::env::set_var(self.key, value);
                } else {
                    std::env::remove_var(self.key);
                }
            }
        }

        let guard = EnvGuard {
            key: "LOCALPASTE_REINDEX",
            previous: std::env::var("LOCALPASTE_REINDEX").ok(),
        };
        std::env::set_var(guard.key, "1");

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
    fn test_move_between_folders_injected_error_rolls_back_reservation_and_preserves_state() {
        let _lock = transaction_failpoint_test_lock()
            .lock()
            .expect("transaction failpoint lock");
        let _guard = FailpointGuard;
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

        let update = UpdatePasteRequest {
            content: None,
            name: None,
            language: None,
            language_is_manual: None,
            folder_id: Some(new_folder_id.clone()),
            tags: None,
        };

        set_transaction_failpoint(Some(TransactionFailpoint::MoveAfterDestinationReserveOnce));
        let result = TransactionOps::move_paste_between_folders(
            &db,
            &paste_id,
            Some(new_folder_id.as_str()),
            update,
        );
        set_transaction_failpoint(None);

        assert!(
            matches!(result, Err(AppError::DatabaseError(message)) if message.contains("Injected transaction failpoint")),
            "failpoint should force a deterministic move error"
        );

        let unchanged = db
            .pastes
            .get(&paste_id)
            .expect("paste lookup")
            .expect("paste exists");
        assert_eq!(
            unchanged.folder_id.as_deref(),
            Some(old_folder_id.as_str()),
            "failed move should preserve canonical folder assignment"
        );

        let old_after = db.folders.get(&old_folder_id).unwrap().unwrap();
        let new_after = db.folders.get(&new_folder_id).unwrap().unwrap();
        assert_eq!(
            old_after.paste_count, 1,
            "old folder count should remain unchanged after failed move"
        );
        assert_eq!(
            new_after.paste_count, 0,
            "destination reservation must rollback on move error"
        );
    }

    #[test]
    fn test_move_between_folders_rejects_destination_deleted_after_reservation() {
        let _lock = transaction_failpoint_test_lock()
            .lock()
            .expect("transaction failpoint lock");
        let _guard = FailpointGuard;
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

        let update = UpdatePasteRequest {
            content: None,
            name: None,
            language: None,
            language_is_manual: None,
            folder_id: Some(new_folder_id.clone()),
            tags: None,
        };

        set_transaction_failpoint(Some(
            TransactionFailpoint::MoveDeleteDestinationAfterReserveOnce,
        ));
        let result = TransactionOps::move_paste_between_folders(
            &db,
            &paste_id,
            Some(new_folder_id.as_str()),
            update,
        );
        set_transaction_failpoint(None);

        assert!(
            matches!(result, Err(AppError::NotFound)),
            "destination deletion during move must reject the move"
        );

        let paste_after = db
            .pastes
            .get(&paste_id)
            .expect("paste lookup")
            .expect("paste exists");
        assert_eq!(
            paste_after.folder_id.as_deref(),
            Some(old_folder_id.as_str()),
            "failed move must preserve canonical folder assignment"
        );

        let old_after = db.folders.get(&old_folder_id).unwrap().unwrap();
        assert_eq!(old_after.paste_count, 1);
        assert!(
            db.folders.get(&new_folder_id).unwrap().is_none(),
            "injected race removes destination folder"
        );
    }

    #[test]
    fn test_create_with_folder_injected_error_rolls_back_reservation_and_leaves_no_paste() {
        let _lock = transaction_failpoint_test_lock()
            .lock()
            .expect("transaction failpoint lock");
        let _guard = FailpointGuard;
        let (db, _temp) = setup_test_db();

        let folder = Folder::new("target-folder".to_string());
        let folder_id = folder.id.clone();
        db.folders.create(&folder).unwrap();

        let mut paste = Paste::new("content".to_string(), "name".to_string());
        paste.folder_id = Some(folder_id.clone());
        let paste_id = paste.id.clone();

        set_transaction_failpoint(Some(
            TransactionFailpoint::CreateAfterDestinationReserveOnce,
        ));
        let result = TransactionOps::create_paste_with_folder(&db, &paste, &folder_id);
        set_transaction_failpoint(None);

        assert!(
            matches!(result, Err(AppError::DatabaseError(message)) if message.contains("CreateAfterDestinationReserveOnce")),
            "injected create failpoint should surface error"
        );
        assert!(
            db.pastes.get(&paste_id).unwrap().is_none(),
            "create error must not leave canonical paste row"
        );
        let folder_after = db.folders.get(&folder_id).unwrap().unwrap();
        assert_eq!(
            folder_after.paste_count, 0,
            "destination reservation must rollback on create error"
        );
    }

    #[test]
    fn test_create_with_folder_rejects_destination_deleted_after_reservation_without_orphan() {
        let _lock = transaction_failpoint_test_lock()
            .lock()
            .expect("transaction failpoint lock");
        let _guard = FailpointGuard;
        let (db, _temp) = setup_test_db();

        let folder = Folder::new("target-folder".to_string());
        let folder_id = folder.id.clone();
        db.folders.create(&folder).unwrap();

        let mut paste = Paste::new("content".to_string(), "name".to_string());
        paste.folder_id = Some(folder_id.clone());
        let paste_id = paste.id.clone();

        set_transaction_failpoint(Some(
            TransactionFailpoint::CreateDeleteDestinationAfterReserveOnce,
        ));
        let result = TransactionOps::create_paste_with_folder(&db, &paste, &folder_id);
        set_transaction_failpoint(None);

        assert!(
            matches!(result, Err(AppError::NotFound)),
            "deleted destination must reject create"
        );
        assert!(
            db.pastes.get(&paste_id).unwrap().is_none(),
            "failed create must not leave an orphan paste assignment"
        );
        assert!(
            db.folders.get(&folder_id).unwrap().is_none(),
            "injected race removes destination folder"
        );
    }

    #[test]
    fn test_create_with_folder_destination_deleted_after_create_rolls_back_canonical_insert() {
        let _lock = transaction_failpoint_test_lock()
            .lock()
            .expect("transaction failpoint lock");
        let _guard = FailpointGuard;
        let (db, _temp) = setup_test_db();

        let folder = Folder::new("target-folder".to_string());
        let folder_id = folder.id.clone();
        db.folders.create(&folder).unwrap();

        let mut paste = Paste::new("content".to_string(), "name".to_string());
        paste.folder_id = Some(folder_id.clone());
        let paste_id = paste.id.clone();

        set_transaction_failpoint(Some(
            TransactionFailpoint::CreateDeleteDestinationAfterCanonicalCreateOnce,
        ));
        let result = TransactionOps::create_paste_with_folder(&db, &paste, &folder_id);
        set_transaction_failpoint(None);

        assert!(
            matches!(result, Err(AppError::NotFound)),
            "create should fail when destination disappears after canonical insert"
        );
        assert!(
            db.pastes.get(&paste_id).unwrap().is_none(),
            "post-create destination loss must not leave orphan canonical row"
        );
        assert!(
            db.folders.get(&folder_id).unwrap().is_none(),
            "injected race removes destination folder"
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
    fn test_folder_update_preserves_corrupt_record_on_error() {
        let (db, _temp) = setup_test_db();
        let tree = db.db.open_tree("folders").unwrap();
        let folder_id = "corrupt-folder-update-id";
        tree.insert(folder_id.as_bytes(), b"not-a-folder").unwrap();

        let result = db
            .folders
            .update(folder_id, "renamed".to_string(), Some(String::new()));
        assert!(
            matches!(result, Err(AppError::Serialization(_))),
            "corrupt folder value should surface serialization error"
        );
        assert!(
            tree.get(folder_id.as_bytes()).unwrap().is_some(),
            "corrupt record should not be deleted by failed folder update"
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

    #[test]
    fn test_concurrent_move_and_delete_keep_folder_counts_consistent() {
        let (db, _temp) = setup_test_db();

        let old_folder = Folder::new("old-folder".to_string());
        let old_folder_id = old_folder.id.clone();
        db.folders.create(&old_folder).unwrap();

        let new_folder = Folder::new("new-folder".to_string());
        let new_folder_id = new_folder.id.clone();
        db.folders.create(&new_folder).unwrap();

        let mut paste = Paste::new("concurrent".to_string(), "move-delete".to_string());
        paste.folder_id = Some(old_folder_id.clone());
        let paste_id = paste.id.clone();
        TransactionOps::create_paste_with_folder(&db, &paste, &old_folder_id).unwrap();

        let mover_db = db.share().unwrap();
        let deleter_db = db.share().unwrap();
        let barrier = Arc::new(Barrier::new(2));

        let mover_barrier = barrier.clone();
        let mover_paste_id = paste_id.clone();
        let mover_folder_id = new_folder_id.clone();
        let mover = thread::spawn(move || {
            mover_barrier.wait();
            let update = UpdatePasteRequest {
                content: None,
                name: None,
                language: None,
                language_is_manual: None,
                folder_id: Some(mover_folder_id.clone()),
                tags: None,
            };
            TransactionOps::move_paste_between_folders(
                &mover_db,
                &mover_paste_id,
                Some(mover_folder_id.as_str()),
                update,
            )
        });

        let deleter_barrier = barrier;
        let deleter_paste_id = paste_id.clone();
        let deleter = thread::spawn(move || {
            deleter_barrier.wait();
            TransactionOps::delete_paste_with_folder(&deleter_db, &deleter_paste_id)
        });

        let move_result = mover.join().expect("mover join");
        let delete_result = deleter.join().expect("deleter join");
        assert!(
            move_result.is_ok(),
            "move should not error: {:?}",
            move_result
        );
        assert!(
            delete_result.is_ok(),
            "delete should not error: {:?}",
            delete_result
        );

        let old_after = db.folders.get(&old_folder_id).unwrap().unwrap();
        let new_after = db.folders.get(&new_folder_id).unwrap().unwrap();
        let maybe_paste = db.pastes.get(&paste_id).unwrap();

        match maybe_paste {
            Some(paste) => {
                assert_eq!(
                    paste.folder_id.as_deref(),
                    Some(new_folder_id.as_str()),
                    "remaining paste should only exist in destination folder"
                );
                assert_eq!(old_after.paste_count, 0);
                assert_eq!(new_after.paste_count, 1);
            }
            None => {
                assert_eq!(old_after.paste_count, 0);
                assert_eq!(new_after.paste_count, 0);
            }
        }
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
    fn test_equal_length_index_mismatch_does_not_leak_stale_metadata() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let db_path_str = db_path.to_str().unwrap().to_string();

        let db = Database::new(&db_path_str).unwrap();
        let stale = Paste::new("stale body".to_string(), "alpha-stale".to_string());
        let stale_id = stale.id.clone();
        db.pastes.create(&stale).unwrap();

        let fresh = Paste::new("fresh body".to_string(), "beta-fresh".to_string());
        let fresh_id = fresh.id.clone();
        let canonical_tree = db.db.open_tree("pastes").unwrap();
        canonical_tree.remove(stale_id.as_bytes()).unwrap();
        canonical_tree
            .insert(
                fresh_id.as_bytes(),
                bincode::serialize(&fresh).expect("serialize fresh"),
            )
            .unwrap();
        drop(canonical_tree);
        drop(db);

        let reopened = Database::new(&db_path_str).unwrap();
        assert!(
            !reopened
                .pastes
                .needs_reconcile_meta_indexes(false)
                .expect("needs reconcile"),
            "startup marker/length checks currently miss equal-length semantic mismatches"
        );

        let listed = reopened.pastes.list_meta(10, None).expect("list meta");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, fresh_id);
        assert_eq!(listed[0].name, "beta-fresh");

        let stale_search = reopened
            .pastes
            .search_meta("alpha-stale", 10, None, None)
            .expect("search stale");
        assert!(
            stale_search.is_empty(),
            "stale metadata should not leak through search results"
        );

        let fresh_search = reopened
            .pastes
            .search_meta("beta-fresh", 10, None, None)
            .expect("search fresh");
        assert_eq!(fresh_search.len(), 1);
        assert_eq!(fresh_search[0].id, fresh_id);
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
}
