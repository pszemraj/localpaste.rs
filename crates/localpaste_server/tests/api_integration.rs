//! Integration tests for the LocalPaste HTTP API.

use axum::http::StatusCode;
use axum_test::TestServer;
use localpaste_server::{
    create_app, models::folder::Folder, AppState, Config, Database, LockOwnerId, PasteLockManager,
};
use serde_json::json;
use std::path::Path;
use std::sync::Arc;
use tempfile::TempDir;

const EXPECTED_FOLDER_DEPRECATION_WARNING: &str =
    "299 - \"Folder APIs are deprecated; prefer tags, search, and smart filters\"";

fn assert_folder_deprecation_headers(response: &axum_test::TestResponse) {
    response.assert_header("deprecation", "true");
    response.assert_contains_header("sunset");
    response.assert_header("warning", EXPECTED_FOLDER_DEPRECATION_WARNING);
}

fn assert_meta_only_shape_header(response: &axum_test::TestResponse) {
    response.assert_header("x-localpaste-response-shape", "meta-only");
}

fn test_config_for_db_path(db_path: &Path) -> Config {
    Config {
        port: 0, // Let OS assign port
        db_path: db_path.to_str().unwrap().to_string(),
        max_paste_size: 10_000_000,
        auto_save_interval: 2000,
        auto_backup: false, // Disable auto-backup in tests
    }
}

fn test_server_for_config(config: Config) -> (TestServer, Arc<PasteLockManager>) {
    let db = Database::new(&config.db_path).unwrap();
    let locks = Arc::new(PasteLockManager::default());
    let state = AppState::with_locks(config, db, locks.clone());
    let app = create_app(state, false);
    let server = TestServer::new(app).unwrap();
    (server, locks)
}

async fn setup_test_server() -> (TestServer, TempDir, Arc<PasteLockManager>) {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");

    let config = test_config_for_db_path(&db_path);
    let (server, locks) = test_server_for_config(config);
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
    assert_meta_only_shape_header(&list_response);
    assert_folder_deprecation_headers(&list_response);
    let pastes: Vec<serde_json::Value> = list_response.json();
    assert_eq!(pastes.len(), 1);
    assert_eq!(pastes[0]["folder_id"], folder_id);
    assert!(pastes[0].get("content").is_none());
    assert!(pastes[0].get("content_len").is_some());

    let list_meta_response = server
        .get(&format!("/api/pastes/meta?folder_id={}", folder_id))
        .await;
    assert_eq!(list_meta_response.status_code(), StatusCode::OK);
    assert_folder_deprecation_headers(&list_meta_response);
    let metas: Vec<serde_json::Value> = list_meta_response.json();
    assert_eq!(metas.len(), 1);
    assert_eq!(metas[0]["folder_id"], folder_id);
    assert!(metas[0].get("content").is_none());

    let search_meta_response = server
        .get(&format!(
            "/api/search/meta?q=folder-paste&folder_id={}",
            folder_id
        ))
        .await;
    assert_eq!(search_meta_response.status_code(), StatusCode::OK);
    assert_folder_deprecation_headers(&search_meta_response);
    let search_meta: Vec<serde_json::Value> = search_meta_response.json();
    assert_eq!(search_meta.len(), 1);
    assert_eq!(search_meta[0]["folder_id"], folder_id);
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
    assert_meta_only_shape_header(&search_response);
    let results: Vec<serde_json::Value> = search_response.json();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["name"], "rust-paste");
    assert!(results[0].get("content").is_none());
}

