//! Owner-lock helpers for process-level writer coordination.

use crate::error::AppError;
use crate::DB_OWNER_LOCK_FILE_NAME;
use fs2::FileExt;
use std::fs::{self, File, OpenOptions};
use std::path::PathBuf;

/// Process-lifetime owner lock guard.
///
/// Keeping this value alive holds an exclusive OS lock on `db.owner.lock`.
pub struct OwnerLockGuard {
    file: File,
    lock_path: PathBuf,
}

impl Drop for OwnerLockGuard {
    fn drop(&mut self) {
        if let Err(err) = self.file.unlock() {
            tracing::warn!(
                "Failed to release owner lock {:?} during drop: {}",
                self.lock_path,
                err
            );
        }
    }
}

/// Return the owner lock file path for a database root.
///
/// # Returns
/// Fully qualified owner-lock path (`<db_path>/db.owner.lock`).
pub fn owner_lock_path(db_path: &str) -> PathBuf {
    PathBuf::from(db_path).join(DB_OWNER_LOCK_FILE_NAME)
}

/// Acquire and hold an exclusive owner lock for the process lifetime.
///
/// # Returns
/// A guard that keeps the owner lock held until dropped.
///
/// # Errors
/// Returns an error when lock file creation/open or lock acquisition fails.
pub fn acquire_owner_lock_for_lifetime(db_path: &str) -> Result<OwnerLockGuard, AppError> {
    let lock_path = owner_lock_path(db_path);
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            AppError::StorageMessage(format!(
                "Failed to prepare owner lock parent '{}': {}",
                parent.display(),
                err
            ))
        })?;
    }

    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|err| {
            AppError::StorageMessage(format!(
                "Failed to open owner lock '{}': {}",
                lock_path.display(),
                err
            ))
        })?;

    match file.try_lock_exclusive() {
        Ok(()) => Ok(OwnerLockGuard { file, lock_path }),
        Err(err) if lock_conflict_error(&err) => Err(AppError::StorageMessage(format!(
            "Database owner lock '{}' is already held by another LocalPaste writer: {}",
            lock_path.display(),
            err
        ))),
        Err(err) => Err(AppError::StorageMessage(format!(
            "Failed to acquire owner lock '{}': {}",
            lock_path.display(),
            err
        ))),
    }
}

fn lock_conflict_error(err: &std::io::Error) -> bool {
    matches!(err.kind(), std::io::ErrorKind::WouldBlock)
        || matches!(err.raw_os_error(), Some(32 | 33))
}

#[cfg(test)]
mod tests {
    use super::{acquire_owner_lock_for_lifetime, owner_lock_path};
    use crate::AppError;
    use crate::DB_OWNER_LOCK_FILE_NAME;
    use tempfile::TempDir;

    #[test]
    fn owner_lock_acquire_reports_conflict_when_already_held() {
        let dir = TempDir::new().expect("temp dir");
        let db_path = dir.path().join("db");
        std::fs::create_dir_all(&db_path).expect("db dir");

        let _guard = acquire_owner_lock_for_lifetime(&db_path.to_string_lossy())
            .expect("acquire owner lock");
        let err = match acquire_owner_lock_for_lifetime(&db_path.to_string_lossy()) {
            Ok(_) => panic!("second lock acquisition should fail"),
            Err(err) => err,
        };
        assert!(
            matches!(err, AppError::StorageMessage(ref message) if message.contains("already held")),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn owner_lock_path_appends_owner_lock_filename() {
        let path = owner_lock_path("some-db");
        assert!(path.ends_with(DB_OWNER_LOCK_FILE_NAME));
    }
}
