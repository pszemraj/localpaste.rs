pub mod backup;
pub mod folder;
pub mod lock;
pub mod paste;

use crate::{
    error::AppError,
    models::{
        folder::Folder,
        paste::{apply_update, Paste, UpdatePasteRequest},
    },
};
use sled::{
    transaction::{
        ConflictableTransactionError, ConflictableTransactionResult, TransactionError,
        TransactionResult, Transactional, TransactionalTree,
    },
    Db, IVec,
};
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

pub struct Database {
    pub db: Arc<Db>,
    pub pastes: paste::PasteDb,
    pub folders: folder::FolderDb,
}

#[cfg(test)]
mod tests;

/// Helper operations that need to touch multiple trees atomically.
pub struct TransactionOps;

impl TransactionOps {
    /// Atomically create a paste and update folder count
    pub fn create_paste_with_folder(
        db: &Database,
        paste: &Paste,
        folder_id: &str,
    ) -> Result<(), AppError> {
        let folders_tree = db.folders.tree();
        let pastes_tree = db.pastes.tree();

        let tx = (folders_tree, pastes_tree).transaction(|(folders, pastes)| {
            change_folder_count(folders, folder_id, 1)?;

            let value =
                bincode::serialize(paste).map_err(|e| abort(AppError::Serialization(e)))?;
            pastes.insert(paste.id.as_bytes(), value)?;
            Ok(())
        });

        tx_result(tx)?;
        Ok(())
    }

    /// Atomically delete a paste and update folder count
    pub fn delete_paste_with_folder(
        db: &Database,
        paste_id: &str,
        folder_id: &str,
    ) -> Result<bool, AppError> {
        let pastes_tree = db.pastes.tree();
        let folders_tree = db.folders.tree();

        let tx = (pastes_tree, folders_tree).transaction(|(pastes, folders)| {
            match pastes.remove(paste_id.as_bytes())? {
                Some(_) => {
                    change_folder_count(folders, folder_id, -1)?;
                    Ok(true)
                }
                None => Ok(false),
            }
        });

        tx_result(tx)
    }

    /// Atomically move a paste between folders
    pub fn move_paste_between_folders(
        db: &Database,
        paste_id: &str,
        update_req: UpdatePasteRequest,
    ) -> Result<Option<Paste>, AppError> {
        let folders_tree = db.folders.tree();
        let pastes_tree = db.pastes.tree();

        let tx = (folders_tree, pastes_tree).transaction(|(folders, pastes)| {
            let existing: IVec = match pastes.get(paste_id.as_bytes())? {
                Some(bytes) => bytes,
                None => return Ok(None),
            };

            let mut paste: Paste =
                bincode::deserialize(&existing).map_err(|e| abort(AppError::Serialization(e)))?;

            let previous_folder = paste.folder_id.clone();
            apply_update(&mut paste, &update_req);
            let updated_folder = paste.folder_id.clone();

            if previous_folder != updated_folder {
                if let Some(old_id) = previous_folder.as_deref() {
                    change_folder_count(folders, old_id, -1)?;
                }
                if let Some(new_id) = updated_folder.as_deref() {
                    change_folder_count(folders, new_id, 1)?;
                }
            }

            let value =
                bincode::serialize(&paste).map_err(|e| abort(AppError::Serialization(e)))?;
            pastes.insert(paste_id.as_bytes(), value)?;

            Ok(Some(paste))
        });

        tx_result(tx)
    }
}

fn tx_result<T>(res: TransactionResult<T, AppError>) -> Result<T, AppError> {
    match res {
        Ok(value) => Ok(value),
        Err(TransactionError::Abort(err)) => Err(err),
        Err(TransactionError::Storage(err)) => Err(AppError::from(err)),
    }
}

fn abort(err: AppError) -> ConflictableTransactionError<AppError> {
    ConflictableTransactionError::Abort(err)
}

fn change_folder_count(
    folders: &TransactionalTree,
    folder_id: &str,
    delta: i32,
) -> ConflictableTransactionResult<(), AppError> {
    if delta == 0 {
        return Ok(());
    }

    let key = folder_id.as_bytes();
    let value = folders
        .get(key)?
        .ok_or_else(|| abort(AppError::NotFound))?;

    let mut folder: Folder =
        bincode::deserialize(&value).map_err(|e| abort(AppError::Serialization(e)))?;

    if delta > 0 {
        folder.paste_count = folder.paste_count.saturating_add(delta as usize);
    } else {
        folder.paste_count = folder
            .paste_count
            .saturating_sub((-delta) as usize);
    }

    let serialized =
        bincode::serialize(&folder).map_err(|e| abort(AppError::Serialization(e)))?;
    folders.insert(key, serialized)?;
    Ok(())
}

impl Database {
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
    #[allow(dead_code)]
    pub fn checksum(&self) -> Result<u32, AppError> {
        self.db
            .checksum()
            .map_err(|e| AppError::DatabaseError(format!("Failed to compute checksum: {}", e)))
    }

    /// Flush all pending writes to disk
    pub fn flush(&self) -> Result<(), AppError> {
        self.db.flush()?;
        Ok(())
    }
}
