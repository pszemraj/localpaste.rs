//! Database layer and transactional helpers for LocalPaste.

/// Backup utilities.
pub mod backup;
/// Folder storage helpers.
pub mod folder;
mod fs_copy;
/// Lock handling helpers.
pub mod lock;
/// Paste storage helpers.
pub mod paste;

use crate::error::AppError;
use crate::{DB_LOCK_EXTENSION, DB_LOCK_FILE_NAME};
use sled::Db;
use std::sync::Arc;

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
        // Minimal Unix environments may not ship `pgrep`; treat that as a recoverable
        // "not running" fallback so stale lock recovery is not permanently blocked.
        ProcessProbeResult::NotRunning
    } else {
        ProcessProbeResult::Unknown
    }
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
///
/// # Returns
/// A tri-state result describing known running, known not-running, or unknown.
///
/// # Errors
/// This probe is best-effort and never returns an error; uncertainty is reported as
/// [`ProcessProbeResult::Unknown`].
#[cfg(unix)]
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
///
/// # Returns
/// A tri-state result describing known running, known not-running, or unknown.
///
/// # Errors
/// This probe is best-effort and never returns an error; uncertainty is reported as
/// [`ProcessProbeResult::Unknown`].
///
/// # Panics
/// This function does not intentionally panic.
#[cfg(windows)]
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
        // Keep stale lock recovery reachable when tasklist is unavailable.
        ProcessProbeResult::NotRunning
    } else {
        ProcessProbeResult::Unknown
    }
}

/// Probe for other LocalPaste processes.
///
/// # Returns
/// Returns `Unknown` on unsupported platforms.
///
/// # Errors
/// This probe is best-effort and never returns an error; unsupported platforms
/// are classified as [`ProcessProbeResult::Unknown`].
#[cfg(not(any(unix, windows)))]
pub fn localpaste_process_probe() -> ProcessProbeResult {
    ProcessProbeResult::Unknown
}

/// Database handle with access to underlying sled trees.
pub struct Database {
    pub db: Arc<Db>,
    pub pastes: paste::PasteDb,
    pub folders: folder::FolderDb,
}

#[cfg(test)]
mod tests;

/// Transaction-like operations for atomic updates across trees.
///
/// Sled transactions are limited to a single tree, so we use careful ordering
/// and rollback logic to maintain consistency across trees.
pub struct TransactionOps;

impl TransactionOps {
    /// Atomically create a paste and update folder count
    ///
    /// # Arguments
    /// - `db`: Database handle.
    /// - `paste`: Paste to insert.
    /// - `folder_id`: Folder that will contain the paste.
    ///
    /// # Returns
    /// `Ok(())` on success.
    ///
    /// # Errors
    /// Propagates storage errors from paste or folder updates.
    pub fn create_paste_with_folder(
        db: &Database,
        paste: &crate::models::paste::Paste,
        folder_id: &str,
    ) -> Result<(), AppError> {
        // First update folder count atomically
        db.folders.update_count(folder_id, 1)?;

        // Then create paste - if this fails, rollback folder count
        if let Err(e) = db.pastes.create(paste) {
            // Best effort rollback - log but don't fail if rollback fails
            if let Err(rollback_err) = db.folders.update_count(folder_id, -1) {
                tracing::error!("Failed to rollback folder count: {}", rollback_err);
            }
            return Err(e);
        }

        Ok(())
    }

