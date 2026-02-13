//! Utilities for handling sled lock files safely.

use crate::error::AppError;
use crate::{
    DB_LOCK_EXTENSION,
    DB_LOCK_FILE_NAME,
    DB_TREE_LOCK_FILE_NAME,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Lock file manager for handling database locks gracefully
pub struct LockManager {
    db_path: PathBuf,
    legacy_lock_path: PathBuf,
}

impl LockManager {
    /// Create a lock manager for a database path.
    ///
    /// # Returns
    /// A new [`LockManager`] instance.
    pub fn new(db_path: &str) -> Self {
        Self {
            db_path: PathBuf::from(db_path),
            legacy_lock_path: PathBuf::from(format!("{}.{}", db_path, DB_LOCK_EXTENSION)),
        }
    }

    fn known_lock_paths(&self) -> Result<Vec<PathBuf>, AppError> {
        let mut lock_paths = vec![
            self.db_path.join(DB_LOCK_FILE_NAME),
            self.db_path.join(DB_TREE_LOCK_FILE_NAME),
            self.legacy_lock_path.clone(),
        ];

        if self.db_path.is_dir() {
            for entry in fs::read_dir(&self.db_path).map_err(|err| {
                AppError::DatabaseError(format!(
                    "Failed to inspect database directory for lock files: {}",
                    err
                ))
            })? {
                let entry = entry.map_err(|err| {
                    AppError::DatabaseError(format!(
                        "Failed to inspect database directory entry: {}",
                        err
                    ))
                })?;
                let path = entry.path();
                let is_lock = path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| ext.eq_ignore_ascii_case(DB_LOCK_EXTENSION))
                    .unwrap_or(false);
                if is_lock {
                    lock_paths.push(path);
                }
            }
        }

        lock_paths.sort();
        lock_paths.dedup();
        Ok(lock_paths)
    }

    /// Force unlock (use with caution!).
    ///
    /// # Returns
    /// Number of lock files removed.
    ///
    /// # Errors
    /// Returns an error if lock discovery or file removal fails.
    pub fn force_unlock(&self) -> Result<usize, AppError> {
        let mut removed_count = 0usize;
        for lock_path in self.known_lock_paths()? {
            if !lock_path.exists() {
                continue;
            }
            tracing::warn!("Force removing lock file: {:?}", lock_path);
            fs::remove_file(&lock_path).map_err(|err| {
                AppError::DatabaseError(format!(
                    "Failed to force remove lock {:?}: {}",
                    lock_path, err
                ))
            })?;
            removed_count += 1;
        }
        Ok(removed_count)
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

#[cfg(test)]
mod tests {
    use super::LockManager;
    use crate::error::AppError;
    use crate::{DB_LOCK_EXTENSION, DB_LOCK_FILE_NAME, DB_TREE_LOCK_FILE_NAME};
    use tempfile::TempDir;

    #[test]
    fn force_unlock_removes_known_lock_files() {
        let dir = TempDir::new().expect("temp dir");
        let db_path = dir.path().join("db");
        std::fs::create_dir_all(&db_path).expect("db dir");

        let db_lock = db_path.join(DB_LOCK_FILE_NAME);
        let extra_lock = db_path.join(DB_TREE_LOCK_FILE_NAME);
        let legacy_lock =
            std::path::PathBuf::from(format!("{}.{}", db_path.to_string_lossy(), DB_LOCK_EXTENSION));
        std::fs::write(&db_lock, b"lock").expect("db lock");
        std::fs::write(&extra_lock, b"lock").expect("extra lock");
        std::fs::write(&legacy_lock, b"lock").expect("legacy lock");

        let manager = LockManager::new(&db_path.to_string_lossy());
        let removed = manager.force_unlock().expect("force unlock");

        assert_eq!(removed, 3);
        assert!(!db_lock.exists());
        assert!(!extra_lock.exists());
        assert!(!legacy_lock.exists());
    }

    #[test]
    fn force_unlock_returns_zero_when_no_lock_files_exist() {
        let dir = TempDir::new().expect("temp dir");
        let db_path = dir.path().join("db");
        std::fs::create_dir_all(&db_path).expect("db dir");

        let manager = LockManager::new(&db_path.to_string_lossy());
        let removed = manager.force_unlock().expect("force unlock");
        assert_eq!(removed, 0);
    }

    #[test]
    fn force_unlock_reports_error_for_unremovable_lock_path() {
        let dir = TempDir::new().expect("temp dir");
        let db_path = dir.path().join("db");
        std::fs::create_dir_all(&db_path).expect("db dir");

        let lock_dir = db_path.join(DB_LOCK_FILE_NAME);
        std::fs::create_dir_all(&lock_dir).expect("lock dir");

        let manager = LockManager::new(&db_path.to_string_lossy());
        let err = manager
            .force_unlock()
            .expect_err("directory lock path should fail removal");
        match err {
            AppError::DatabaseError(message) => {
                assert!(
                    message.contains("Failed to force remove lock"),
                    "unexpected error: {}",
                    message
                );
            }
            other => panic!("unexpected error variant: {:?}", other),
        }
    }
}
