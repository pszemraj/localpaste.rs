//! Database integration tests.

#[cfg(test)]
mod db_tests {
    use super::super::*;
    use crate::models::{folder::*, paste::*};
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
}
