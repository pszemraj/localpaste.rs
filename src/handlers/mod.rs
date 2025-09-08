pub mod folder;
pub mod paste;

use axum::{http::StatusCode, Json};
use serde_json::Value;
use tracing::{error, info, warn};

/// Handle client-side error/log reporting
pub async fn log_client_error(Json(payload): Json<Value>) -> StatusCode {
    let level = payload["level"].as_str().unwrap_or("error");
    let message = payload["message"].as_str().unwrap_or("Unknown error");
    let stack = payload["stack"].as_str().unwrap_or("");
    let source = payload["source"].as_str().unwrap_or("");
    let url = payload["url"].as_str().unwrap_or("");

    match level {
        "error" => error!(
            "🔴 JS Error: {}\n   URL: {}\n   Source: {}\n   Stack: {}",
            message, url, source, stack
        ),
        "warn" => warn!("🟡 JS Warning: {} ({})", message, url),
        "info" => info!("🔵 JS Info: {} ({})", message, url),
        _ => error!("JS: {}", message),
    }

    StatusCode::NO_CONTENT
}
