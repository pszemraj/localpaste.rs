//! Owner-lock helpers for process-level writer coordination.

use super::ProcessProbeResult;
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

/// Probe whether another process currently holds the owner lock.
///
/// # Returns
/// [`ProcessProbeResult::Running`] when locked by another process, or
/// [`ProcessProbeResult::NotRunning`] when this process can safely acquire/release
/// the owner lock. Probe/tooling failures return [`ProcessProbeResult::Unknown`].
///
/// # Errors
/// This helper does not return `Result`; uncertainty is represented as
/// [`ProcessProbeResult::Unknown`].
pub fn probe_owner_lock(db_path: &str) -> ProcessProbeResult {
    let lock_path = owner_lock_path(db_path);
    if let Some(parent) = lock_path.parent() {
        if parent.exists() && !parent.is_dir() {
            tracing::warn!(
                "Owner-lock probe found non-directory parent '{}'",
                parent.display()
            );
            return ProcessProbeResult::Unknown;
        }
    }
    if !lock_path.exists() {
        return ProcessProbeResult::NotRunning;
    }

    let file = match OpenOptions::new()
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
    {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return ProcessProbeResult::NotRunning;
        }
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
        Err(err) if lock_conflict_error(&err) => ProcessProbeResult::Running,
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

fn lock_conflict_error(err: &std::io::Error) -> bool {
    matches!(err.kind(), std::io::ErrorKind::WouldBlock)
        || matches!(err.raw_os_error(), Some(32 | 33))
}

#[cfg(test)]
mod tests {
    use super::{acquire_owner_lock_for_lifetime, owner_lock_path, probe_owner_lock};
    use crate::db::ProcessProbeResult;
    use crate::DB_OWNER_LOCK_FILE_NAME;
    use tempfile::TempDir;

    #[test]
    fn owner_lock_probe_status_matrix_covers_free_held_and_unusable_paths() {
        enum ProbeCase {
            FreeDirectory,
            HeldDirectory,
            UnusablePath,
        }

        let cases = [
            ProbeCase::FreeDirectory,
            ProbeCase::HeldDirectory,
            ProbeCase::UnusablePath,
        ];

        for case in cases {
            match case {
                ProbeCase::FreeDirectory => {
                    let dir = TempDir::new().expect("temp dir");
                    let db_path = dir.path().join("db");
                    std::fs::create_dir_all(&db_path).expect("db dir");
                    let probe = probe_owner_lock(&db_path.to_string_lossy());
                    assert_eq!(probe, ProcessProbeResult::NotRunning);
                }
                ProbeCase::HeldDirectory => {
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
                ProbeCase::UnusablePath => {
                    let dir = TempDir::new().expect("temp dir");
                    let db_file = dir.path().join("db-as-file");
                    std::fs::write(&db_file, b"not a directory").expect("seed file");
                    let probe = probe_owner_lock(&db_file.to_string_lossy());
                    assert_eq!(probe, ProcessProbeResult::Unknown);
                }
            }
        }
    }

    #[test]
    fn owner_lock_probe_does_not_create_missing_directories() {
        let dir = TempDir::new().expect("temp dir");
        let db_path = dir.path().join("missing").join("db");
        let parent = db_path.parent().expect("db parent").to_path_buf();
        assert!(!parent.exists(), "test precondition: parent must be absent");

        let probe = probe_owner_lock(&db_path.to_string_lossy());
        assert_eq!(probe, ProcessProbeResult::NotRunning);
        assert!(
            !parent.exists(),
            "probe_owner_lock should not create directories as a side effect"
        );
    }

    #[test]
    fn owner_lock_path_appends_owner_lock_filename() {
        let path = owner_lock_path("some-db");
        assert!(path.ends_with(DB_OWNER_LOCK_FILE_NAME));
    }
}
