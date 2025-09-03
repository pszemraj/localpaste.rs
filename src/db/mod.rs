pub mod folder;
pub mod lock;
pub mod paste;

use crate::error::AppError;
use sled::Db;
use std::sync::Arc;

pub struct Database {
    #[allow(dead_code)]
    db: Arc<Db>,
    pub pastes: paste::PasteDb,
    pub folders: folder::FolderDb,
}

#[cfg(test)]
mod tests;

/// Transaction-like operations for atomic updates across trees
/// Since sled transactions require single tree, we use careful ordering
/// and rollback logic to maintain consistency
pub struct TransactionOps;

impl TransactionOps {
    /// Atomically create a paste and update folder count
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
    pub fn new(path: &str) -> Result<Self, AppError> {
        // Ensure the data directory exists
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent).ok();
        }

        // Check for lock issues before opening
        let lock_manager = lock::LockManager::new(path);
        match lock_manager.check_lock() {
            lock::LockStatus::StaleLock => {
                tracing::warn!("Found stale database lock, attempting cleanup...");
                lock_manager.cleanup_stale_lock()?;
            }
            lock::LockStatus::LockedByProcess(pid) => {
                return Err(AppError::DatabaseError(format!(
                    "Database is locked by another LocalPaste instance (PID: {}). \
                    Please close it first or use --force-unlock if you're sure it's not running.",
                    pid
                )));
            }
            lock::LockStatus::LockedUnknown => {
                return Err(AppError::DatabaseError(
                    "Database appears to be locked. If LocalPaste crashed previously, \
                    you may need to use --force-unlock to recover.".to_string()
                ));
            }
            lock::LockStatus::Unlocked => {
                // Good to go
            }
        }

        // Open database - sled handles concurrent access properly
        let db = match sled::open(path) {
            Ok(db) => Arc::new(db),
            Err(e) if e.to_string().contains("could not acquire lock") => {
                // Provide helpful error message
                return Err(AppError::DatabaseError(format!(
                    "Could not open database at '{}'. Another instance may be running.\n\
                    If you're sure no other instance is running, you can:\n\
                    1. Run with --force-unlock to remove stale locks\n\
                    2. Check for other LocalPaste processes: ps aux | grep localpaste\n\
                    3. Remove lock files manually: rm {}/*.lock\n\
                    Original error: {}",
                    path, path, e
                )));
            }
            Err(e) => return Err(AppError::DatabaseError(e.to_string())),
        };

        Ok(Self {
            pastes: paste::PasteDb::new(db.clone())?,
            folders: folder::FolderDb::new(db.clone())?,
            db,
        })
    }

    /// Flush all pending writes to disk
    pub fn flush(&self) -> Result<(), AppError> {
        self.db.flush()?;
        Ok(())
    }
}
