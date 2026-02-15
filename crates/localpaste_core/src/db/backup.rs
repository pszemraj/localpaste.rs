//! Backup and restore helpers for redb databases.

use super::tables::{
    FOLDERS, FOLDERS_DELETING, PASTES, PASTES_BY_UPDATED, PASTES_META, REDB_FILE_NAME,
};
use super::time_util::unix_timestamp_seconds;
use crate::error::AppError;
use redb::{ReadableDatabase, ReadableTable};
use std::path::PathBuf;
use std::time::SystemTime;

/// Backup manager for a database path.
pub struct BackupManager {
    db_path: PathBuf,
    db_file_path: PathBuf,
}

impl BackupManager {
    /// Create a backup manager for the database path.
    ///
    /// # Returns
    /// A new [`BackupManager`] bound to `db_path`.
    pub fn new(db_path: &str) -> Self {
        let db_path = PathBuf::from(db_path);
        let db_file_path = db_path.join(REDB_FILE_NAME);
        Self {
            db_path,
            db_file_path,
        }
    }

    /// Create a backup by snapshotting all known tables into a new redb file.
    ///
    /// This avoids direct file-copy behavior on open databases, which can fail
    /// on platforms with strict file locking (notably Windows).
    /// Snapshot consistency comes from a source read transaction; copied rows are
    /// committed into the destination backup file via a destination write transaction.
    ///
    /// # Returns
    /// The created backup file path, or an empty string when no database file exists.
    ///
    /// # Errors
    /// Returns an error when transaction start or filesystem copy operations fail.
    pub fn create_backup(&self, db: &redb::Database) -> Result<String, AppError> {
        if !self.db_file_path.exists() {
            return Ok(String::new());
        }

        let timestamp = unix_timestamp_seconds(SystemTime::now())?;
        let backup_path = self.next_backup_path(timestamp);

        let source_read = db.begin_read()?;
        let backup_db = redb::Database::create(&backup_path)?;
        let backup_write = backup_db.begin_write()?;
        Self::copy_bytes_table(&source_read, &backup_write, PASTES)?;
        Self::copy_bytes_table(&source_read, &backup_write, PASTES_META)?;
        Self::copy_bytes_table(&source_read, &backup_write, FOLDERS)?;
        Self::copy_unit_table(&source_read, &backup_write, FOLDERS_DELETING)?;
        Self::copy_updated_index_table(&source_read, &backup_write)?;
        backup_write.commit()?;

        tracing::info!("Created database backup at: {:?}", backup_path);
        Ok(backup_path.to_string_lossy().to_string())
    }

    fn next_backup_path(&self, timestamp: u64) -> PathBuf {
        let mut candidate = self
            .db_path
            .with_extension(format!("backup.{}.redb", timestamp));
        let mut suffix = 1usize;
        while candidate.exists() {
            candidate = self
                .db_path
                .with_extension(format!("backup.{}.{}.redb", timestamp, suffix));
            suffix += 1;
        }
        candidate
    }

    fn copy_bytes_table(
        source: &redb::ReadTransaction,
        destination: &redb::WriteTransaction,
        table: redb::TableDefinition<&str, &[u8]>,
    ) -> Result<(), AppError> {
        let source_table = match source.open_table(table) {
            Ok(table) => table,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(()),
            Err(err) => return Err(err.into()),
        };
        let mut destination_table = destination.open_table(table)?;

        for row in source_table.iter()? {
            let (key, value) = row?;
            let key_owned = key.value().to_string();
            let value_owned = value.value().to_vec();
            destination_table.insert(key_owned.as_str(), value_owned.as_slice())?;
        }

        Ok(())
    }

    fn copy_unit_table(
        source: &redb::ReadTransaction,
        destination: &redb::WriteTransaction,
        table: redb::TableDefinition<&str, ()>,
    ) -> Result<(), AppError> {
        let source_table = match source.open_table(table) {
            Ok(table) => table,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(()),
            Err(err) => return Err(err.into()),
        };
        let mut destination_table = destination.open_table(table)?;

        for row in source_table.iter()? {
            let (key, _) = row?;
            let key_owned = key.value().to_string();
            destination_table.insert(key_owned.as_str(), ())?;
        }

        Ok(())
    }

    fn copy_updated_index_table(
        source: &redb::ReadTransaction,
        destination: &redb::WriteTransaction,
    ) -> Result<(), AppError> {
        let source_table = match source.open_table(PASTES_BY_UPDATED) {
            Ok(table) => table,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(()),
            Err(err) => return Err(err.into()),
        };
        let mut destination_table = destination.open_table(PASTES_BY_UPDATED)?;

        for row in source_table.iter()? {
            let (key, _) = row?;
            let (reverse_millis, paste_id) = key.value();
            let paste_id_owned = paste_id.to_string();
            destination_table.insert((reverse_millis, paste_id_owned.as_str()), ())?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{unix_timestamp_seconds, BackupManager};
    use crate::db::tables::{PASTES, PASTES_META};
    use crate::error::AppError;
    use crate::models::paste::Paste;
    use crate::Database;
    use redb::ReadableDatabase;
    use std::time::{Duration, UNIX_EPOCH};
    use tempfile::TempDir;

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

    #[test]
    fn create_backup_writes_snapshot_file_for_open_database() {
        let temp_dir = TempDir::new().expect("temp dir");
        let db_path = temp_dir.path().join("db");
        let db_path_str = db_path.to_str().expect("db path");

        let db = Database::new(db_path_str).expect("open db");
        let paste = Paste::new("backup-body".to_string(), "backup-name".to_string());
        db.pastes.create(&paste).expect("create paste");

        let manager = BackupManager::new(db_path_str);
        let backup_path = manager
            .create_backup(db.db.as_ref())
            .expect("create backup path");
        assert!(!backup_path.is_empty(), "backup path should be returned");
        assert!(
            std::path::Path::new(&backup_path).exists(),
            "backup file should exist"
        );

        let backup_db = redb::Database::create(&backup_path).expect("open backup");
        let read_txn = backup_db.begin_read().expect("begin read");
        let pastes = read_txn.open_table(PASTES).expect("open pastes");
        let metas = read_txn.open_table(PASTES_META).expect("open metas");
        assert!(
            pastes
                .get(paste.id.as_str())
                .expect("paste lookup")
                .is_some(),
            "backup must include canonical paste rows"
        );
        assert!(
            metas.get(paste.id.as_str()).expect("meta lookup").is_some(),
            "backup must include metadata rows"
        );
    }
}
