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

    /// Restore from a backup directory.
    ///
    /// # Returns
    /// `Ok(())` when the database has been restored.
    ///
    /// # Errors
    /// Returns an error if deletion or copy fails.
    #[allow(dead_code)]
    pub fn restore_backup(&self, backup_dir: &str) -> Result<(), AppError> {
        // Simple directory copy restoration
        if self.db_path.exists() {
            fs::remove_dir_all(&self.db_path).map_err(|e| {
                AppError::DatabaseError(format!("Failed to remove old database: {}", e))
            })?;
        }

        copy_dir_recursive(Path::new(backup_dir), &self.db_path)?;

        tracing::info!("Restored database from backup: {}", backup_dir);
        Ok(())
    }

    /// Verify database integrity using checksums.
    ///
    /// # Arguments
    /// - `db1`: First database handle.
    /// - `db2`: Second database handle.
    ///
    /// # Returns
    /// `true` if checksums match.
    ///
    /// # Errors
    /// Returns an error if a checksum cannot be computed.
    #[allow(dead_code)]
    pub fn verify_integrity(db1: &sled::Db, db2: &sled::Db) -> Result<bool, AppError> {
        let checksum1 = db1
            .checksum()
            .map_err(|e| AppError::DatabaseError(format!("Failed to compute checksum: {}", e)))?;

        let checksum2 = db2
            .checksum()
            .map_err(|e| AppError::DatabaseError(format!("Failed to compute checksum: {}", e)))?;

        Ok(checksum1 == checksum2)
    }

    /// List available backups.
    ///
    /// # Returns
    /// A sorted list of backup paths.
    ///
    /// # Errors
    /// Returns an error if the backup directory cannot be read.
    #[allow(dead_code)]
    pub fn list_backups(&self) -> Result<Vec<String>, AppError> {
        let parent = self.db_path.parent().unwrap_or(Path::new("."));
        let mut backups = Vec::new();

        for entry in fs::read_dir(parent)
            .map_err(|e| AppError::DatabaseError(format!("Failed to read directory: {}", e)))?
        {
            let entry = entry
                .map_err(|e| AppError::DatabaseError(format!("Failed to read entry: {}", e)))?;

            let path = entry.path();
            if let Some(ext) = path.extension() {
                if ext == "sledexport" {
                    backups.push(path.to_string_lossy().to_string());
                }
            }
        }

        backups.sort();
        Ok(backups)
    }

    /// Clean up old backups (keep last N backups).
    ///
    /// # Returns
    /// `Ok(())` after cleanup completes.
    ///
    /// # Errors
    /// Returns an error if removing backup files fails.
    ///
    /// # Panics
    /// Does not intentionally panic; any panic would indicate a logic bug.
    #[allow(dead_code)]
    pub fn cleanup_old_backups(&self, keep_count: usize) -> Result<(), AppError> {
        let mut backups = self.list_backups()?;

        if backups.len() <= keep_count {
            return Ok(());
        }

        // Sort by timestamp (newest first)
        backups.sort_by(|a, b| b.cmp(a));

        // Remove old backups
        for backup in &backups[keep_count..] {
            fs::remove_file(backup).map_err(|e| {
                AppError::DatabaseError(format!("Failed to remove old backup: {}", e))
            })?;
            tracing::info!("Removed old backup: {}", backup);
        }

        Ok(())
    }
}