#[tokio::test]
async fn test_search_language_filter_is_case_insensitive_and_trimmed() {
    let (server, _temp, _locks) = setup_test_server().await;

    server
        .post("/api/paste")
        .json(&json!({
            "content": "def run():\n    return 1",
            "name": "python-note",
            "language": "python",
            "language_is_manual": true,
            "tags": ["shared-tag"]
        }))
        .await;

    server
        .post("/api/paste")
        .json(&json!({
            "content": "fn run() -> i32 { 1 }",
            "name": "rust-note",
            "language": "rust",
            "language_is_manual": true,
            "tags": ["shared-tag"]
        }))
        .await;

    let search_response = server
        .get("/api/search?q=shared-tag&language=%20%20PyThOn%20%20")
        .await;
    assert_eq!(search_response.status_code(), StatusCode::OK);
    let search_results: Vec<serde_json::Value> = search_response.json();
    assert_eq!(search_results.len(), 1);
    assert_eq!(search_results[0]["name"], "python-note");

    let search_meta_response = server
        .get("/api/search/meta?q=shared-tag&language=%20PYTHON%20")
        .await;
    assert_eq!(search_meta_response.status_code(), StatusCode::OK);
    let search_meta_results: Vec<serde_json::Value> = search_meta_response.json();
    assert_eq!(search_meta_results.len(), 1);
    assert_eq!(search_meta_results[0]["name"], "python-note");
}

#[tokio::test]
async fn test_search_empty_or_whitespace_query_returns_no_results() {
    let (server, _temp, _locks) = setup_test_server().await;

    server
        .post("/api/paste")
        .json(&json!({
            "content": "hello world",
            "name": "hello-note"
        }))
        .await;

    let empty_search = server.get("/api/search?q=").await;
    assert_eq!(empty_search.status_code(), StatusCode::OK);
    let empty_results: Vec<serde_json::Value> = empty_search.json();
    assert!(empty_results.is_empty());

    let whitespace_search = server.get("/api/search?q=%20%20%20").await;
    assert_eq!(whitespace_search.status_code(), StatusCode::OK);
    let whitespace_results: Vec<serde_json::Value> = whitespace_search.json();
    assert!(whitespace_results.is_empty());

    let whitespace_meta_search = server.get("/api/search/meta?q=%20%20").await;
    assert_eq!(whitespace_meta_search.status_code(), StatusCode::OK);
    let whitespace_meta_results: Vec<serde_json::Value> = whitespace_meta_search.json();
    assert!(whitespace_meta_results.is_empty());
}

#[tokio::test]
async fn test_metadata_endpoints_return_meta_and_preserve_search_semantics() {
    let (server, _temp, _locks) = setup_test_server().await;

    // Content-only match (should be excluded from /api/search/meta).
    server
        .post("/api/paste")
        .json(&json!({
            "content": "needle-in-content only",
            "name": "content-only"
        }))
        .await;

    // Name/tag match (should be included by /api/search/meta).
    let tagged_response = server
        .post("/api/paste")
        .json(&json!({
            "content": "plain text",
            "name": "needle-name",
            "tags": ["needle-tag"]
        }))
        .await;
    assert_eq!(tagged_response.status_code(), StatusCode::OK);

    let list_meta_response = server.get("/api/pastes/meta?limit=10").await;
    assert_eq!(list_meta_response.status_code(), StatusCode::OK);
    let list_meta: Vec<serde_json::Value> = list_meta_response.json();
    assert!(!list_meta.is_empty());
    assert!(list_meta
        .iter()
        .all(|item| item.get("content").is_none() && item.get("content_len").is_some()));

    let full_search_response = server.get("/api/search?q=needle").await;
    assert_eq!(full_search_response.status_code(), StatusCode::OK);
    assert_meta_only_shape_header(&full_search_response);
    let full_results: Vec<serde_json::Value> = full_search_response.json();
    assert_eq!(full_results.len(), 2);
    assert!(full_results
        .iter()
        .all(|item| item.get("content").is_none() && item.get("content_len").is_some()));

    let meta_search_response = server.get("/api/search/meta?q=needle").await;
    assert_eq!(meta_search_response.status_code(), StatusCode::OK);
    let meta_results: Vec<serde_json::Value> = meta_search_response.json();
    assert_eq!(meta_results.len(), 1);
    assert_eq!(meta_results[0]["name"], "needle-name");
    assert!(meta_results[0].get("content").is_none());
}

