//! Integration tests for the LocalPaste HTTP API.

use axum::http::StatusCode;
use axum_test::TestServer;
use localpaste_server::{
    create_app, models::folder::Folder, AppState, Config, Database, PasteLockManager,
};
use serde_json::json;
use std::sync::Arc;
use tempfile::TempDir;

fn assert_folder_deprecation_headers(response: &axum_test::TestResponse) {
    response.assert_header("deprecation", "true");
    response.assert_contains_header("sunset");
    response.assert_contains_header("warning");
    response.assert_contains_header("link");
}

async fn setup_test_server() -> (TestServer, TempDir, Arc<PasteLockManager>) {
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
    let locks = Arc::new(PasteLockManager::default());
    let state = AppState::with_locks(config, db, locks.clone());
    let app = create_app(state, false);

    let server = TestServer::new(app).unwrap();
    (server, temp_dir, locks)
}

#[tokio::test]
async fn test_paste_lifecycle() {
    let (server, _temp, _locks) = setup_test_server().await;

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
    let (server, _temp, _locks) = setup_test_server().await;

    // Create a folder
    let create_response = server
        .post("/api/folder")
        .json(&json!({
            "name": "Test Folder"
        }))
        .await;

    assert_eq!(create_response.status_code(), StatusCode::OK);
    assert_folder_deprecation_headers(&create_response);
    let folder: serde_json::Value = create_response.json();
    let folder_id = folder["id"].as_str().unwrap();

    // List folders
    let list_response = server.get("/api/folders").await;

    assert_eq!(list_response.status_code(), StatusCode::OK);
    assert_folder_deprecation_headers(&list_response);
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
    assert_folder_deprecation_headers(&update_response);
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
    assert_folder_deprecation_headers(&child_response);
    let child: serde_json::Value = child_response.json();
    let child_id = child["id"].as_str().unwrap();
    assert_eq!(child["parent_id"], folder_id);

    // Verify both folders appear
    let updated_list_response = server.get("/api/folders").await;
    assert_eq!(updated_list_response.status_code(), StatusCode::OK);
    assert_folder_deprecation_headers(&updated_list_response);
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
    assert_folder_deprecation_headers(&delete_response);

    // The nested paste should now be unfiled
    let moved_response = server.get(&format!("/api/paste/{}", child_paste_id)).await;
    assert_eq!(moved_response.status_code(), StatusCode::OK);
    let moved: serde_json::Value = moved_response.json();
    assert!(moved["folder_id"].is_null());

    // No folders should remain
    let remaining_response = server.get("/api/folders").await;
    assert_eq!(remaining_response.status_code(), StatusCode::OK);
    assert_folder_deprecation_headers(&remaining_response);
    let remaining: Vec<serde_json::Value> = remaining_response.json();
    assert!(remaining.is_empty());
}

#[tokio::test]
async fn test_paste_with_folder() {
    let (server, _temp, _locks) = setup_test_server().await;

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
    assert_folder_deprecation_headers(&paste_response);
    let paste: serde_json::Value = paste_response.json();
    assert_eq!(paste["folder_id"], folder_id);

    // List pastes in folder
    let list_response = server
        .get(&format!("/api/pastes?folder_id={}", folder_id))
        .await;

    assert_eq!(list_response.status_code(), StatusCode::OK);
    assert_folder_deprecation_headers(&list_response);
    let pastes: Vec<serde_json::Value> = list_response.json();
    assert_eq!(pastes.len(), 1);
    assert_eq!(pastes[0]["folder_id"], folder_id);
}

