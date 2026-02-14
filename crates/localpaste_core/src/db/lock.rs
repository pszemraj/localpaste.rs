//! Utilities for handling sled lock files safely.

use super::{fs_copy::copy_dir_recursive, ProcessProbeResult};
use crate::error::AppError;
use crate::{
    DB_LOCK_EXTENSION, DB_LOCK_FILE_NAME, DB_OWNER_LOCK_FILE_NAME, DB_TREE_LOCK_FILE_NAME,
};
use fs2::FileExt;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

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
/// Fully-qualified owner lock path (`<db_path>/db.owner.lock`).
pub fn owner_lock_path(db_path: &str) -> PathBuf {
    PathBuf::from(db_path).join(DB_OWNER_LOCK_FILE_NAME)
}

/// Acquire and hold an exclusive owner lock for the process lifetime.
///
/// # Returns
/// [`OwnerLockGuard`] that keeps the owner lock held until dropped.
///
/// # Errors
/// Returns [`AppError::DatabaseError`] when the owner lock cannot be acquired.
pub fn acquire_owner_lock_for_lifetime(db_path: &str) -> Result<OwnerLockGuard, AppError> {
    let lock_path = owner_lock_path(db_path);
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            AppError::DatabaseError(format!(
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
            AppError::DatabaseError(format!(
                "Failed to open owner lock '{}': {}",
                lock_path.display(),
                err
            ))
        })?;

    match file.try_lock_exclusive() {
        Ok(()) => Ok(OwnerLockGuard { file, lock_path }),
        Err(err)
            if matches!(
                err.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::PermissionDenied
            ) =>
        {
            Err(AppError::DatabaseError(format!(
                "Database owner lock '{}' is already held by another LocalPaste writer.",
                lock_path.display()
            )))
        }
        Err(err) => Err(AppError::DatabaseError(format!(
            "Failed to acquire owner lock '{}': {}",
            lock_path.display(),
            err
        ))),
    }
}

/// Probe whether another process currently holds the owner lock.
///
/// # Returns
/// - [`ProcessProbeResult::Running`] when the lock is held.
/// - [`ProcessProbeResult::NotRunning`] when the lock can be acquired.
/// - [`ProcessProbeResult::Unknown`] on probe/tooling errors.
///
/// # Errors
/// This probe never returns an error. Uncertainty is represented as
/// [`ProcessProbeResult::Unknown`].
pub fn probe_owner_lock(db_path: &str) -> ProcessProbeResult {
    let lock_path = owner_lock_path(db_path);
    if let Some(parent) = lock_path.parent() {
        if let Err(err) = fs::create_dir_all(parent) {
            tracing::warn!(
                "Owner-lock probe failed creating parent '{}': {}",
                parent.display(),
                err
            );
            return ProcessProbeResult::Unknown;
        }
    }
    let file = match OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
    {
        Ok(file) => file,
        Err(err) => {
            tracing::warn!(
                "Owner-lock probe failed opening '{}': {}",
                lock_path.display(),
                err
            );
            return ProcessProbeResult::Unknown;
        }
    };

    match file.try_lock_exclusive() {
        Ok(()) => {
            if let Err(err) = file.unlock() {
                tracing::warn!(
                    "Owner-lock probe failed releasing '{}': {}",
                    lock_path.display(),
                    err
                );
                ProcessProbeResult::Unknown
            } else {
                ProcessProbeResult::NotRunning
            }
        }
        Err(err)
            if matches!(
                err.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::PermissionDenied
            ) =>
        {
            ProcessProbeResult::Running
        }
        Err(err) => {
            tracing::warn!(
                "Owner-lock probe failed locking '{}': {}",
                lock_path.display(),
                err
            );
            ProcessProbeResult::Unknown
        }
    }
}

fn unix_timestamp_seconds(now: SystemTime) -> Result<u64, AppError> {
    now.duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|err| {
            AppError::DatabaseError(format!(
                "Failed to compute backup timestamp from system clock: {}",
                err
            ))
        })
}

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
                let is_owner_lock = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| name == DB_OWNER_LOCK_FILE_NAME)
                    .unwrap_or(false);
                if is_lock && !is_owner_lock {
                    lock_paths.push(path);
                }
            }
        }

        lock_paths.sort();
        lock_paths.dedup();
        Ok(lock_paths)
    }

    fn ensure_lock_file_is_unlockable(lock_path: &Path) -> Result<(), AppError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .truncate(false)
            .open(lock_path)
            .map_err(|err| {
                AppError::DatabaseError(format!(
                    "Refusing to remove lock '{}': unable to open for lock validation: {}",
                    lock_path.display(),
                    err
                ))
            })?;

        match file.try_lock_exclusive() {
            Ok(()) => {
                file.unlock().map_err(|err| {
                    AppError::DatabaseError(format!(
                        "Refusing to remove lock '{}': failed to release validation lock: {}",
                        lock_path.display(),
                        err
                    ))
                })?;
                Ok(())
            }
            Err(err) if lock_conflict_error(&err) => Err(AppError::DatabaseError(format!(
                "Refusing to remove lock '{}': it appears to be held by another process",
                lock_path.display()
            ))),
            Err(err) => Err(AppError::DatabaseError(format!(
                "Refusing to remove lock '{}': lock validation failed: {}",
                lock_path.display(),
                err
            ))),
        }
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
            Self::ensure_lock_file_is_unlockable(&lock_path)?;
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
    pub fn backup_database(db_path: &str) -> Result<String, AppError> {
        let db_path = Path::new(db_path);
        if !db_path.exists() {
            return Ok(String::new());
        }

        let timestamp = unix_timestamp_seconds(SystemTime::now())?;

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

fn lock_conflict_error(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::PermissionDenied
    ) || matches!(err.raw_os_error(), Some(32 | 33))
}