#[tokio::test]
async fn test_delete_folder_rejects_when_descendant_paste_is_locked() {
    let (server, _temp, locks) = setup_test_server().await;

    let folder_response = server
        .post("/api/folder")
        .json(&json!({
            "name": "Locked folder"
        }))
        .await;
    assert_eq!(folder_response.status_code(), StatusCode::OK);
    let folder: serde_json::Value = folder_response.json();
    let folder_id = folder["id"].as_str().unwrap();

    let paste_response = server
        .post("/api/paste")
        .json(&json!({
            "content": "locked body",
            "name": "locked-in-folder",
            "folder_id": folder_id
        }))
        .await;
    assert_eq!(paste_response.status_code(), StatusCode::OK);
    let paste: serde_json::Value = paste_response.json();
    let paste_id = paste["id"].as_str().unwrap().to_string();
    let owner_a = LockOwnerId::new("folder-owner-a".to_string());
    let owner_b = LockOwnerId::new("folder-owner-b".to_string());

    locks
        .acquire(&paste_id, &owner_a)
        .expect("owner a acquires");
    locks
        .acquire(&paste_id, &owner_b)
        .expect("owner b acquires");

    let delete_locked_response = server.delete(&format!("/api/folder/{}", folder_id)).await;
    assert_eq!(delete_locked_response.status_code(), StatusCode::LOCKED);

    // No migration/deletion should occur while lock is held.
    let still_foldered = server.get(&format!("/api/paste/{}", paste_id)).await;
    assert_eq!(still_foldered.status_code(), StatusCode::OK);
    let still_foldered_json: serde_json::Value = still_foldered.json();
    assert_eq!(still_foldered_json["folder_id"], folder_id);

    locks
        .release(&paste_id, &owner_a)
        .expect("owner a releases");

    // One holder still remains, so delete should continue to be rejected.
    let delete_still_locked = server.delete(&format!("/api/folder/{}", folder_id)).await;
    assert_eq!(delete_still_locked.status_code(), StatusCode::LOCKED);

    locks
        .release(&paste_id, &owner_b)
        .expect("owner b releases");

    let delete_after_unlock = server.delete(&format!("/api/folder/{}", folder_id)).await;
    assert_eq!(delete_after_unlock.status_code(), StatusCode::OK);
    assert_folder_deprecation_headers(&delete_after_unlock);

    let moved = server.get(&format!("/api/paste/{}", paste_id)).await;
    assert_eq!(moved.status_code(), StatusCode::OK);
    let moved_json: serde_json::Value = moved.json();
    assert!(moved_json["folder_id"].is_null());
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

    // Oversized decoded content must be rejected by either middleware (413) or
    // handler validation (400), depending on configured transport headroom.
    assert!(
        matches!(
            response.status_code(),
            StatusCode::BAD_REQUEST | StatusCode::PAYLOAD_TOO_LARGE
        ),
        "expected BAD_REQUEST or PAYLOAD_TOO_LARGE, got {}",
        response.status_code()
    );
}

