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

/// Check if a LocalPaste process is already running
#[cfg(unix)]
fn is_localpaste_running() -> bool {
    use std::process::Command;

    let output = Command::new("pgrep").arg("-f").arg("localpaste").output();

    match output {
        Ok(result) => !result.stdout.is_empty(),
        Err(_) => false,
    }
}

#[cfg(not(unix))]
fn is_localpaste_running() -> bool {
    // On non-Unix, be conservative
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
    /// - `folder_id`: Folder containing the paste.
    ///
    /// # Returns
    /// `Ok(true)` if a paste was deleted, `Ok(false)` if not found.
    ///
    /// # Errors
    /// Propagates storage errors from the paste tree.
    pub fn delete_paste_with_folder(
        db: &Database,
        paste_id: &str,
        folder_id: &str,
    ) -> Result<bool, AppError> {
        // Delete paste first
        let deleted = db.pastes.delete(paste_id)?;

        if deleted {
            // Update folder count - if this fails, log but continue
            // (paste is already deleted, better to have incorrect count than fail)
            if let Err(e) = db.folders.update_count(folder_id, -1) {
                tracing::error!("Failed to update folder count after paste deletion: {}", e);
            }
        }

        Ok(deleted)
    }

    /// Atomically move a paste between folders
    ///
    /// # Arguments
    /// - `db`: Database handle.
    /// - `paste_id`: Paste identifier to update.
    /// - `old_folder_id`: Existing folder id, if any.
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
        old_folder_id: Option<&str>,
        new_folder_id: Option<&str>,
        update_req: crate::models::paste::UpdatePasteRequest,
    ) -> Result<Option<crate::models::paste::Paste>, AppError> {
        // If folder is changing, update counts first
        if old_folder_id != new_folder_id {
            // Decrement old folder count
            if let Some(old_id) = old_folder_id {
                db.folders.update_count(old_id, -1)?;
            }

            // Increment new folder count
            if let Some(new_id) = new_folder_id {
                if let Err(e) = db.folders.update_count(new_id, 1) {
                    // Rollback old folder count change
                    if let Some(old_id) = old_folder_id {
                        if let Err(rollback_err) = db.folders.update_count(old_id, 1) {
                            tracing::error!(
                                "Failed to rollback old folder count: {}",
                                rollback_err
                            );
                        }
                    }
                    return Err(e);
                }
            }

            // Update paste - if this fails, rollback both folder counts
            match db.pastes.update(paste_id, update_req) {
                Ok(result) => Ok(result),
                Err(e) => {
                    // Rollback folder count changes
                    if let Some(old_id) = old_folder_id {
                        if let Err(rollback_err) = db.folders.update_count(old_id, 1) {
                            tracing::error!(
                                "Failed to rollback old folder count: {}",
                                rollback_err
                            );
                        }
                    }
                    if let Some(new_id) = new_folder_id {
                        if let Err(rollback_err) = db.folders.update_count(new_id, -1) {
                            tracing::error!(
                                "Failed to rollback new folder count: {}",
                                rollback_err
                            );
                        }
                    }
                    Err(e)
                }
            }
        } else {
            // No folder change, just update paste
            db.pastes.update(paste_id, update_req)
        }
    }
}

impl Database {
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
                        Please close it first or wait for it to shut down."
                            .to_string(),
                    ));
                } else {
                    // No LocalPaste running, probably a stale sled lock
                    // Sled locks are in the DB directory itself
                    return Err(AppError::DatabaseError(format!(
                        "Database appears to be locked but no LocalPaste is running.\n\
                        This can happen after a crash. To recover:\n\n\
                        1. Make a backup: cp -r {} {}.backup\n\
                        2. Remove lock files: rm {}/*.lock {}/db.lock\n\
                        3. Try starting again\n\n\
                        If that doesn't work, restore from auto-backup:\n\
                        ls -la {}/*.backup.* | tail -1",
                        path,
                        path,
                        path,
                        path,
                        std::path::Path::new(path)
                            .parent()
                            .unwrap_or(std::path::Path::new("."))
                            .display()
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

    /// Get database checksum for verification
    ///
    /// # Returns
    /// A checksum value from sled.
    ///
    /// # Errors
    /// Returns an error if sled cannot compute a checksum.
    #[allow(dead_code)]
    pub fn checksum(&self) -> Result<u32, AppError> {
        self.db
            .checksum()
            .map_err(|e| AppError::DatabaseError(format!("Failed to compute checksum: {}", e)))
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
