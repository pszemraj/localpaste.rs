//! Database layer and transactional helpers for LocalPaste.

/// Backup utilities.
pub mod backup;
/// Folder storage helpers.
pub mod folder;
/// Lock handling helpers.
pub mod lock;
/// Paste storage helpers.
pub mod paste;
/// Typed redb table definitions.
pub mod tables;
mod time_util;
mod transactions;

use crate::db::tables::REDB_FILE_NAME;
use crate::error::AppError;
use crate::folder_ops::reconcile_folder_invariants;
use redb::{Database as RedbDatabase, DatabaseError};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock, Weak};

pub use transactions::TransactionOps;

/// Process probe state used for lock-safety decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessProbeResult {
    /// A matching LocalPaste process was positively identified.
    Running,
    /// No matching LocalPaste process was found.
    NotRunning,
    /// Probe tooling/parsing failed, so liveness is uncertain.
    Unknown,
}

#[cfg(unix)]
const UNIX_PGREP_EXACT_NAMES: &[&str] = &["localpaste", "localpaste-gui"];
#[cfg(unix)]
const UNIX_PGREP_CMDLINE_NAMES: &[&str] = &["generate-test-data"];

#[cfg(unix)]
fn merge_probe_result(left: ProcessProbeResult, right: ProcessProbeResult) -> ProcessProbeResult {
    use ProcessProbeResult::{NotRunning, Running, Unknown};
    match (left, right) {
        (Running, _) | (_, Running) => Running,
        (Unknown, _) | (_, Unknown) => Unknown,
        (NotRunning, NotRunning) => NotRunning,
    }
}

#[cfg(unix)]
fn pgrep_output_probe_result(stdout: &[u8], current_pid: u32) -> ProcessProbeResult {
    let mut saw_pid = false;
    let mut saw_invalid = false;

    for line in String::from_utf8_lossy(stdout).lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match trimmed.parse::<u32>() {
            Ok(pid) => {
                saw_pid = true;
                if pid != current_pid {
                    return ProcessProbeResult::Running;
                }
            }
            Err(_) => saw_invalid = true,
        }
    }

    if saw_invalid && !saw_pid {
        ProcessProbeResult::Unknown
    } else {
        ProcessProbeResult::NotRunning
    }
}

#[cfg(unix)]
fn pgrep_probe_result(args: &[&str], current_pid: u32) -> ProcessProbeResult {
    use std::process::Command;

    let output = match Command::new("pgrep").args(args).output() {
        Ok(output) => output,
        Err(err) => return pgrep_error_probe_result(&err),
    };

    if output.status.success() {
        return pgrep_output_probe_result(&output.stdout, current_pid);
    }

    if output.status.code() == Some(1) {
        ProcessProbeResult::NotRunning
    } else {
        ProcessProbeResult::Unknown
    }
}

#[cfg(unix)]
fn pgrep_error_probe_result(err: &std::io::Error) -> ProcessProbeResult {
    if err.kind() == std::io::ErrorKind::NotFound {
        tracing::warn!("pgrep is unavailable; process ownership probe is unknown");
    }
    ProcessProbeResult::Unknown
}

#[cfg(unix)]
fn pgrep_exact_name_probe_result(process_name: &str, current_pid: u32) -> ProcessProbeResult {
    pgrep_probe_result(&["-x", process_name], current_pid)
}

#[cfg(unix)]
fn pgrep_cmdline_probe_result(binary_name: &str, current_pid: u32) -> ProcessProbeResult {
    let pattern = format!(r"(^|[/ ]){}($|[[:space:]])", binary_name);
    pgrep_probe_result(&["-f", pattern.as_str()], current_pid)
}

/// Probe for other LocalPaste processes.
#[cfg(unix)]
///
/// # Returns
/// Best-effort process-liveness classification for known LocalPaste binaries.
///
/// # Errors
/// This helper does not return `Result`; probe uncertainty is represented as
/// [`ProcessProbeResult::Unknown`].
pub fn localpaste_process_probe() -> ProcessProbeResult {
    let current_pid = std::process::id();
    let exact_probe = UNIX_PGREP_EXACT_NAMES
        .iter()
        .fold(ProcessProbeResult::NotRunning, |result, name| {
            merge_probe_result(result, pgrep_exact_name_probe_result(name, current_pid))
        });
    let cmdline_probe = UNIX_PGREP_CMDLINE_NAMES
        .iter()
        .fold(ProcessProbeResult::NotRunning, |result, name| {
            merge_probe_result(result, pgrep_cmdline_probe_result(name, current_pid))
        });
    merge_probe_result(exact_probe, cmdline_probe)
}