#[tokio::test]
async fn test_max_paste_size_allows_exact_content_limit_with_json_overhead() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("body-limit-overhead.db");
    let config = Config {
        port: 0,
        db_path: db_path.to_str().unwrap().to_string(),
        max_paste_size: 20_000,
        auto_save_interval: 2000,
        auto_backup: false,
    };
    let (server, _locks) = test_server_for_config(config);

    // Quote-heavy content expands close to 2x in JSON (`\"` per decoded byte).
    let at_limit = "\"".repeat(20_000);
    let at_limit_response = server
        .post("/api/paste")
        .json(&json!({
            "content": at_limit.clone(),
            "name": "at-limit"
        }))
        .await;
    assert_eq!(at_limit_response.status_code(), StatusCode::OK);
    let created: serde_json::Value = at_limit_response.json();
    let paste_id = created["id"].as_str().unwrap();

    let update_at_limit_response = server
        .put(&format!("/api/paste/{}", paste_id))
        .json(&json!({
            "content": at_limit
        }))
        .await;
    assert_eq!(update_at_limit_response.status_code(), StatusCode::OK);

    let above_limit = "\"".repeat(20_001);
    let above_limit_response = server
        .post("/api/paste")
        .json(&json!({
            "content": above_limit.clone(),
            "name": "above-limit"
        }))
        .await;
    assert_eq!(above_limit_response.status_code(), StatusCode::BAD_REQUEST);

    let update_above_limit_response = server
        .put(&format!("/api/paste/{}", paste_id))
        .json(&json!({
            "content": above_limit
        }))
        .await;
    assert_eq!(
        update_above_limit_response.status_code(),
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn test_strict_cors_allows_ipv6_loopback_origin() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("strict-cors-ipv6.db");
    let mut config = test_config_for_db_path(&db_path);
    config.port = 4055;
    let (server, _locks) = test_server_for_config(config);
    let ipv6_origin = "http://[::1]:4055";
    let ipv4_alias_origin = "http://127.0.0.2:4055";
    let mismatched_port_origin = "http://127.0.0.1:9123";

    let ipv6_response = server
        .get("/api/pastes")
        .add_header("origin", ipv6_origin)
        .await;
    assert_eq!(ipv6_response.status_code(), StatusCode::OK);
    ipv6_response.assert_header("access-control-allow-origin", ipv6_origin);

    let ipv4_alias_response = server
        .get("/api/pastes")
        .add_header("origin", ipv4_alias_origin)
        .await;
    assert_eq!(ipv4_alias_response.status_code(), StatusCode::OK);
    ipv4_alias_response.assert_header("access-control-allow-origin", ipv4_alias_origin);

    let mismatched_port_response = server
        .get("/api/pastes")
        .add_header("origin", mismatched_port_origin)
        .await;
    assert_eq!(mismatched_port_response.status_code(), StatusCode::OK);
    assert!(!mismatched_port_response.contains_header("access-control-allow-origin"));
}

