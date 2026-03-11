//! Integration tests for compare-oriented LocalPaste HTTP endpoints.

mod support;

use axum::http::StatusCode;
use localpaste_core::env::{env_lock, EnvGuard};
use serde_json::json;
use support::setup_test_server;

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn test_version_and_diff_endpoints_roundtrip() {
    let _env_lock = env_lock().lock().expect("env lock");
    let _interval_guard = EnvGuard::set("LOCALPASTE_PASTE_VERSION_INTERVAL_SECS", "1");
    let (server, _temp, _locks) = setup_test_server();

    let create_response = server
        .post("/api/paste")
        .json(&json!({
            "content": "v1",
            "name": "versioned"
        }))
        .await;
    assert_eq!(create_response.status_code(), StatusCode::OK);
    let created: serde_json::Value = create_response.json();
    let paste_id = created["id"].as_str().unwrap().to_string();

    let list_versions_initial = server
        .get(&format!("/api/paste/{}/versions?limit=20", paste_id))
        .await;
    assert_eq!(list_versions_initial.status_code(), StatusCode::OK);
    let mut versions: Vec<serde_json::Value> = list_versions_initial.json();
    assert_eq!(versions.len(), 0);

    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;

    let update_response = server
        .put(&format!("/api/paste/{}", paste_id))
        .json(&json!({
            "content": "v2"
        }))
        .await;
    assert_eq!(update_response.status_code(), StatusCode::OK);

    let list_versions_after_update = server
        .get(&format!("/api/paste/{}/versions?limit=20", paste_id))
        .await;
    assert_eq!(list_versions_after_update.status_code(), StatusCode::OK);
    versions = list_versions_after_update.json();
    assert!(!versions.is_empty());
    let oldest_version = versions.last().unwrap()["version_id_ms"].as_u64().unwrap();

    let get_oldest = server
        .get(&format!(
            "/api/paste/{}/versions/{}",
            paste_id, oldest_version
        ))
        .await;
    assert_eq!(get_oldest.status_code(), StatusCode::OK);
    let oldest_snapshot: serde_json::Value = get_oldest.json();
    assert_eq!(oldest_snapshot["content"], "v1");

    let diff_response = server
        .post("/api/diff")
        .json(&json!({
            "left": { "paste_id": paste_id.as_str(), "version_id_ms": null },
            "right": { "paste_id": paste_id.as_str(), "version_id_ms": oldest_version }
        }))
        .await;
    assert_eq!(diff_response.status_code(), StatusCode::OK);
    let diff: serde_json::Value = diff_response.json();
    assert_eq!(diff["equal"], false);

    let equal_response = server
        .post("/api/equal")
        .json(&json!({
            "left": { "paste_id": paste_id.as_str(), "version_id_ms": oldest_version },
            "right": { "paste_id": paste_id.as_str(), "version_id_ms": oldest_version }
        }))
        .await;
    assert_eq!(equal_response.status_code(), StatusCode::OK);
    let equal: serde_json::Value = equal_response.json();
    assert_eq!(equal["equal"], true);

    let duplicate_response = server
        .post(&format!(
            "/api/paste/{}/versions/{}/duplicate",
            paste_id, oldest_version
        ))
        .json(&json!({ "name": "duplicated-from-version" }))
        .await;
    assert_eq!(duplicate_response.status_code(), StatusCode::OK);
    let duplicated: serde_json::Value = duplicate_response.json();
    assert_eq!(duplicated["content"], "v1");

    let reset_response = server
        .post(&format!(
            "/api/paste/{}/versions/{}/reset-hard",
            paste_id, oldest_version
        ))
        .await;
    assert_eq!(reset_response.status_code(), StatusCode::OK);
    let reset: serde_json::Value = reset_response.json();
    assert_eq!(reset["content"], "v1");

    let list_versions_after_reset = server
        .get(&format!("/api/paste/{}/versions?limit=20", paste_id))
        .await;
    assert_eq!(list_versions_after_reset.status_code(), StatusCode::OK);
    let versions_after_reset: Vec<serde_json::Value> = list_versions_after_reset.json();
    assert_eq!(versions_after_reset.len(), 0);
}