/// Probe for other LocalPaste processes.
#[cfg(windows)]
///
/// # Returns
/// Best-effort process-liveness classification for known LocalPaste binaries.
///
/// # Errors
/// This helper does not return `Result`; probe uncertainty is represented as
/// [`ProcessProbeResult::Unknown`].
///
/// # Panics
/// This function does not intentionally panic.
pub fn localpaste_process_probe() -> ProcessProbeResult {
    use std::process::Command;

    let output = match Command::new("tasklist").arg("/FO").arg("CSV").output() {
        Ok(output) => output,
        Err(err) => return tasklist_error_probe_result(err.kind()),
    };
    if !output.status.success() {
        return ProcessProbeResult::Unknown;
    }

    let current_pid = std::process::id();
    let csv = String::from_utf8_lossy(&output.stdout);
    let mut saw_invalid = false;
    for line in csv.lines().skip(1) {
        let parts: Vec<&str> = line.trim().trim_matches('"').split("\",\"").collect();
        if parts.len() < 2 {
            saw_invalid = true;
            continue;
        }
        let process_name = parts[0].to_ascii_lowercase();
        let pid = match parts[1].parse::<u32>() {
            Ok(pid) => pid,
            Err(_) => {
                saw_invalid = true;
                continue;
            }
        };
        if (process_name == "localpaste.exe"
            || process_name == "localpaste-gui.exe"
            || process_name == "generate-test-data.exe")
            && pid != current_pid
        {
            return ProcessProbeResult::Running;
        }
    }
    if saw_invalid {
        ProcessProbeResult::Unknown
    } else {
        ProcessProbeResult::NotRunning
    }
}

#[cfg(windows)]
fn tasklist_error_probe_result(kind: std::io::ErrorKind) -> ProcessProbeResult {
    if kind == std::io::ErrorKind::NotFound {
        tracing::warn!("tasklist is unavailable; process ownership probe is unknown");
    }
    ProcessProbeResult::Unknown
}

/// Probe for other LocalPaste processes.
#[cfg(not(any(unix, windows)))]
///
/// # Returns
/// Always returns [`ProcessProbeResult::Unknown`] on unsupported platforms.
///
/// # Errors
/// This helper does not return `Result`.
pub fn localpaste_process_probe() -> ProcessProbeResult {
    ProcessProbeResult::Unknown
}

/// Database handle with access to underlying redb tables.
pub struct Database {
    pub db: Arc<RedbDatabase>,
    pub pastes: paste::PasteDb,
    pub folders: folder::FolderDb,
    _owner_lock_guard: Option<Arc<lock::OwnerLockGuard>>,
    pub(crate) folder_txn_lock: Arc<Mutex<()>>,
}

#[cfg(test)]
mod tests;

fn looks_like_legacy_sled_layout(db_dir: &Path) -> Result<bool, AppError> {
    const SLED_HINTS: &[&str] = &[
        "blobs",
        "conf",
        "db",
        "pastes",
        "pastes_meta",
        "pastes_by_updated",
        "pastes_meta_state",
        "folders",
        "folders_deleting",
        "db.lock",
        "tree.lock",
    ];

    let entries = std::fs::read_dir(db_dir).map_err(|err| {
        AppError::StorageMessage(format!(
            "Failed to inspect database directory '{}': {}",
            db_dir.display(),
            err
        ))
    })?;

    for entry in entries {
        let entry = entry.map_err(|err| {
            AppError::StorageMessage(format!(
                "Failed to inspect database directory entry in '{}': {}",
                db_dir.display(),
                err
            ))
        })?;
        let name = entry.file_name().to_string_lossy().to_string();
        if SLED_HINTS.contains(&name.as_str()) || name.starts_with("snap.") {
            return Ok(true);
        }
        if name.ends_with(".lock") && name != "db.owner.lock" {
            return Ok(true);
        }
    }

    Ok(false)
}

