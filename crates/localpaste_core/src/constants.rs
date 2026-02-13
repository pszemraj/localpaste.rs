//! Shared constants used across LocalPaste crates.

/// Default API port for LocalPaste.
pub const DEFAULT_PORT: u16 = 38411;

/// Default maximum paste size accepted by the API layer.
pub const DEFAULT_MAX_PASTE_SIZE: usize = 10 * 1024 * 1024;

/// Default autosave interval in milliseconds.
pub const DEFAULT_AUTO_SAVE_INTERVAL_MS: u64 = 2_000;

/// Default list and search limits used by GUI list pagination.
pub const DEFAULT_LIST_PASTES_LIMIT: usize = 512;
/// Default upper bound for sidebar search result sets.
pub const DEFAULT_SEARCH_PASTES_LIMIT: usize = 512;

/// Default base URL for CLI/API clients.
pub const DEFAULT_CLI_SERVER_URL: &str = "http://localhost:38411";

/// Lock file names and patterns used by sled recovery.
pub const DB_LOCK_FILE_NAME: &str = "db.lock";
/// Alternate lock filename used by some sled layouts.
pub const DB_TREE_LOCK_FILE_NAME: &str = "tree.lock";
/// Legacy lock suffix used when scanning for stale lock artifacts.
pub const DB_LOCK_EXTENSION: &str = "lock";
