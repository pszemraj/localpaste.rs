//! Integration tests for version-history API edge cases.

mod support;

use axum::http::StatusCode;
use localpaste_core::env::{env_lock, EnvGuard};
use serde_json::json;
use support::{setup_test_server, test_config_for_db_path, test_server_for_config};
use tempfile::TempDir;

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn test_duplicate_version_accepts_empty_body_and_uses_generated_name() {
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

    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;

    let update_response = server
        .put(&format!("/api/paste/{}", paste_id))
        .json(&json!({
            "content": "v2"
        }))
        .await;
    assert_eq!(update_response.status_code(), StatusCode::OK);

    let list_versions_response = server
        .get(&format!("/api/paste/{}/versions?limit=20", paste_id))
        .await;
    assert_eq!(list_versions_response.status_code(), StatusCode::OK);
    let versions: Vec<serde_json::Value> = list_versions_response.json();
    let version_id = versions[0]["version_id_ms"].as_u64().unwrap();

    let duplicate_response = server
        .post(&format!(
            "/api/paste/{}/versions/{}/duplicate",
            paste_id, version_id
        ))
        .await;
    assert_eq!(duplicate_response.status_code(), StatusCode::OK);
    let duplicated: serde_json::Value = duplicate_response.json();
    assert_eq!(duplicated["content"], "v1");
    assert_ne!(duplicated["id"], created["id"]);
    assert!(
        !duplicated["name"]
            .as_str()
            .expect("duplicate name")
            .trim()
            .is_empty(),
        "bodyless duplicate should still generate a valid paste name"
    );
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn test_version_retention_cap_is_enforced_through_api_updates() {
    let _env_lock = env_lock().lock().expect("env lock");
    let _interval_guard = EnvGuard::set("LOCALPASTE_VERSION_INTERVAL_SECS", "1");
    let _retention_guard = EnvGuard::set("LOCALPASTE_VERSION_RETENTION_LIMIT", "2");
    let (server, _temp, _locks) = setup_test_server();

    let create_response = server
        .post("/api/paste")
        .json(&json!({
            "content": "v1",
            "name": "retention-api"
        }))
        .await;
    assert_eq!(create_response.status_code(), StatusCode::OK);
    let created: serde_json::Value = create_response.json();
    let paste_id = created["id"].as_str().unwrap().to_string();

    let mut pruned_version_id = None;
    for content in ["v2", "v3", "v4"] {
        tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
        let update_response = server
            .put(&format!("/api/paste/{}", paste_id))
            .json(&json!({ "content": content }))
            .await;
        assert_eq!(update_response.status_code(), StatusCode::OK);

        if content == "v2" {
            let versions_response = server
                .get(&format!("/api/paste/{}/versions?limit=20", paste_id))
                .await;
            assert_eq!(versions_response.status_code(), StatusCode::OK);
            let versions: Vec<serde_json::Value> = versions_response.json();
            pruned_version_id = versions[0]["version_id_ms"].as_u64();
        }
    }

    let versions_response = server
        .get(&format!("/api/paste/{}/versions?limit=20", paste_id))
        .await;
    assert_eq!(versions_response.status_code(), StatusCode::OK);
    let versions: Vec<serde_json::Value> = versions_response.json();
    assert_eq!(versions.len(), 2);

    let newest_snapshot_response = server
        .get(&format!(
            "/api/paste/{}/versions/{}",
            paste_id,
            versions[0]["version_id_ms"].as_u64().unwrap()
        ))
        .await;
    assert_eq!(newest_snapshot_response.status_code(), StatusCode::OK);
    let newest_snapshot: serde_json::Value = newest_snapshot_response.json();
    assert_eq!(newest_snapshot["content"], "v3");

    let older_snapshot_response = server
        .get(&format!(
            "/api/paste/{}/versions/{}",
            paste_id,
            versions[1]["version_id_ms"].as_u64().unwrap()
        ))
        .await;
    assert_eq!(older_snapshot_response.status_code(), StatusCode::OK);
    let older_snapshot: serde_json::Value = older_snapshot_response.json();
    assert_eq!(older_snapshot["content"], "v2");

    let pruned_response = server
        .get(&format!(
            "/api/paste/{}/versions/{}",
            paste_id,
            pruned_version_id.expect("first archived version id")
        ))
        .await;
    assert_eq!(pruned_response.status_code(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_version_mutations_reject_historical_snapshots_exceeding_current_size_limit() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("version-size-limit.db");
    let paste_id: String;
    let version_id: u64;

    {
        let mut config = test_config_for_db_path(&db_path);
        config.max_paste_size = 10;
        let (server, _locks) = test_server_for_config(config);

        let create_response = server
            .post("/api/paste")
            .json(&json!({
                "content": "12345",
                "name": "seed"
            }))
            .await;
        assert_eq!(create_response.status_code(), StatusCode::OK);
        let created: serde_json::Value = create_response.json();
        paste_id = created["id"].as_str().unwrap().to_string();

        let update_response = server
            .put(&format!("/api/paste/{}", paste_id))
            .json(&json!({
                "content": "x"
            }))
            .await;
        assert_eq!(update_response.status_code(), StatusCode::OK);

        let list_versions_response = server
            .get(&format!("/api/paste/{}/versions?limit=20", paste_id))
            .await;
        assert_eq!(list_versions_response.status_code(), StatusCode::OK);
        let versions: Vec<serde_json::Value> = list_versions_response.json();
        version_id = versions[0]["version_id_ms"].as_u64().unwrap();
    }

    {
        let mut config = test_config_for_db_path(&db_path);
        config.max_paste_size = 4;
        let (server, _locks) = test_server_for_config(config);

        let duplicate_response = server
            .post(&format!(
                "/api/paste/{}/versions/{}/duplicate",
                paste_id, version_id
            ))
            .await;
        assert_eq!(duplicate_response.status_code(), StatusCode::BAD_REQUEST);
        let duplicate_body: serde_json::Value = duplicate_response.json();
        assert_eq!(
            duplicate_body["error"].as_str(),
            Some("Paste size exceeds maximum of 4 bytes")
        );

        let reset_response = server
            .post(&format!(
                "/api/paste/{}/versions/{}/reset-hard",
                paste_id, version_id
            ))
            .await;
        assert_eq!(reset_response.status_code(), StatusCode::BAD_REQUEST);
        let reset_body: serde_json::Value = reset_response.json();
        assert_eq!(
            reset_body["error"].as_str(),
            Some("Paste size exceeds maximum of 4 bytes")
        );

        let current_response = server.get(&format!("/api/paste/{}", paste_id)).await;
        assert_eq!(current_response.status_code(), StatusCode::OK);
        let current: serde_json::Value = current_response.json();
        assert_eq!(current["content"], "x");

        let versions_response = server
            .get(&format!("/api/paste/{}/versions?limit=20", paste_id))
            .await;
        assert_eq!(versions_response.status_code(), StatusCode::OK);
        let versions: Vec<serde_json::Value> = versions_response.json();
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0]["version_id_ms"].as_u64(), Some(version_id));

        let list_response = server.get("/api/pastes?limit=20").await;
        assert_eq!(list_response.status_code(), StatusCode::OK);
        let items: Vec<serde_json::Value> = list_response.json();
        assert_eq!(
            items.len(),
            1,
            "failed duplicate must not create an oversized paste row"
        );
    }
}