impl Database {
    fn shared_folder_txn_lock_for_db(db: &Arc<RedbDatabase>) -> Result<Arc<Mutex<()>>, AppError> {
        static REGISTRY: OnceLock<Mutex<HashMap<usize, Weak<Mutex<()>>>>> = OnceLock::new();
        let registry = REGISTRY.get_or_init(|| Mutex::new(HashMap::new()));
        let mut registry_guard = registry.lock().map_err(|_| {
            AppError::StorageMessage("Shared folder transaction lock registry poisoned".to_string())
        })?;
        registry_guard.retain(|_, lock| lock.upgrade().is_some());

        let key = Arc::as_ptr(db) as usize;
        if let Some(existing) = registry_guard.get(&key).and_then(Weak::upgrade) {
            return Ok(existing);
        }

        let shared_lock = Arc::new(Mutex::new(()));
        registry_guard.insert(key, Arc::downgrade(&shared_lock));
        Ok(shared_lock)
    }

    fn from_shared_with_coordination(
        db: Arc<RedbDatabase>,
        owner_lock_guard: Option<Arc<lock::OwnerLockGuard>>,
        folder_txn_lock: Arc<Mutex<()>>,
    ) -> Result<Self, AppError> {
        Ok(Self {
            pastes: paste::PasteDb::new(db.clone())?,
            folders: folder::FolderDb::new(db.clone())?,
            db,
            _owner_lock_guard: owner_lock_guard,
            folder_txn_lock,
        })
    }

    /// Build a database handle from an existing shared redb instance.
    ///
    /// # Returns
    /// A [`Database`] handle that shares the same underlying redb instance.
    ///
    /// # Errors
    /// Returns an error when table accessors or coordination primitives cannot
    /// be initialized.
    pub fn from_shared(db: Arc<RedbDatabase>) -> Result<Self, AppError> {
        let folder_txn_lock = Self::shared_folder_txn_lock_for_db(&db)?;
        Self::from_shared_with_coordination(db, None, folder_txn_lock)
    }

    /// Clone this handle for another subsystem in the same process.
    ///
    /// # Returns
    /// A new [`Database`] view sharing the same storage handle and locks.
    ///
    /// # Errors
    /// Returns an error when accessor initialization fails.
    pub fn share(&self) -> Result<Self, AppError> {
        Self::from_shared_with_coordination(
            self.db.clone(),
            self._owner_lock_guard.clone(),
            self.folder_txn_lock.clone(),
        )
    }

    /// Open the database and initialize tables.
    ///
    /// # Returns
    /// An initialized [`Database`] instance.
    ///
    /// # Errors
    /// Returns an error when directory setup, lock acquisition, redb open, or
    /// startup invariant repair cannot be completed.
    pub fn new(path: &str) -> Result<Self, AppError> {
        let db_dir = Path::new(path);
        if db_dir.exists() && !db_dir.is_dir() {
            return Err(AppError::StorageMessage(format!(
                "DB_PATH '{}' must be a directory",
                db_dir.display()
            )));
        }

        if db_dir.exists() {
            let db_file = db_dir.join(REDB_FILE_NAME);
            if !db_file.exists() && looks_like_legacy_sled_layout(db_dir)? {
                return Err(AppError::StorageMessage(format!(
                    "Detected legacy sled database files in '{}' but '{}' is missing.\n\
                    This build uses redb and cannot read sled data directly.\n\
                    No automatic migration is bundled in this release.\n\
                    Back up this directory, migrate it using a compatible sled->redb tool,\n\
                    or set DB_PATH to a new empty directory.",
                    db_dir.display(),
                    db_file.display()
                )));
            }
        }

        std::fs::create_dir_all(db_dir).map_err(|err| {
            AppError::StorageMessage(format!(
                "Failed to create database directory '{}': {}",
                db_dir.display(),
                err
            ))
        })?;

        let owner_lock_guard = Some(Arc::new(lock::acquire_owner_lock_for_lifetime(path)?));
        let db_file = db_dir.join(REDB_FILE_NAME);
        let db = match RedbDatabase::create(&db_file) {
            Ok(db) => Arc::new(db),
            Err(DatabaseError::DatabaseAlreadyOpen) => match localpaste_process_probe() {
                ProcessProbeResult::Running => {
                    return Err(AppError::StorageMessage(
                        "Another LocalPaste instance is already running.\n\
                        Please close it first, or set DB_PATH to use a different database location."
                            .to_string(),
                    ));
                }
                ProcessProbeResult::Unknown => {
                    return Err(AppError::StorageMessage(
                        "Database appears to be open, but LocalPaste process ownership could not be verified.\n\
                        Treat this as potentially active usage. Close localpaste/localpaste-gui/\
                        generate-test-data processes and retry, or set DB_PATH to a different location."
                            .to_string(),
                    ));
                }
                ProcessProbeResult::NotRunning => {
                    return Err(AppError::StorageMessage(
                        "Database appears to be open by another writer.\n\
                        Stop other LocalPaste processes or set DB_PATH to a different location."
                            .to_string(),
                    ));
                }
            },
            Err(err) => return Err(AppError::Database(err.into())),
        };

        let database = Self {
            pastes: paste::PasteDb::new(db.clone())?,
            folders: folder::FolderDb::new(db.clone())?,
            db,
            _owner_lock_guard: owner_lock_guard,
            folder_txn_lock: Arc::new(Mutex::new(())),
        };

        database.folders.clear_delete_markers()?;
        if let Err(err) = reconcile_folder_invariants(&database) {
            tracing::error!(
                "Startup folder invariant reconcile failed; continuing in degraded mode: {}",
                err
            );
        }

        Ok(database)
    }

