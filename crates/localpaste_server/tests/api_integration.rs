//! Integration tests for the LocalPaste HTTP API.

/// Shared real-listener server harness for API integration tests.
pub mod support;

use axum::http::StatusCode;
use localpaste_server::{Config, LockOwnerId};
use serde_json::json;
use support::{
    setup_test_server, test_config_for_db_path, test_server_for_config, TestResponse, TestServer,
};
use tempfile::TempDir;

fn assert_meta_only_shape_header(response: &TestResponse) {
    response.assert_header("x-localpaste-response-shape", "meta-only");
}

#[tokio::test]
async fn test_paste_lifecycle() {
    let (server, _locks) = setup_test_server();

    // Create a paste
    let create_response = server
        .post("/api/paste")
        .json(&json!({
            "content": "Hello, World!",
            "name": "test-paste"
        }))
        .await;

    assert_eq!(create_response.status_code(), StatusCode::OK);
    create_response.assert_header(
        localpaste_core::LOCALPASTE_SERVER_HEADER,
        localpaste_core::LOCALPASTE_SERVER_VALUE,
    );
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
async fn test_malformed_extractors_return_bad_request() {
    let (server, _locks) = setup_test_server();

    for path in [
        "/api/pastes?limit=abc",
        "/api/search?q=needle&case_sensitive=maybe",
        "/api/search",
        "/api/paste/abc/versions/not-a-number",
    ] {
        let response = server.get(path).await;
        assert_eq!(
            response.status_code(),
            StatusCode::BAD_REQUEST,
            "path should reject malformed extractor input: {path}"
        );
    }
}

#[tokio::test]
async fn test_paste_search() {
    let (server, _locks) = setup_test_server();

    // Create multiple pastes. The first query only appears in content so this
    // fails if canonical search regresses to metadata-only matching.
    server
        .post("/api/paste")
        .json(&json!({
            "content": "Rust is awesome",
            "name": "language-note"
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

    let search_response = server.get("/api/search?q=rust").await;

    assert_eq!(search_response.status_code(), StatusCode::OK);
    assert_meta_only_shape_header(&search_response);
    let results: Vec<serde_json::Value> = search_response.json();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["name"], "language-note");
    assert!(results[0].get("content").is_none());

    let case_sensitive_mismatch = server.get("/api/search?q=rust&case_sensitive=true").await;
    assert_eq!(case_sensitive_mismatch.status_code(), StatusCode::OK);
    let case_sensitive_mismatch_results: Vec<serde_json::Value> = case_sensitive_mismatch.json();
    assert!(case_sensitive_mismatch_results.is_empty());

    let case_sensitive_match = server.get("/api/search?q=Rust&case_sensitive=true").await;
    assert_eq!(case_sensitive_match.status_code(), StatusCode::OK);
    let case_sensitive_match_results: Vec<serde_json::Value> = case_sensitive_match.json();
    assert_eq!(case_sensitive_match_results.len(), 1);
    assert_eq!(case_sensitive_match_results[0]["name"], "language-note");
}

#[tokio::test]
async fn test_search_language_filter_is_case_insensitive_and_trimmed() {
    let (server, _locks) = setup_test_server();

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
    let (server, _locks) = setup_test_server();

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
    let (server, _locks) = setup_test_server();

    // Content-only match for canonical search using a low-signal stopword that
    // derived metadata intentionally drops.
    server
        .post("/api/paste")
        .json(&json!({
            "content": "with with with",
            "name": "content-only"
        }))
        .await;

    // Name/tag match for metadata search.
    let tagged_response = server
        .post("/api/paste")
        .json(&json!({
            "content": "plain text",
            "name": "needle-name",
            "tags": ["needle-tag"]
        }))
        .await;
    assert_eq!(tagged_response.status_code(), StatusCode::OK);

    let derived_response = server
        .post("/api/paste")
        .json(&json!({
            "content": "fsdp2 validation failed after cublaslt retry\nfsdp2 validation repeated\n",
            "name": "derived-terms"
        }))
        .await;
    assert_eq!(derived_response.status_code(), StatusCode::OK);

    let list_meta_response = server.get("/api/pastes/meta?limit=10").await;
    assert_eq!(list_meta_response.status_code(), StatusCode::OK);
    let list_meta: Vec<serde_json::Value> = list_meta_response.json();
    assert!(!list_meta.is_empty());
    assert!(list_meta
        .iter()
        .all(|item| item.get("content").is_none() && item.get("content_len").is_some()));

    let full_search_response = server.get("/api/search?q=with").await;
    assert_eq!(full_search_response.status_code(), StatusCode::OK);
    assert_meta_only_shape_header(&full_search_response);
    let full_results: Vec<serde_json::Value> = full_search_response.json();
    assert_eq!(full_results.len(), 1);
    assert_eq!(full_results[0]["name"], "content-only");
    assert!(full_results
        .iter()
        .all(|item| item.get("content").is_none() && item.get("content_len").is_some()));

    let derived_search_response = server.get("/api/search?q=fsdp2%20cublaslt").await;
    assert_eq!(derived_search_response.status_code(), StatusCode::OK);
    let derived_results: Vec<serde_json::Value> = derived_search_response.json();
    assert_eq!(derived_results.len(), 1);
    assert_eq!(derived_results[0]["name"], "derived-terms");
    assert!(derived_results[0].get("content").is_none());

    let meta_search_response = server.get("/api/search/meta?q=needle").await;
    assert_eq!(meta_search_response.status_code(), StatusCode::OK);
    let meta_results: Vec<serde_json::Value> = meta_search_response.json();
    assert_eq!(meta_results.len(), 1);
    assert_eq!(meta_results[0]["name"], "needle-name");
    assert!(meta_results[0].get("content").is_none());
}

#[tokio::test]
async fn test_max_paste_size_enforcement() {
    let (server, _locks) = setup_test_server();

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
        search_case_sensitive: false,
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
async fn test_strict_cors_origin_matrix() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("strict-cors-origins.db");
    let config = test_config_for_db_path(&db_path);
    let (server, _locks) = test_server_for_config(config);
    let port = server.port();
    let wrong_port = if port == u16::MAX { port - 1 } else { port + 1 };
    let allowed_ipv6 = format!("http://[::1]:{port}");
    let allowed_ipv4 = format!("http://127.0.0.2:{port}");
    let wrong_port_origin = format!("http://127.0.0.1:{wrong_port}");
    let cases = [
        (allowed_ipv6, true),
        (allowed_ipv4, true),
        (wrong_port_origin, false),
        ("http://example.com:3000".to_string(), false),
    ];

    for (origin, should_allow) in cases {
        let response = server
            .get("/api/pastes")
            .add_header("origin", origin.as_str())
            .await;
        assert_eq!(response.status_code(), StatusCode::OK);
        if should_allow {
            response.assert_header("access-control-allow-origin", origin.as_str());
        } else {
            assert!(!response.contains_header("access-control-allow-origin"));
        }
    }
}

#[tokio::test]
async fn test_locked_paste_mutation_matrix_rejects_until_all_holders_release() {
    #[derive(Clone, Copy)]
    enum LockedMutationKind {
        Delete,
        Update,
    }

    async fn issue_locked_mutation(
        server: &TestServer,
        kind: LockedMutationKind,
        paste_id: &str,
    ) -> TestResponse {
        match kind {
            LockedMutationKind::Delete => server.delete(&format!("/api/paste/{}", paste_id)).await,
            LockedMutationKind::Update => {
                server
                    .put(&format!("/api/paste/{}", paste_id))
                    .json(&json!({
                        "content": "new body"
                    }))
                    .await
            }
        }
    }

    let cases = [LockedMutationKind::Delete, LockedMutationKind::Update];
    for kind in cases {
        let (server, locks) = setup_test_server();

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
        let owner_a = LockOwnerId::new("owner-a".to_string());

        locks
            .acquire(&paste_id, &owner_a)
            .expect("owner a acquires");

        let locked_response = issue_locked_mutation(&server, kind, &paste_id).await;
        assert_eq!(locked_response.status_code(), StatusCode::LOCKED);

        if matches!(kind, LockedMutationKind::Delete) {
            let owner_b = LockOwnerId::new("owner-b".to_string());
            locks
                .acquire(&paste_id, &owner_b)
                .expect("owner b acquires");
            locks
                .release(&paste_id, &owner_a)
                .expect("owner a releases");
            let still_locked = issue_locked_mutation(&server, kind, &paste_id).await;
            assert_eq!(still_locked.status_code(), StatusCode::LOCKED);
            locks
                .release(&paste_id, &owner_b)
                .expect("owner b releases");
        } else {
            locks
                .release(&paste_id, &owner_a)
                .expect("owner a releases");
        }

        let ok_response = issue_locked_mutation(&server, kind, &paste_id).await;
        assert_eq!(ok_response.status_code(), StatusCode::OK);
        if matches!(kind, LockedMutationKind::Update) {
            let updated: serde_json::Value = ok_response.json();
            assert_eq!(updated["content"], "new body");
        }
    }
}
