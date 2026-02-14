//! Backup and restore helpers for redb databases.

use super::tables::REDB_FILE_NAME;
use super::time_util::unix_timestamp_seconds;
use crate::error::AppError;
use std::path::PathBuf;
use std::time::SystemTime;

/// Backup manager for a database path.
pub struct BackupManager {
    db_path: PathBuf,
    db_file_path: PathBuf,
}

impl BackupManager {
    /// Create a backup manager for the database path.
    pub fn new(db_path: &str) -> Self {
        let db_path = PathBuf::from(db_path);
        let db_file_path = db_path.join(REDB_FILE_NAME);
        Self {
            db_path,
            db_file_path,
        }
    }

    /// Create a backup by copying `data.redb` while holding a write transaction.
    ///
    /// Holding a write transaction blocks concurrent writers, guaranteeing a
    /// consistent on-disk snapshot for the copy operation.
    pub fn create_backup(&self, db: &redb::Database) -> Result<String, AppError> {
        if !self.db_file_path.exists() {
            return Ok(String::new());
        }

        let timestamp = unix_timestamp_seconds(SystemTime::now())?;
        let backup_path = self.db_path.with_extension(format!("backup.{}.redb", timestamp));

        let write_txn = db.begin_write()?;
        std::fs::copy(&self.db_file_path, &backup_path).map_err(|err| {
            AppError::StorageMessage(format!(
                "Failed to copy '{}' to '{}': {}",
                self.db_file_path.display(),
                backup_path.display(),
                err
            ))
        })?;
        drop(write_txn);

        tracing::info!("Created database backup at: {:?}", backup_path);
        Ok(backup_path.to_string_lossy().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::unix_timestamp_seconds;
    use crate::error::AppError;
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn backup_timestamp_reports_error_for_pre_epoch_clock() {
        let pre_epoch = UNIX_EPOCH - Duration::from_secs(1);
        let err = unix_timestamp_seconds(pre_epoch).expect_err("pre-epoch time should fail");
        match err {
            AppError::StorageMessage(message) => {
                assert!(
                    message.contains("Failed to compute backup timestamp"),
                    "unexpected error: {}",
                    message
                );
            }
            other => panic!("unexpected error variant: {:?}", other),
        }
    }
}