#[tokio::test]
async fn test_paste_search() {
    let (server, _temp, _locks) = setup_test_server().await;

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
    let (server, _temp, _locks) = setup_test_server().await;

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
    let (server, _temp, _locks) = setup_test_server().await;

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
async fn test_whitespace_folder_ids_normalize_consistently() {
    let (server, _temp, _locks) = setup_test_server().await;

    // Parent id containing only whitespace should normalize to top-level.
    let top_level_response = server
        .post("/api/folder")
        .json(&json!({
            "name": "Top Level",
            "parent_id": "   "
        }))
        .await;
    assert_eq!(top_level_response.status_code(), StatusCode::OK);
    let top_level: serde_json::Value = top_level_response.json();
    assert!(top_level["parent_id"].is_null());

    let parent_response = server
        .post("/api/folder")
        .json(&json!({
            "name": "Parent"
        }))
        .await;
    assert_eq!(parent_response.status_code(), StatusCode::OK);
    let parent: serde_json::Value = parent_response.json();
    let parent_id = parent["id"].as_str().unwrap();

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

    // Whitespace parent update should explicitly clear parent.
    let clear_parent_response = server
        .put(&format!("/api/folder/{}", child_id))
        .json(&json!({
            "name": "Child",
            "parent_id": "   "
        }))
        .await;
    assert_eq!(clear_parent_response.status_code(), StatusCode::OK);
    let cleared_child: serde_json::Value = clear_parent_response.json();
    assert!(cleared_child["parent_id"].is_null());

    // Whitespace folder id on create should be treated as unfiled.
    let whitespace_create_response = server
        .post("/api/paste")
        .json(&json!({
            "content": "whitespace-audit one",
            "name": "ws-create",
            "folder_id": "   "
        }))
        .await;
    assert_eq!(whitespace_create_response.status_code(), StatusCode::OK);
    let whitespace_created: serde_json::Value = whitespace_create_response.json();
    assert!(whitespace_created["folder_id"].is_null());

    // Assign to a folder, then clear with whitespace update.
    let foldered_paste_response = server
        .post("/api/paste")
        .json(&json!({
            "content": "whitespace-audit two",
            "name": "ws-update",
            "folder_id": parent_id
        }))
        .await;
    assert_eq!(foldered_paste_response.status_code(), StatusCode::OK);
    let foldered_paste: serde_json::Value = foldered_paste_response.json();
    let foldered_paste_id = foldered_paste["id"].as_str().unwrap();

    let whitespace_update_response = server
        .put(&format!("/api/paste/{}", foldered_paste_id))
        .json(&json!({
            "folder_id": "   "
        }))
        .await;
    assert_eq!(whitespace_update_response.status_code(), StatusCode::OK);
    let whitespace_updated: serde_json::Value = whitespace_update_response.json();
    assert!(whitespace_updated["folder_id"].is_null());

    // Whitespace filter should behave like no filter in list/search paths.
    let all_list_response = server.get("/api/pastes?limit=20").await;
    assert_eq!(all_list_response.status_code(), StatusCode::OK);
    let all_list: Vec<serde_json::Value> = all_list_response.json();

    let whitespace_list_response = server.get("/api/pastes?limit=20&folder_id=%20%20%20").await;
    assert_eq!(whitespace_list_response.status_code(), StatusCode::OK);
    let whitespace_list: Vec<serde_json::Value> = whitespace_list_response.json();
    assert_eq!(whitespace_list.len(), all_list.len());

    let all_search_response = server.get("/api/search?q=whitespace-audit&limit=20").await;
    assert_eq!(all_search_response.status_code(), StatusCode::OK);
    let all_search: Vec<serde_json::Value> = all_search_response.json();

    let whitespace_search_response = server
        .get("/api/search?q=whitespace-audit&limit=20&folder_id=%20%20%20")
        .await;
    assert_eq!(whitespace_search_response.status_code(), StatusCode::OK);
    let whitespace_search: Vec<serde_json::Value> = whitespace_search_response.json();
    assert_eq!(whitespace_search.len(), all_search.len());
}

#[tokio::test]
async fn test_folder_deletion_migrates_pastes() {
    let (server, _temp, _locks) = setup_test_server().await;

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
    let (server, _temp, _locks) = setup_test_server().await;

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

#[tokio::test]
async fn test_delete_locked_paste_rejected() {
    let (server, _temp, locks) = setup_test_server().await;

    // Create a paste
    let create_response = server
        .post("/api/paste")
        .json(&json!({
            "content": "Locked content",
            "name": "locked-paste"
        }))
        .await;

    assert_eq!(create_response.status_code(), StatusCode::OK);
    let paste: serde_json::Value = create_response.json();
    let paste_id = paste["id"].as_str().unwrap().to_string();

    // Lock it as if the GUI has it open.
    locks.lock(&paste_id);

    // Attempt delete through API
    let delete_response = server.delete(&format!("/api/paste/{}", paste_id)).await;
    assert_eq!(delete_response.status_code(), StatusCode::LOCKED);

    // Unlock and delete should work
    locks.unlock(&paste_id);
    let delete_response = server.delete(&format!("/api/paste/{}", paste_id)).await;
    assert_eq!(delete_response.status_code(), StatusCode::OK);
}