#[tokio::test]
async fn test_strict_cors_rejects_non_loopback_origin() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("strict-cors-non-loopback.db");
    let config = test_config_for_db_path(&db_path);
    let (server, _locks) = test_server_for_config(config);

    let response = server
        .get("/api/pastes")
        .add_header("origin", "http://example.com:3000")
        .await;

    assert_eq!(response.status_code(), StatusCode::OK);
    assert!(!response.contains_header("access-control-allow-origin"));
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
async fn test_server_rejects_assignments_to_delete_marked_folder() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("delete-marked.db");
    let config = test_config_for_db_path(&db_path);

    let db = Database::new(&config.db_path).unwrap();
    let state = AppState::new(config, db);
    let setup_state = state.clone();

    let folder = Folder::new("Marked".to_string());
    let folder_id = folder.id.clone();
    setup_state.db.folders.create(&folder).unwrap();
    setup_state
        .db
        .folders
        .mark_deleting(std::slice::from_ref(&folder_id))
        .unwrap();

    let app = create_app(state, false);
    let server = TestServer::new(app).unwrap();

    let create_marked_response = server
        .post("/api/paste")
        .json(&json!({
            "content": "body",
            "name": "marked-create",
            "folder_id": folder_id
        }))
        .await;
    assert_eq!(
        create_marked_response.status_code(),
        StatusCode::BAD_REQUEST
    );

    let seed_response = server
        .post("/api/paste")
        .json(&json!({
            "content": "body",
            "name": "seed"
        }))
        .await;
    assert_eq!(seed_response.status_code(), StatusCode::OK);
    let seed: serde_json::Value = seed_response.json();
    let seed_id = seed["id"].as_str().unwrap();

    let update_marked_response = server
        .put(&format!("/api/paste/{}", seed_id))
        .json(&json!({ "folder_id": folder_id }))
        .await;
    assert_eq!(
        update_marked_response.status_code(),
        StatusCode::BAD_REQUEST
    );

    let after = server.get(&format!("/api/paste/{}", seed_id)).await;
    assert_eq!(after.status_code(), StatusCode::OK);
    let after_json: serde_json::Value = after.json();
    assert!(after_json["folder_id"].is_null());
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

    let all_meta_list_response = server.get("/api/pastes/meta?limit=20").await;
    assert_eq!(all_meta_list_response.status_code(), StatusCode::OK);
    let all_meta_list: Vec<serde_json::Value> = all_meta_list_response.json();

    let whitespace_meta_list_response = server
        .get("/api/pastes/meta?limit=20&folder_id=%20%20%20")
        .await;
    assert_eq!(whitespace_meta_list_response.status_code(), StatusCode::OK);
    let whitespace_meta_list: Vec<serde_json::Value> = whitespace_meta_list_response.json();
    assert_eq!(whitespace_meta_list.len(), all_meta_list.len());

    let all_meta_search_response = server
        .get("/api/search/meta?q=whitespace-audit&limit=20")
        .await;
    assert_eq!(all_meta_search_response.status_code(), StatusCode::OK);
    let all_meta_search: Vec<serde_json::Value> = all_meta_search_response.json();

    let whitespace_meta_search_response = server
        .get("/api/search/meta?q=whitespace-audit&limit=20&folder_id=%20%20%20")
        .await;
    assert_eq!(
        whitespace_meta_search_response.status_code(),
        StatusCode::OK
    );
    let whitespace_meta_search: Vec<serde_json::Value> = whitespace_meta_search_response.json();
    assert_eq!(whitespace_meta_search.len(), all_meta_search.len());
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
    let config = test_config_for_db_path(&db_path);

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
    let owner_a = LockOwnerId::new("paste-owner-a".to_string());
    let owner_b = LockOwnerId::new("paste-owner-b".to_string());

    // Lock it as if two GUI sessions have it open.
    locks
        .acquire(&paste_id, &owner_a)
        .expect("owner a acquires");
    locks
        .acquire(&paste_id, &owner_b)
        .expect("owner b acquires");

    // Attempt delete through API
    let delete_response = server.delete(&format!("/api/paste/{}", paste_id)).await;
    assert_eq!(delete_response.status_code(), StatusCode::LOCKED);

    // Releasing one holder should keep it locked.
    locks
        .release(&paste_id, &owner_a)
        .expect("owner a releases");
    let delete_response = server.delete(&format!("/api/paste/{}", paste_id)).await;
    assert_eq!(delete_response.status_code(), StatusCode::LOCKED);

    // Unlock final holder and delete should work.
    locks
        .release(&paste_id, &owner_b)
        .expect("owner b releases");
    let delete_response = server.delete(&format!("/api/paste/{}", paste_id)).await;
    assert_eq!(delete_response.status_code(), StatusCode::OK);
}

#[tokio::test]
async fn test_update_locked_paste_rejected() {
    let (server, _temp, locks) = setup_test_server().await;

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
    let owner = LockOwnerId::new("update-owner".to_string());

    locks.acquire(&paste_id, &owner).expect("owner acquires");

    let update_response = server
        .put(&format!("/api/paste/{}", paste_id))
        .json(&json!({
            "content": "new body"
        }))
        .await;
    assert_eq!(update_response.status_code(), StatusCode::LOCKED);

    locks.release(&paste_id, &owner).expect("owner releases");

    let update_response = server
        .put(&format!("/api/paste/{}", paste_id))
        .json(&json!({
            "content": "new body"
        }))
        .await;
    assert_eq!(update_response.status_code(), StatusCode::OK);
    let updated: serde_json::Value = update_response.json();
    assert_eq!(updated["content"], "new body");
}
