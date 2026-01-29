//! HTTP error wrapper for mapping core errors into API responses.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
pub use localpaste_core::AppError;
use serde_json::json;

/// HTTP-facing error type for Axum handlers.
#[derive(Debug)]
pub struct HttpError(pub AppError);

impl From<AppError> for HttpError {
    fn from(err: AppError) -> Self {
        Self(err)
    }
}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        let (status, error_message) = match &self.0 {
            AppError::NotFound => (StatusCode::NOT_FOUND, "Not found"),
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.as_str()),
            AppError::DatabaseError(msg) => {
                tracing::error!("Database error: {}", msg);
                (StatusCode::INTERNAL_SERVER_ERROR, "Database error")
            }
            other => {
                tracing::error!("Internal error: {:?}", other);
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
            }
        };

        let body = Json(json!({ "error": error_message }));
        (status, body).into_response()
    }
}
