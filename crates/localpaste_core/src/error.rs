//! Application error types for core storage and domain logic.
use thiserror::Error;

/// Top-level application error type.
#[derive(Error, Debug)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(#[from] redb::Error),

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

impl From<redb::DatabaseError> for AppError {
    fn from(value: redb::DatabaseError) -> Self {
        Self::Database(value.into())
    }
}

impl From<redb::TransactionError> for AppError {
    fn from(value: redb::TransactionError) -> Self {
        Self::Database(value.into())
    }
}

impl From<redb::TableError> for AppError {
    fn from(value: redb::TableError) -> Self {
        Self::Database(value.into())
    }
}

impl From<redb::StorageError> for AppError {
    fn from(value: redb::StorageError) -> Self {
        Self::Database(value.into())
    }
}

impl From<redb::CommitError> for AppError {
    fn from(value: redb::CommitError) -> Self {
        Self::Database(value.into())
    }
}
