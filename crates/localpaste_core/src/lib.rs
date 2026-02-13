//! Core domain library for LocalPaste (config, storage, models).

/// Configuration loading and defaults.
pub mod config;
/// Shared cross-crate constants.
pub mod constants;
/// Database access layer and transactions.
pub mod db;
/// Application error types (storage/domain).
pub mod error;
/// Shared folder tree operations.
pub mod folder_ops;
/// Data models for API requests and persistence.
pub mod models;
/// Paste naming helpers.
pub mod naming;

pub use config::Config;
pub use constants::{
    DB_LOCK_EXTENSION, DB_LOCK_FILE_NAME, DB_TREE_LOCK_FILE_NAME, DEFAULT_AUTO_SAVE_INTERVAL_MS,
    DEFAULT_CLI_SERVER_URL, DEFAULT_LIST_PASTES_LIMIT, DEFAULT_MAX_PASTE_SIZE, DEFAULT_PORT,
    DEFAULT_SEARCH_PASTES_LIMIT,
};
pub use db::Database;
pub use error::AppError;
