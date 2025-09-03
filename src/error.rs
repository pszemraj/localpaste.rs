use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(#[from] sled::Error),
    
    #[error("Database error: {0}")]
    DatabaseError(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] bincode::Error),

    #[error("Not found")]
    NotFound,

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Internal server error")]
    #[allow(dead_code)]
    Internal,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, error_message) = match self {
            AppError::NotFound => (StatusCode::NOT_FOUND, "Not found"),
            AppError::BadRequest(ref msg) => (StatusCode::BAD_REQUEST, msg.as_str()),
            AppError::DatabaseError(ref msg) => {
                tracing::error!("Database error: {}", msg);
                (StatusCode::INTERNAL_SERVER_ERROR, "Database error")
            }
            _ => {
                tracing::error!("Internal error: {:?}", self);
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
            }
        };

        let body = Json(json!({ "error": error_message }));
        (status, body).into_response()
    }
}
