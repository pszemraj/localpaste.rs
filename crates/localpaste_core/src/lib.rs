//! Core domain library for LocalPaste (config, storage, models).

/// Configuration loading and defaults.
pub mod config;
/// Shared cross-crate constants.
pub mod constants;
/// Database access layer and transactions.
pub mod db;
/// Language detection adapters and canonicalization.
pub mod detection;
/// Process-global environment mutation helpers.
pub mod env;
/// Application error types (storage/domain).
pub mod error;
/// Shared folder tree operations.
pub mod folder_ops;
/// Data models for API requests and persistence.
pub mod models;
/// Paste naming helpers.
pub mod naming;
#[cfg(test)]
pub(crate) mod test_support;
/// Shared string and host normalization helpers.
pub mod text;

pub use config::Config;
pub use constants::{
    API_ADDR_FILE_NAME, DB_OWNER_LOCK_FILE_NAME, DEFAULT_AUTO_SAVE_INTERVAL_MS,
    DEFAULT_CLI_SERVER_URL, DEFAULT_LIST_PASTES_LIMIT, DEFAULT_MAX_PASTE_SIZE, DEFAULT_PORT,
    DEFAULT_SEARCH_PASTES_LIMIT,
};
pub use db::Database;
pub use detection::detect_language;
pub use error::AppError;
