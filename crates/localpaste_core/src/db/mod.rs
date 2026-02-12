//! Database layer and transactional helpers for LocalPaste.

/// Backup utilities.
pub mod backup;
/// Folder storage helpers.
pub mod folder;
/// Lock handling helpers.
pub mod lock;
/// Paste storage helpers.
pub mod paste;

use crate::error::AppError;
use sled::Db;
use std::sync::Arc;

/// Check if another LocalPaste process is already running.
///
/// # Returns
/// `true` when a matching process id (other than the current process) is found.
#[cfg(unix)]
pub fn is_localpaste_running() -> bool {
    use std::process::Command;

    let current_pid = std::process::id();
    let output = Command::new("pgrep").arg("-f").arg("localpaste").output();

    match output {
        Ok(result) => String::from_utf8_lossy(&result.stdout)
            .lines()
            .filter_map(|line| line.trim().parse::<u32>().ok())
            .any(|pid| pid != current_pid),
        Err(_) => false,
    }
}

/// Check if another LocalPaste process is already running.
///
/// # Returns
/// `true` when a matching process id (other than the current process) is found.
///
/// # Panics
/// This function does not intentionally panic.
#[cfg(windows)]
pub fn is_localpaste_running() -> bool {
    use std::process::Command;

    let output = Command::new("tasklist").arg("/FO").arg("CSV").output();
    let Ok(output) = output else {
        return false;
    };

    let current_pid = std::process::id();
    let csv = String::from_utf8_lossy(&output.stdout);
    csv.lines().skip(1).any(|line| {
        let parts: Vec<&str> = line.trim().trim_matches('"').split("\",\"").collect();
        if parts.len() < 2 {
            return false;
        }
        let process_name = parts[0].to_ascii_lowercase();
        let pid = parts[1].parse::<u32>().ok();
        (process_name == "localpaste.exe" || process_name == "localpaste-gui.exe")
            && pid.map(|pid| pid != current_pid).unwrap_or(false)
    })
}

/// Check if another LocalPaste process is already running.
///
/// # Returns
/// Always returns `false` on unsupported platforms.
#[cfg(not(any(unix, windows)))]
pub fn is_localpaste_running() -> bool {
    false
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

                // Check if there's actually another LocalPaste process
                if is_localpaste_running() {
                    return Err(AppError::DatabaseError(
                        "Another LocalPaste instance is already running.\n\
                        Please close it first, or set DB_PATH to use a different database location."
                            .to_string(),
                    ));
                } else {
                    let parent = std::path::Path::new(path)
                        .parent()
                        .unwrap_or(std::path::Path::new("."))
                        .display()
                        .to_string();
                    let (backup_cmd, remove_cmd, restore_cmd) = if cfg!(windows) {
                        (
                            format!(
                                "Copy-Item -Recurse -Force \"{}\" \"{}.backup\"",
                                path, path
                            ),
                            format!(
                                "Remove-Item -Force \"{}\\*.lock\",\"{}\\db.lock\",\"{}.lock\"",
                                path, path, path
                            ),
                            format!(
                                "Get-ChildItem \"{}\\*.backup.*\" | Sort-Object LastWriteTime | Select-Object -Last 1",
                                parent
                            ),
                        )
                    } else {
                        (
                            format!("cp -r {} {}.backup", path, path),
                            format!("rm -f {}/*.lock {}/db.lock {}.lock", path, path, path),
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
            Err(e) => return Err(AppError::DatabaseError(e.to_string())),
        };

        Ok(Self {
            pastes: paste::PasteDb::new(db.clone())?,
            folders: folder::FolderDb::new(db.clone())?,
            db,
        })
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