    /// Compatibility no-op. redb durability is guaranteed on commit.
    ///
    /// # Returns
    /// Always returns `Ok(())`.
    ///
    /// # Errors
    /// This compatibility helper currently returns no errors.
    pub fn flush(&self) -> Result<(), AppError> {
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod process_detection_tests {
    use super::{
        pgrep_error_probe_result, pgrep_output_probe_result, ProcessProbeResult,
        UNIX_PGREP_CMDLINE_NAMES,
    };
    use std::io::ErrorKind;

    #[test]
    fn unix_pid_parser_ignores_current_pid_and_invalid_lines() {
        let current_pid = 4242u32;
        let stdout = b"garbage\n4242\n";
        let probe = pgrep_output_probe_result(stdout, current_pid);
        assert_eq!(probe, ProcessProbeResult::NotRunning);
    }

    #[test]
    fn unix_pid_parser_detects_other_localpaste_pid() {
        let current_pid = 4242u32;
        let stdout = b"1111\n4242\n";
        let probe = pgrep_output_probe_result(stdout, current_pid);
        assert_eq!(probe, ProcessProbeResult::Running);
    }

    #[test]
    fn unix_pid_parser_marks_unknown_when_output_is_unparseable() {
        let current_pid = 4242u32;
        let stdout = b"not-a-pid\nalso-bad\n";
        let probe = pgrep_output_probe_result(stdout, current_pid);
        assert_eq!(probe, ProcessProbeResult::Unknown);
    }

    #[test]
    fn unix_probe_includes_tooling_writer_processes() {
        assert!(
            UNIX_PGREP_CMDLINE_NAMES.contains(&"generate-test-data"),
            "process allowlist for lock-owner detection must include tooling writers"
        );
    }

    #[test]
    fn unix_probe_treats_missing_pgrep_as_unknown() {
        let err = std::io::Error::new(ErrorKind::NotFound, "pgrep not found");
        let probe = pgrep_error_probe_result(&err);
        assert_eq!(probe, ProcessProbeResult::Unknown);
    }

    #[test]
    fn unix_probe_keeps_unknown_for_non_notfound_pgrep_errors() {
        let err = std::io::Error::new(ErrorKind::PermissionDenied, "permission denied");
        let probe = pgrep_error_probe_result(&err);
        assert_eq!(probe, ProcessProbeResult::Unknown);
    }
}

#[cfg(all(test, windows))]
mod process_detection_windows_tests {
    use super::{tasklist_error_probe_result, ProcessProbeResult};
    use std::io::ErrorKind;

    #[test]
    fn windows_probe_treats_missing_tasklist_as_unknown() {
        let probe = tasklist_error_probe_result(ErrorKind::NotFound);
        assert_eq!(probe, ProcessProbeResult::Unknown);
    }

    #[test]
    fn windows_probe_keeps_unknown_for_other_tasklist_errors() {
        let probe = tasklist_error_probe_result(ErrorKind::PermissionDenied);
        assert_eq!(probe, ProcessProbeResult::Unknown);
    }
}