#[cfg(test)]
mod tests {
    use super::{acquire_owner_lock_for_lifetime, owner_lock_path, probe_owner_lock, LockManager};
    use crate::db::ProcessProbeResult;
    use crate::error::AppError;
    use crate::{
        DB_LOCK_EXTENSION, DB_LOCK_FILE_NAME, DB_OWNER_LOCK_FILE_NAME, DB_TREE_LOCK_FILE_NAME,
    };
    use fs2::FileExt;
    use std::fs::OpenOptions;
    use std::time::{Duration, UNIX_EPOCH};
    use tempfile::TempDir;

    #[test]
    fn force_unlock_removes_known_lock_files() {
        let dir = TempDir::new().expect("temp dir");
        let db_path = dir.path().join("db");
        std::fs::create_dir_all(&db_path).expect("db dir");

        let db_lock = db_path.join(DB_LOCK_FILE_NAME);
        let extra_lock = db_path.join(DB_TREE_LOCK_FILE_NAME);
        let owner_lock = db_path.join(DB_OWNER_LOCK_FILE_NAME);
        let legacy_lock = std::path::PathBuf::from(format!(
            "{}.{}",
            db_path.to_string_lossy(),
            DB_LOCK_EXTENSION
        ));
        std::fs::write(&db_lock, b"lock").expect("db lock");
        std::fs::write(&extra_lock, b"lock").expect("extra lock");
        std::fs::write(&owner_lock, b"lock").expect("owner lock");
        std::fs::write(&legacy_lock, b"lock").expect("legacy lock");

        let manager = LockManager::new(&db_path.to_string_lossy());
        let removed = manager.force_unlock().expect("force unlock");

        assert_eq!(removed, 3);
        assert!(!db_lock.exists());
        assert!(!extra_lock.exists());
        assert!(owner_lock.exists());
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
                    message.contains("unable to open for lock validation"),
                    "unexpected error: {}",
                    message
                );
            }
            other => panic!("unexpected error variant: {:?}", other),
        }
    }

    #[test]
    fn force_unlock_refuses_when_lock_is_held() {
        let dir = TempDir::new().expect("temp dir");
        let db_path = dir.path().join("db");
        std::fs::create_dir_all(&db_path).expect("db dir");
        let db_lock = db_path.join(DB_LOCK_FILE_NAME);
        std::fs::write(&db_lock, b"lock").expect("db lock");

        let held = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&db_lock)
            .expect("open db lock");
        held.try_lock_exclusive().expect("hold db lock");

        let manager = LockManager::new(&db_path.to_string_lossy());
        let err = manager
            .force_unlock()
            .expect_err("active lock should block force unlock");
        match err {
            AppError::DatabaseError(message) => {
                assert!(
                    message.contains("appears to be held by another process"),
                    "unexpected error: {}",
                    message
                );
            }
            other => panic!("unexpected error variant: {:?}", other),
        }
        assert!(db_lock.exists(), "active lock file must remain untouched");
        held.unlock().expect("release db lock");
    }

    #[test]
    fn owner_lock_probe_reports_not_running_when_lock_is_free() {
        let dir = TempDir::new().expect("temp dir");
        let db_path = dir.path().join("db");
        std::fs::create_dir_all(&db_path).expect("db dir");

        let probe = probe_owner_lock(&db_path.to_string_lossy());
        assert_eq!(probe, ProcessProbeResult::NotRunning);
    }

    #[test]
    fn owner_lock_probe_is_conservative_when_lock_is_held() {
        let dir = TempDir::new().expect("temp dir");
        let db_path = dir.path().join("db");
        std::fs::create_dir_all(&db_path).expect("db dir");

        let _guard = acquire_owner_lock_for_lifetime(&db_path.to_string_lossy())
            .expect("acquire owner lock");
        let probe = probe_owner_lock(&db_path.to_string_lossy());
        assert!(
            probe != ProcessProbeResult::NotRunning,
            "held owner lock must not be classified as safe-not-running"
        );
    }

    #[test]
    fn owner_lock_probe_reports_unknown_when_owner_lock_path_is_unusable() {
        let dir = TempDir::new().expect("temp dir");
        let db_file = dir.path().join("db-as-file");
        std::fs::write(&db_file, b"not a directory").expect("seed file");

        let probe = probe_owner_lock(&db_file.to_string_lossy());
        assert_eq!(probe, ProcessProbeResult::Unknown);
    }

    #[test]
    fn owner_lock_path_appends_owner_lock_filename() {
        let path = owner_lock_path("some-db");
        assert!(path.ends_with(DB_OWNER_LOCK_FILE_NAME));
    }

    #[test]
    fn backup_timestamp_reports_error_for_pre_epoch_clock() {
        let pre_epoch = UNIX_EPOCH - Duration::from_secs(1);
        let err =
            super::unix_timestamp_seconds(pre_epoch).expect_err("pre-epoch time should not panic");
        match err {
            AppError::DatabaseError(message) => {
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
