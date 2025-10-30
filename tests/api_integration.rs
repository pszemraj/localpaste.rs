use axum::http::StatusCode;
use axum_test::TestServer;
use localpaste::{create_app, models::folder::Folder, AppState, Config, Database};
use serde_json::json;
use tempfile::TempDir;

async fn setup_test_server() -> (TestServer, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");

    let config = Config {
        port: 0, // Let OS assign port
        db_path: db_path.to_str().unwrap().to_string(),
        max_paste_size: 10_000_000,
        auto_save_interval: 2000,
        auto_backup: false, // Disable auto-backup in tests
    };

    let db = Database::new(&config.db_path).unwrap();
    let state = AppState::new(config, db);
    let app = create_app(state, false);

    let server = TestServer::new(app).unwrap();
    (server, temp_dir)
}

#[tokio::test]
async fn test_paste_lifecycle() {
    let (server, _temp) = setup_test_server().await;

    // Create a paste
    let create_response = server
        .post("/api/paste")
        .json(&json!({
            "content": "Hello, World!",
            "name": "test-paste"
        }))
        .await;

    assert_eq!(create_response.status_code(), StatusCode::OK);
    let paste: serde_json::Value = create_response.json();
    let paste_id = paste["id"].as_str().unwrap();

    // Get the paste
    let get_response = server.get(&format!("/api/paste/{}", paste_id)).await;

    assert_eq!(get_response.status_code(), StatusCode::OK);
    let retrieved: serde_json::Value = get_response.json();
    assert_eq!(retrieved["content"], "Hello, World!");
    assert_eq!(retrieved["name"], "test-paste");

    // Update the paste
    let update_response = server
        .put(&format!("/api/paste/{}", paste_id))
        .json(&json!({
            "content": "Updated content",
            "name": "updated-paste"
        }))
        .await;

    assert_eq!(update_response.status_code(), StatusCode::OK);
    let updated: serde_json::Value = update_response.json();
    assert_eq!(updated["content"], "Updated content");
    assert_eq!(updated["name"], "updated-paste");

    // Delete the paste
    let delete_response = server.delete(&format!("/api/paste/{}", paste_id)).await;

    assert_eq!(delete_response.status_code(), StatusCode::OK);

    // Verify it's deleted
    let get_deleted = server.get(&format!("/api/paste/{}", paste_id)).await;

    assert_eq!(get_deleted.status_code(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_folder_lifecycle() {
    let (server, _temp) = setup_test_server().await;

    // Create a folder
    let create_response = server
        .post("/api/folder")
        .json(&json!({
            "name": "Test Folder"
        }))
        .await;

    assert_eq!(create_response.status_code(), StatusCode::OK);
    let folder: serde_json::Value = create_response.json();
    let folder_id = folder["id"].as_str().unwrap();

    // List folders
    let list_response = server.get("/api/folders").await;

    assert_eq!(list_response.status_code(), StatusCode::OK);
    let folders: Vec<serde_json::Value> = list_response.json();
    assert_eq!(folders.len(), 1);
    assert_eq!(folders[0]["name"], "Test Folder");

    // Update folder
    let update_response = server
        .put(&format!("/api/folder/{}", folder_id))
        .json(&json!({
            "name": "Updated Folder"
        }))
        .await;

    assert_eq!(update_response.status_code(), StatusCode::OK);
    let updated: serde_json::Value = update_response.json();
    assert_eq!(updated["name"], "Updated Folder");

    // Create a nested subfolder
    let child_response = server
        .post("/api/folder")
        .json(&json!({
            "name": "Child Folder",
            "parent_id": folder_id
        }))
        .await;

    assert_eq!(child_response.status_code(), StatusCode::OK);
    let child: serde_json::Value = child_response.json();
    let child_id = child["id"].as_str().unwrap();
    assert_eq!(child["parent_id"], folder_id);

    // Verify both folders appear
    let updated_list_response = server.get("/api/folders").await;
    assert_eq!(updated_list_response.status_code(), StatusCode::OK);
    let updated_folders: Vec<serde_json::Value> = updated_list_response.json();
    assert_eq!(updated_folders.len(), 2);

    // Create a paste inside the child folder
    let child_paste_response = server
        .post("/api/paste")
        .json(&json!({
            "content": "Nested content",
            "name": "nested-paste",
            "folder_id": child_id
        }))
        .await;
    assert_eq!(child_paste_response.status_code(), StatusCode::OK);
    let child_paste: serde_json::Value = child_paste_response.json();
    let child_paste_id = child_paste["id"].as_str().unwrap();

    // Delete the parent folder (should cascade)
    let delete_response = server.delete(&format!("/api/folder/{}", folder_id)).await;
    assert_eq!(delete_response.status_code(), StatusCode::OK);

    // The nested paste should now be unfiled
    let moved_response = server.get(&format!("/api/paste/{}", child_paste_id)).await;
    assert_eq!(moved_response.status_code(), StatusCode::OK);
    let moved: serde_json::Value = moved_response.json();
    assert!(moved["folder_id"].is_null());

    // No folders should remain
    let remaining_response = server.get("/api/folders").await;
    assert_eq!(remaining_response.status_code(), StatusCode::OK);
    let remaining: Vec<serde_json::Value> = remaining_response.json();
    assert!(remaining.is_empty());
}

#[tokio::test]
async fn test_paste_with_folder() {
    let (server, _temp) = setup_test_server().await;

    // Create a folder first
    let folder_response = server
        .post("/api/folder")
        .json(&json!({
            "name": "My Folder"
        }))
        .await;

    let folder: serde_json::Value = folder_response.json();
    let folder_id = folder["id"].as_str().unwrap();

    // Create paste in folder
    let paste_response = server
        .post("/api/paste")
        .json(&json!({
            "content": "Content in folder",
            "name": "folder-paste",
            "folder_id": folder_id
        }))
        .await;

    assert_eq!(paste_response.status_code(), StatusCode::OK);
    let paste: serde_json::Value = paste_response.json();
    assert_eq!(paste["folder_id"], folder_id);

    // List pastes in folder
    let list_response = server
        .get(&format!("/api/pastes?folder_id={}", folder_id))
        .await;

    assert_eq!(list_response.status_code(), StatusCode::OK);
    let pastes: Vec<serde_json::Value> = list_response.json();
    assert_eq!(pastes.len(), 1);
    assert_eq!(pastes[0]["folder_id"], folder_id);
}

#[tokio::test]
async fn test_paste_search() {
    let (server, _temp) = setup_test_server().await;

    // Create multiple pastes
    server
        .post("/api/paste")
        .json(&json!({
            "content": "Rust is awesome",
            "name": "rust-paste"
        }))
        .await;

    server
        .post("/api/paste")
        .json(&json!({
            "content": "Python is great",
            "name": "python-paste"
        }))
        .await;

    server
        .post("/api/paste")
        .json(&json!({
            "content": "JavaScript rocks",
            "name": "js-paste"
        }))
        .await;

    // Search for "rust"
    let search_response = server.get("/api/search?q=rust").await;

    assert_eq!(search_response.status_code(), StatusCode::OK);
    let results: Vec<serde_json::Value> = search_response.json();
    assert_eq!(results.len(), 1);
    assert!(results[0]["content"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("rust"));
}

#[tokio::test]
async fn test_max_paste_size_enforcement() {
    let (server, _temp) = setup_test_server().await;

    // Create a very large content string (11MB, exceeding the 10MB limit)
    let large_content = "x".repeat(11_000_000);

    let response = server
        .post("/api/paste")
        .json(&json!({
            "content": large_content,
            "name": "too-large"
        }))
        .await;

    // Should fail with 413 Payload Too Large (or 400 Bad Request)
    // The middleware layer returns 413 when the body limit is exceeded
    assert_eq!(response.status_code(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn test_invalid_folder_association() {
    let (server, _temp) = setup_test_server().await;

    // Try to create paste with non-existent folder
    let response = server
        .post("/api/paste")
        .json(&json!({
            "content": "Test content",
            "name": "test",
            "folder_id": "non-existent-folder-id"
        }))
        .await;

    assert_eq!(response.status_code(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_folder_deletion_migrates_pastes() {
    let (server, _temp) = setup_test_server().await;

    // Create folder
    let folder_response = server
        .post("/api/folder")
        .json(&json!({
            "name": "Temp Folder"
        }))
        .await;

    let folder: serde_json::Value = folder_response.json();
    let folder_id = folder["id"].as_str().unwrap();

    // Create paste in folder
    let paste_response = server
        .post("/api/paste")
        .json(&json!({
            "content": "Will be migrated",
            "name": "migrated-paste",
            "folder_id": folder_id
        }))
        .await;

    let paste: serde_json::Value = paste_response.json();
    let paste_id = paste["id"].as_str().unwrap();

    // Delete folder
    server.delete(&format!("/api/folder/{}", folder_id)).await;

    // Check paste still exists but has no folder
    let get_paste = server.get(&format!("/api/paste/{}", paste_id)).await;

    assert_eq!(get_paste.status_code(), StatusCode::OK);
    let migrated_paste: serde_json::Value = get_paste.json();
    assert_eq!(migrated_paste["folder_id"], serde_json::Value::Null);
}

#[tokio::test]
async fn test_update_folder_rejects_cycle() {
    let (server, _temp) = setup_test_server().await;

    // Create parent folder
    let parent_response = server
        .post("/api/folder")
        .json(&json!({
            "name": "Parent"
        }))
        .await;
    assert_eq!(parent_response.status_code(), StatusCode::OK);
    let parent: serde_json::Value = parent_response.json();
    let parent_id = parent["id"].as_str().unwrap();

    // Create child folder
    let child_response = server
        .post("/api/folder")
        .json(&json!({
            "name": "Child",
            "parent_id": parent_id
        }))
        .await;
    assert_eq!(child_response.status_code(), StatusCode::OK);
    let child: serde_json::Value = child_response.json();
    let child_id = child["id"].as_str().unwrap();

    // Attempt to set the parent folder's parent to its child (would create a cycle)
    let cycle_response = server
        .put(&format!("/api/folder/{}", parent_id))
        .json(&json!({
            "name": "Parent",
            "parent_id": child_id
        }))
        .await;

    assert_eq!(cycle_response.status_code(), StatusCode::BAD_REQUEST);

    // Ensure the parent folder still has no parent
    let folders_response = server.get("/api/folders").await;
    assert_eq!(folders_response.status_code(), StatusCode::OK);
    let folders: Vec<serde_json::Value> = folders_response.json();
    let parent_entry = folders.iter().find(|f| f["id"] == parent_id).unwrap();
    assert!(parent_entry["parent_id"].is_null());
}

#[tokio::test]
async fn test_delete_folder_with_cycle_completes() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("cycle.db");

    let config = Config {
        port: 0,
        db_path: db_path.to_str().unwrap().to_string(),
        max_paste_size: 10_000_000,
        auto_save_interval: 2000,
        auto_backup: false,
    };

    let db = Database::new(&config.db_path).unwrap();
    let state = AppState::new(config, db);
    let setup_state = state.clone();

    let root = Folder::with_parent("Root".to_string(), None);
    let child = Folder::with_parent("Child".to_string(), Some(root.id.clone()));

    setup_state.db.folders.create(&root).unwrap();
    setup_state.db.folders.create(&child).unwrap();
    setup_state
        .db
        .folders
        .update(&root.id, root.name.clone(), Some(child.id.clone()))
        .unwrap();

    let app = create_app(state, false);
    let server = TestServer::new(app).unwrap();

    let delete_response = server.delete(&format!("/api/folder/{}", root.id)).await;
    assert_eq!(delete_response.status_code(), StatusCode::OK);

    let folders_response = server.get("/api/folders").await;
    assert_eq!(folders_response.status_code(), StatusCode::OK);
    let folders: Vec<serde_json::Value> = folders_response.json();
    assert!(folders.is_empty());
}