    /// Atomically delete a paste and update folder count
    ///
    /// # Arguments
    /// - `db`: Database handle.
    /// - `paste_id`: Paste identifier to delete.
    ///
    /// # Returns
    /// `Ok(true)` if a paste was deleted, `Ok(false)` if not found.
    ///
    /// # Errors
    /// Propagates storage errors from the paste tree.
    pub fn delete_paste_with_folder(db: &Database, paste_id: &str) -> Result<bool, AppError> {
        let deleted = db.pastes.delete_and_return(paste_id)?;

        if let Some(paste) = deleted {
            if let Some(folder_id) = paste.folder_id.as_deref() {
                // Update folder count - if this fails, log but continue
                // (paste is already deleted, better to have incorrect count than fail)
                if let Err(e) = db.folders.update_count(folder_id, -1) {
                    tracing::error!("Failed to update folder count after paste deletion: {}", e);
                }
            }

            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Atomically move a paste between folders
    ///
    /// # Arguments
    /// - `db`: Database handle.
    /// - `paste_id`: Paste identifier to update.
    /// - `new_folder_id`: Destination folder id, if any.
    /// - `update_req`: Update payload to apply to the paste.
    ///
    /// # Returns
    /// Updated paste if it existed.
    ///
    /// # Errors
    /// Propagates storage errors from paste or folder updates.
    pub fn move_paste_between_folders(
        db: &Database,
        paste_id: &str,
        new_folder_id: Option<&str>,
        update_req: crate::models::paste::UpdatePasteRequest,
    ) -> Result<Option<crate::models::paste::Paste>, AppError> {
        const MAX_MOVE_RETRIES: usize = 8;

        for _ in 0..MAX_MOVE_RETRIES {
            let current = match db.pastes.get(paste_id)? {
                Some(paste) => paste,
                None => return Ok(None),
            };

            let old_folder_id = current.folder_id.as_deref();
            let folder_changing = old_folder_id != new_folder_id;

            // Reserve the destination count first so we can fail fast if the folder is gone.
            if folder_changing {
                if let Some(new_id) = new_folder_id {
                    db.folders.update_count(new_id, 1)?;
                }
            }

            let update_result =
                db.pastes
                    .update_if_folder_matches(paste_id, old_folder_id, update_req.clone());
            match update_result {
                Ok(Some(updated)) => {
                    // Paste update succeeded; best-effort decrement of prior folder count.
                    if folder_changing {
                        if let Some(old_id) = old_folder_id {
                            if let Err(err) = db.folders.update_count(old_id, -1) {
                                tracing::error!(
                                    "Failed to decrement old folder count after move: {}",
                                    err
                                );
                            }
                        }
                    }
                    return Ok(Some(updated));
                }
                Ok(None) => {
                    // Compare-and-swap mismatch or deletion. Roll back destination reservation.
                    if folder_changing {
                        if let Some(new_id) = new_folder_id {
                            if let Err(err) = db.folders.update_count(new_id, -1) {
                                tracing::error!(
                                    "Failed to rollback destination folder count after conflict: {}",
                                    err
                                );
                            }
                        }
                    }

                    if db.pastes.get(paste_id)?.is_none() {
                        return Ok(None);
                    }
                }
                Err(err) => {
                    if folder_changing {
                        if let Some(new_id) = new_folder_id {
                            if let Err(rollback_err) = db.folders.update_count(new_id, -1) {
                                tracing::error!(
                                    "Failed to rollback destination folder count after error: {}",
                                    rollback_err
                                );
                            }
                        }
                    }
                    return Err(err);
                }
            }
        }

        Err(AppError::DatabaseError(
            "Paste update conflicted repeatedly; please retry.".to_string(),
        ))
    }
}

impl Database {
    /// Build a database handle from an existing shared sled instance.
    ///
    /// This is used when multiple components in the same process need
    /// independent helpers (trees) without reopening the database path.
    ///
    /// # Returns
    /// A new [`Database`] wrapper that shares the underlying sled instance.
    ///
    /// # Errors
    /// Returns an error if the required trees cannot be opened.
    pub fn from_shared(db: Arc<Db>) -> Result<Self, AppError> {
        Ok(Self {
            pastes: paste::PasteDb::new(db.clone())?,
            folders: folder::FolderDb::new(db.clone())?,
            db,
        })
    }

    /// Clone this handle for another subsystem in the same process.
    ///
    /// This avoids a second `sled::open` call (which would contend for the
    /// filesystem lock) while still providing separate tree handles.
    ///
    /// # Returns
    /// A new [`Database`] that shares the underlying sled instance.
    ///
    /// # Errors
    /// Returns an error if tree initialization fails.
    pub fn share(&self) -> Result<Self, AppError> {
        Self::from_shared(self.db.clone())
    }

    /// Open the database and initialize trees.
    ///
    /// # Returns
    /// A fully initialized [`Database`].
    ///
    /// # Errors
    /// Returns an error if sled cannot open the database or trees.
    pub fn new(path: &str) -> Result<Self, AppError> {
        // Ensure the data directory exists
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent).ok();
        }

        // Try to open database - sled handles its own locking
        let db = match sled::open(path) {
            Ok(db) => Arc::new(db),
            Err(e) if e.to_string().contains("could not acquire lock") => {
                // This is sled's internal lock, not our lock file
                // It means another process has the database open

                // Uncertain liveness must remain conservative to avoid data corruption.
                match localpaste_process_probe() {
                    ProcessProbeResult::Running => {
                        return Err(AppError::DatabaseError(
                            "Another LocalPaste instance is already running.\n\
                            Please close it first, or set DB_PATH to use a different database location."
                                .to_string(),
                        ));
                    }
                    ProcessProbeResult::Unknown => {
                        return Err(AppError::DatabaseError(
                            "Database appears to be locked, but LocalPaste process ownership could not be verified.\n\
                            Treat this as potentially active usage; do not force unlock.\n\
                            Close any localpaste/localpaste-gui/generate-test-data processes, then retry,\n\
                            or set DB_PATH to a different location."
                                .to_string(),
                        ));
                    }
                    ProcessProbeResult::NotRunning => {
                        let parent = std::path::Path::new(path)
                            .parent()
                            .unwrap_or(std::path::Path::new("."))
                            .display()
                            .to_string();
                        let wildcard = format!("{}\\*.{}", path, DB_LOCK_EXTENSION);
                        let (backup_cmd, remove_cmd, restore_cmd) = if cfg!(windows) {
                            (
                                format!(
                                    "Copy-Item -Recurse -Force \"{}\" \"{}.backup\"",
                                    path, path
                                ),
                                format!(
                                    "Remove-Item -Force \"{}\",\"{}\\\\{}\",\"{}.{}\"",
                                    wildcard,
                                    path,
                                    DB_LOCK_FILE_NAME,
                                    path,
                                    DB_LOCK_EXTENSION
                                ),
                                format!(
                                    "Get-ChildItem \"{}\\*.backup.*\" | Sort-Object LastWriteTime | Select-Object -Last 1",
                                    parent
                                ),
                            )
                        } else {
                            (
                                format!("cp -r {} {}.backup", path, path),
                                format!(
                                    "rm -f {0}/*.{1} {0}/{2} {0}.{1}",
                                    path, DB_LOCK_EXTENSION, DB_LOCK_FILE_NAME,
                                ),
                                format!("ls -la {}/*.backup.* | tail -1", parent),
                            )
                        };

                        return Err(AppError::DatabaseError(format!(
                            "Database appears to be locked.\n\
                            Another process may still be using it, or a previous crash left a stale lock.\n\
                            If you just started the localpaste server for CLI tests, stop it before starting the GUI,\n\
                            or set DB_PATH to a different location.\n\n\
                            To recover from a stale lock:\n\
                            1. {}\n\
                            2. {}\n\
                            3. Try starting again\n\n\
                            If that doesn't work, restore from auto-backup:\n\
                            {}\n\
                            Or use:\n\
                            localpaste --force-unlock",
                            backup_cmd, remove_cmd, restore_cmd
                        )));
                    }
                }
            }
            Err(e) => return Err(AppError::DatabaseError(e.to_string())),
        };

        let database = Self {
            pastes: paste::PasteDb::new(db.clone())?,
            folders: folder::FolderDb::new(db.clone())?,
            db,
        };
        let force_reindex = crate::config::env_flag_enabled("LOCALPASTE_REINDEX");
        if database
            .pastes
            .needs_reconcile_meta_indexes(force_reindex)?
        {
            database.pastes.reconcile_meta_indexes()?;
        }
        Ok(database)
    }

