//! Regression tests for explicit language handling on paste creation.

mod support;

use axum::http::StatusCode;
use serde_json::json;
use support::setup_test_server;

#[tokio::test]
async fn test_create_paste_respects_explicit_language_even_when_content_differs() {
    let (server, _temp, _locks) = setup_test_server();

    let create_response = server
        .post("/api/paste")
        .json(&json!({
            "content": "fn main() { println!(\"hello\"); }",
            "name": "manual-language",
            "language": "python",
            "language_is_manual": true
        }))
        .await;

    assert_eq!(create_response.status_code(), StatusCode::OK);
    let paste: serde_json::Value = create_response.json();
    assert_eq!(paste["language"], "python");
    assert_eq!(paste["language_is_manual"], true);
}

#[tokio::test]
async fn test_create_paste_with_explicit_auto_mode_starts_unresolved() {
    let (server, _temp, _locks) = setup_test_server();

    let create_response = server
        .post("/api/paste")
        .json(&json!({
            "content": "fn main() { println!(\"hello\"); }",
            "name": "auto-language",
            "language_is_manual": false
        }))
        .await;

    assert_eq!(create_response.status_code(), StatusCode::OK);
    let paste: serde_json::Value = create_response.json();
    assert!(paste["language"].is_null());
    assert_eq!(paste["language_is_manual"], false);
}
