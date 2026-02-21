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

#[tokio::test]
async fn test_default_create_then_auto_toggle_then_content_redetect_cycle() {
    let (server, _temp, _locks) = setup_test_server();

    let created = server
        .post("/api/paste")
        .json(&json!({
            "content": "fn main() { println!(\"hello\"); }",
            "name": "default-create"
        }))
        .await;
    assert_eq!(created.status_code(), StatusCode::OK);
    let created_json: serde_json::Value = created.json();
    let paste_id = created_json["id"].as_str().expect("create response id");
    assert_eq!(created_json["language"], "rust");
    assert_eq!(created_json["language_is_manual"], true);

    let switched_auto = server
        .put(&format!("/api/paste/{}", paste_id))
        .json(&json!({
            "language_is_manual": false
        }))
        .await;
    assert_eq!(switched_auto.status_code(), StatusCode::OK);
    let switched_auto_json: serde_json::Value = switched_auto.json();
    assert!(switched_auto_json["language"].is_null());
    assert_eq!(switched_auto_json["language_is_manual"], false);

    let redetected = server
        .put(&format!("/api/paste/{}", paste_id))
        .json(&json!({
            "content": "def main():\n    import sys\n    print('hello')\n"
        }))
        .await;
    assert_eq!(redetected.status_code(), StatusCode::OK);
    let redetected_json: serde_json::Value = redetected.json();
    assert_eq!(redetected_json["language"], "python");
    assert_eq!(redetected_json["language_is_manual"], true);
}

#[tokio::test]
async fn test_metadata_save_does_not_clear_legacy_auto_resolved_language() {
    let (server, _temp, _locks) = setup_test_server();

    let created = server
        .post("/api/paste")
        .json(&json!({
            "content": "fn main() { println!(\"hello\"); }",
            "name": "legacy-auto",
            "language": "rust",
            "language_is_manual": false
        }))
        .await;
    assert_eq!(created.status_code(), StatusCode::OK);
    let created_json: serde_json::Value = created.json();
    let paste_id = created_json["id"].as_str().expect("create response id");
    assert_eq!(created_json["language"], "rust");
    assert_eq!(created_json["language_is_manual"], false);

    let metadata_update = server
        .put(&format!("/api/paste/{}", paste_id))
        .json(&json!({
            "name": "legacy-auto-renamed",
            "language_is_manual": false
        }))
        .await;
    assert_eq!(metadata_update.status_code(), StatusCode::OK);
    let updated_json: serde_json::Value = metadata_update.json();
    assert_eq!(updated_json["language"], "rust");
    assert_eq!(updated_json["language_is_manual"], false);
}
