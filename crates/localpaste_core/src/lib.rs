//! Core domain library for LocalPaste (config, storage, models).

/// Configuration loading and defaults.
pub mod config;
/// Database access layer and transactions.
pub mod db;
/// Application error types (storage/domain).
pub mod error;
/// Data models for API requests and persistence.
pub mod models;
/// Paste naming helpers.
pub mod naming;

pub use config::Config;
pub use db::Database;
pub use error::AppError;
