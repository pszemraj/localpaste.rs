//! Utilities for handling sled lock files safely.

use crate::error::AppError;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Lock file manager for handling database locks gracefully
pub struct LockManager {
    lock_path: PathBuf,
}

impl LockManager {
    /// Create a lock manager for a database path.
    ///
    /// # Returns
    /// A new [`LockManager`] instance.
    pub fn new(db_path: &str) -> Self {
        let lock_path = PathBuf::from(format!("{}.lock", db_path));
        Self { lock_path }
    }

    /// Force unlock (use with caution!).
    ///
    /// # Returns
    /// `Ok(())` if the lock file is removed or not present.
    ///
    /// # Errors
    /// Returns an error if removal fails.
    pub fn force_unlock(&self) -> Result<(), AppError> {
        if self.lock_path.exists() {
            tracing::warn!("Force removing lock file: {:?}", self.lock_path);
            fs::remove_file(&self.lock_path).map_err(|e| {
                AppError::DatabaseError(format!("Failed to force remove lock: {}", e))
            })?;
        }
        Ok(())
    }

    /// Create a backup of the database before potentially destructive operations.
    ///
    /// # Returns
    /// The backup path, or an empty string if the database path does not exist.
    ///
    /// # Errors
    /// Returns an error if the backup copy fails.
    ///
    /// # Panics
    /// Panics if the system clock is before `UNIX_EPOCH`.
    pub fn backup_database(db_path: &str) -> Result<String, AppError> {
        let db_path = Path::new(db_path);
        if !db_path.exists() {
            return Ok(String::new());
        }

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let backup_path = db_path.with_extension(format!("backup.{}", timestamp));

        // Copy the entire database directory
        if db_path.is_dir() {
            copy_dir_recursive(db_path, &backup_path)?;
        } else {
            fs::copy(db_path, &backup_path).map_err(|e| {
                AppError::DatabaseError(format!("Failed to backup database: {}", e))
            })?;
        }

        tracing::debug!("Created database backup at: {:?}", backup_path);
        Ok(backup_path.to_string_lossy().to_string())
    }
}

/// Recursively copy a directory
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), AppError> {
    fs::create_dir_all(dst).map_err(|e| {
        AppError::DatabaseError(format!("Failed to create backup directory: {}", e))
    })?;

    for entry in fs::read_dir(src)
        .map_err(|e| AppError::DatabaseError(format!("Failed to read directory: {}", e)))?
    {
        let entry = entry.map_err(|e| {
            AppError::DatabaseError(format!("Failed to read directory entry: {}", e))
        })?;

        let path = entry.path();
        let file_name = entry.file_name();
        let dst_path = dst.join(&file_name);

        if path.is_dir() {
            copy_dir_recursive(&path, &dst_path)?;
        } else {
            fs::copy(&path, &dst_path).map_err(|e| {
                AppError::DatabaseError(format!("Failed to copy file {:?}: {}", path, e))
            })?;
        }
    }

    Ok(())
}
