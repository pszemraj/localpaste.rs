//! Backup and restore helpers for sled databases.

use crate::error::AppError;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

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

/// Backup manager using sled's native export/import functionality
pub struct BackupManager {
    db_path: PathBuf,
}

impl BackupManager {
    /// Create a backup manager for the database path.
    ///
    /// # Returns
    /// A new [`BackupManager`] bound to `db_path`.
    pub fn new(db_path: &str) -> Self {
        Self {
            db_path: PathBuf::from(db_path),
        }
    }

    /// Create a backup by copying the database directory.
    ///
    /// Since sled's export/import is for version migrations, we use directory copy.
    ///
    /// # Returns
    /// The backup path, or an empty string if the database path does not exist.
    ///
    /// # Errors
    /// Returns an error if copying files fails.
    ///
    /// # Panics
    /// Panics if the system clock is before `UNIX_EPOCH`.
    pub fn create_backup(&self, _db: &sled::Db) -> Result<String, AppError> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let backup_path = self.db_path.with_extension(format!("backup.{}", timestamp));

        // For sled, the best backup is a copy of the directory after flush
        if self.db_path.exists() {
            copy_dir_recursive(&self.db_path, &backup_path)?;
            tracing::info!("Created database backup at: {:?}", backup_path);
            Ok(backup_path.to_string_lossy().to_string())
        } else {
            Ok(String::new())
        }
    }
}
