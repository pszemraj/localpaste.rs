//! Core domain library for LocalPaste (config, storage, models).

/// Configuration loading and defaults.
pub mod config;
/// Shared cross-crate constants.
pub mod constants;
/// Database access layer and transactions.
pub mod db;
/// Language detection adapters and canonicalization.
pub mod detection;
/// Line-based diff helpers and API payload types.
pub mod diff;
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
/// Locally-derived retrieval metadata.
pub mod semantic;
/// Shared helpers used by `localpaste_core` tests.
#[cfg(test)]
pub(crate) mod test_support;
/// Shared string and host normalization helpers.
pub mod text;
/// Shared validation helpers for paste-domain invariants.
pub mod validation;

pub use config::Config;
pub use constants::{
    API_ADDR_FILE_NAME, DB_OWNER_LOCK_FILE_NAME, DEFAULT_AUTO_SAVE_INTERVAL_MS,
    DEFAULT_CLI_SERVER_URL, DEFAULT_LIST_PASTES_LIMIT, DEFAULT_MAX_PASTE_SIZE,
    DEFAULT_PASTE_VERSION_INTERVAL_SECS, DEFAULT_PASTE_VERSION_RETENTION_LIMIT, DEFAULT_PORT,
    DEFAULT_SEARCH_PASTES_LIMIT, LOCALPASTE_SERVER_HEADER, LOCALPASTE_SERVER_VALUE,
    MAX_DIFF_INPUT_BYTES,
};
pub use db::Database;
pub use detection::detect_language;
pub use error::AppError;
