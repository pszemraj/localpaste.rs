use localpaste::{create_app, AppState, Config, Database};
use axum::http::StatusCode;
use axum_test::TestServer;
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
    let app = create_app(state);
    
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
    let get_response = server
        .get(&format!("/api/paste/{}", paste_id))
        .await;
    
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
    let delete_response = server
        .delete(&format!("/api/paste/{}", paste_id))
        .await;
    
    assert_eq!(delete_response.status_code(), StatusCode::OK);
    
    // Verify it's deleted
    let get_deleted = server
        .get(&format!("/api/paste/{}", paste_id))
        .await;
    
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
    let list_response = server
        .get("/api/folders")
        .await;
    
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
    
    // Delete folder
    let delete_response = server
        .delete(&format!("/api/folder/{}", folder_id))
        .await;
    
    assert_eq!(delete_response.status_code(), StatusCode::OK);
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
    let search_response = server
        .get("/api/search?q=rust")
        .await;
    
    assert_eq!(search_response.status_code(), StatusCode::OK);
    let results: Vec<serde_json::Value> = search_response.json();
    assert_eq!(results.len(), 1);
    assert!(results[0]["content"].as_str().unwrap().to_lowercase().contains("rust"));
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
    server
        .delete(&format!("/api/folder/{}", folder_id))
        .await;
    
    // Check paste still exists but has no folder
    let get_paste = server
        .get(&format!("/api/paste/{}", paste_id))
        .await;
    
    assert_eq!(get_paste.status_code(), StatusCode::OK);
    let migrated_paste: serde_json::Value = get_paste.json();
    assert_eq!(migrated_paste["folder_id"], serde_json::Value::Null);
}