    /// Flush all pending writes to disk.
    ///
    /// # Returns
    /// `Ok(())` after all pending writes are flushed.
    ///
    /// # Errors
    /// Returns an error if sled fails to flush.
    pub fn flush(&self) -> Result<(), AppError> {
        self.db.flush()?;
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
    fn unix_force_unlock_guard_includes_tooling_writer_processes() {
        assert!(
            UNIX_PGREP_CMDLINE_NAMES.contains(&"generate-test-data"),
            "process allowlist for lock-owner detection must include tooling writers"
        );
    }

    #[test]
    fn unix_probe_treats_missing_pgrep_as_not_running() {
        let err = std::io::Error::new(ErrorKind::NotFound, "pgrep not found");
        let probe = pgrep_error_probe_result(&err);
        assert_eq!(probe, ProcessProbeResult::NotRunning);
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
    fn windows_probe_treats_missing_tasklist_as_not_running() {
        let probe = tasklist_error_probe_result(ErrorKind::NotFound);
        assert_eq!(probe, ProcessProbeResult::NotRunning);
    }

    #[test]
    fn windows_probe_keeps_unknown_for_other_tasklist_errors() {
        let probe = tasklist_error_probe_result(ErrorKind::PermissionDenied);
        assert_eq!(probe, ProcessProbeResult::Unknown);
    }
}
