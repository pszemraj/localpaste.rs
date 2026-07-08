//! Regression tests for explicit language handling on paste creation.

/// Shared real-listener server harness for manual-language tests.
pub mod support;

use axum::http::StatusCode;
use serde_json::{json, Value};
use support::{setup_test_server, TestServer};

async fn create_paste(server: &TestServer, request: Value) -> Value {
    let response = server.post("/api/paste").json(&request).await;
    assert_eq!(response.status_code(), StatusCode::OK);
    response.json()
}

async fn update_paste(server: &TestServer, paste_id: &str, request: Value) -> Value {
    let response = server
        .put(&format!("/api/paste/{}", paste_id))
        .json(&request)
        .await;
    assert_eq!(response.status_code(), StatusCode::OK);
    response.json()
}

fn assert_language_mode(paste: &Value, expected_language: Option<&str>, expected_manual: bool) {
    assert_eq!(paste["language"].as_str(), expected_language);
    assert_eq!(paste["language_is_manual"], expected_manual);
}

#[tokio::test]
async fn test_create_paste_manual_language_mode_matrix() {
    struct Case {
        request: Value,
        expected_language: Option<&'static str>,
        expected_manual: bool,
    }

    let cases = [
        Case {
            request: json!({
                "content": "fn main() { println!(\"hello\"); }",
                "name": "manual-language",
                "language": "python",
                "language_is_manual": true
            }),
            expected_language: Some("python"),
            expected_manual: true,
        },
        Case {
            request: json!({
                "content": "fn main() { println!(\"hello\"); }",
                "name": "auto-language",
                "language_is_manual": false
            }),
            expected_language: None,
            expected_manual: false,
        },
    ];

    for case in cases {
        let (server, _locks) = setup_test_server();

        let paste = create_paste(&server, case.request).await;
        assert_language_mode(&paste, case.expected_language, case.expected_manual);
    }
}

#[tokio::test]
async fn test_auto_language_update_transitions_cover_redetect_and_legacy_preservation() {
    {
        let (server, _locks) = setup_test_server();

        let created_json = create_paste(
            &server,
            json!({
                "content": "fn main() { println!(\"hello\"); }",
                "name": "default-create"
            }),
        )
        .await;
        let paste_id = created_json["id"].as_str().expect("create response id");
        assert_language_mode(&created_json, Some("rust"), true);

        let switched_auto_json = update_paste(
            &server,
            paste_id,
            json!({
                "language_is_manual": false
            }),
        )
        .await;
        assert_language_mode(&switched_auto_json, None, false);

        let redetected_json = update_paste(
            &server,
            paste_id,
            json!({
                "content": "def main():\n    import sys\n    print('hello')\n"
            }),
        )
        .await;
        assert_language_mode(&redetected_json, Some("python"), true);
    }

    {
        let (server, _locks) = setup_test_server();

        let created_json = create_paste(
            &server,
            json!({
                "content": "fn main() { println!(\"hello\"); }",
                "name": "legacy-auto",
                "language": "rust",
                "language_is_manual": false
            }),
        )
        .await;
        let paste_id = created_json["id"].as_str().expect("create response id");
        assert_language_mode(&created_json, Some("rust"), false);

        let updated_json = update_paste(
            &server,
            paste_id,
            json!({
                "name": "legacy-auto-renamed",
                "language_is_manual": false
            }),
        )
        .await;
        assert_language_mode(&updated_json, Some("rust"), false);
    }
}
