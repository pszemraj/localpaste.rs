//! Application error types for core storage and domain logic.
use thiserror::Error;

/// Top-level application error type.
#[derive(Error, Debug)]
pub enum AppError {
    #[error("Database backend error: {0}")]
    Database(#[from] sled::Error),

    #[error("Storage error: {0}")]
    StorageMessage(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] bincode::Error),

    #[error("Not found")]
    NotFound,

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Locked: {0}")]
    Locked(String),

    #[error("Internal server error")]
    Internal,
}