#[tokio::test]
async fn test_diff_endpoint_rejects_oversized_compare_inputs() {
    let (server, _temp, _locks) = setup_test_server();
    let oversized = "x".repeat((localpaste_core::MAX_DIFF_INPUT_BYTES / 2) + 1);

    let left_response = server
        .post("/api/paste")
        .json(&json!({
            "content": oversized,
            "name": "diff-left"
        }))
        .await;
    assert_eq!(left_response.status_code(), StatusCode::OK);
    let left: serde_json::Value = left_response.json();

    let right_response = server
        .post("/api/paste")
        .json(&json!({
            "content": "y".repeat((localpaste_core::MAX_DIFF_INPUT_BYTES / 2) + 1),
            "name": "diff-right"
        }))
        .await;
    assert_eq!(right_response.status_code(), StatusCode::OK);
    let right: serde_json::Value = right_response.json();

    let diff_response = server
        .post("/api/diff")
        .json(&json!({
            "left": { "paste_id": left["id"].as_str().unwrap(), "version_id_ms": null },
            "right": { "paste_id": right["id"].as_str().unwrap(), "version_id_ms": null }
        }))
        .await;

    assert_eq!(diff_response.status_code(), StatusCode::PAYLOAD_TOO_LARGE);
    let body: serde_json::Value = diff_response.json();
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|message| message.contains("Combined diff input exceeds")),
        "unexpected error payload: {body}"
    );
}

#[tokio::test]
async fn test_equal_endpoint_rejects_oversized_compare_inputs() {
    let (server, _temp, _locks) = setup_test_server();
    let oversized = "x".repeat((localpaste_core::MAX_DIFF_INPUT_BYTES / 2) + 1);

    let left_response = server
        .post("/api/paste")
        .json(&json!({
            "content": oversized,
            "name": "equal-left"
        }))
        .await;
    assert_eq!(left_response.status_code(), StatusCode::OK);
    let left: serde_json::Value = left_response.json();

    let right_response = server
        .post("/api/paste")
        .json(&json!({
            "content": "y".repeat((localpaste_core::MAX_DIFF_INPUT_BYTES / 2) + 1),
            "name": "equal-right"
        }))
        .await;
    assert_eq!(right_response.status_code(), StatusCode::OK);
    let right: serde_json::Value = right_response.json();

    let equal_response = server
        .post("/api/equal")
        .json(&json!({
            "left": { "paste_id": left["id"].as_str().unwrap(), "version_id_ms": null },
            "right": { "paste_id": right["id"].as_str().unwrap(), "version_id_ms": null }
        }))
        .await;

    assert_eq!(equal_response.status_code(), StatusCode::PAYLOAD_TOO_LARGE);
    let body: serde_json::Value = equal_response.json();
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|message| message.contains("Combined diff input exceeds")),
        "unexpected error payload: {body}"
    );
}

#[tokio::test]
async fn test_compare_endpoints_allow_large_identical_refs() {
    let (server, _temp, _locks) = setup_test_server();
    let oversized = "x".repeat((localpaste_core::MAX_DIFF_INPUT_BYTES / 2) + 1);

    let create_response = server
        .post("/api/paste")
        .json(&json!({
            "content": oversized,
            "name": "same-large"
        }))
        .await;
    assert_eq!(create_response.status_code(), StatusCode::OK);
    let paste: serde_json::Value = create_response.json();
    let paste_id = paste["id"].as_str().unwrap();

    let diff_response = server
        .post("/api/diff")
        .json(&json!({
            "left": { "paste_id": paste_id, "version_id_ms": null },
            "right": { "paste_id": paste_id, "version_id_ms": null }
        }))
        .await;
    assert_eq!(diff_response.status_code(), StatusCode::OK);
    let diff: serde_json::Value = diff_response.json();
    assert_eq!(diff["equal"], true);
    assert_eq!(diff["unified"], json!([]));

    let equal_response = server
        .post("/api/equal")
        .json(&json!({
            "left": { "paste_id": paste_id, "version_id_ms": null },
            "right": { "paste_id": paste_id, "version_id_ms": null }
        }))
        .await;
    assert_eq!(equal_response.status_code(), StatusCode::OK);
    let equal: serde_json::Value = equal_response.json();
    assert_eq!(equal["equal"], true);
}
