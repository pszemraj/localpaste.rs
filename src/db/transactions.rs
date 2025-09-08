use crate::error::AppError;
use crate::models::{folder::Folder, paste::Paste};
use sled::Transactional;

/// Proper atomic transactions using Sled's transaction API
pub struct AtomicOps;

impl AtomicOps {
    /// Atomically create a paste using Sled transactions
    pub fn create_paste(
        db: &sled::Db,
        paste: &Paste,
    ) -> Result<(), AppError> {
        let paste_tree = db.open_tree("pastes")?;
        
        // Use Sled's transaction API for atomicity
        (&paste_tree,).transaction(|(pastes,)| {
            let key = paste.id.as_bytes();
            let value = bincode::serialize(paste)
                .map_err(|e| sled::transaction::ConflictableTransactionError::Abort(e))?;
            pastes.insert(key, value)?;
            Ok(())
        }).map_err(|e| AppError::DatabaseError(format!("Transaction failed: {}", e)))?;
        
        Ok(())
    }
    
    /// Atomically update a paste
    pub fn update_paste(
        db: &sled::Db,
        paste_id: &str,
        updates: impl FnOnce(&mut Paste) -> Result<(), AppError>,
    ) -> Result<Option<Paste>, AppError> {
        let paste_tree = db.open_tree("pastes")?;
        
        let result = (&paste_tree,).transaction(|(pastes,)| {
            let key = paste_id.as_bytes();
            
            match pastes.get(key)? {
                Some(bytes) => {
                    let mut paste: Paste = bincode::deserialize(&bytes)
                        .map_err(|e| sled::transaction::ConflictableTransactionError::Abort(e))?;
                    
                    // Apply updates
                    updates(&mut paste)
                        .map_err(|e| sled::transaction::ConflictableTransactionError::Abort(e))?;
                    
                    paste.updated_at = chrono::Utc::now();
                    
                    let new_value = bincode::serialize(&paste)
                        .map_err(|e| sled::transaction::ConflictableTransactionError::Abort(e))?;
                    pastes.insert(key, new_value)?;
                    
                    Ok(Some(paste))
                }
                None => Ok(None)
            }
        }).map_err(|e| AppError::DatabaseError(format!("Update transaction failed: {}", e)))?;
        
        Ok(result)
    }
    
    /// Atomically delete a paste
    pub fn delete_paste(
        db: &sled::Db,
        paste_id: &str,
    ) -> Result<bool, AppError> {
        let paste_tree = db.open_tree("pastes")?;
        
        let deleted = (&paste_tree,).transaction(|(pastes,)| {
            let key = paste_id.as_bytes();
            Ok(pastes.remove(key)?.is_some())
        }).map_err(|e| AppError::DatabaseError(format!("Delete transaction failed: {}", e)))?;
        
        Ok(deleted)
    }
    
    /// Atomically move paste between folders (if we were still tracking counts)
    /// Since we calculate counts on demand now, this is just a regular update
    pub fn move_paste_to_folder(
        db: &sled::Db,
        paste_id: &str,
        new_folder_id: Option<String>,
    ) -> Result<Option<Paste>, AppError> {
        Self::update_paste(db, paste_id, |paste| {
            paste.folder_id = new_folder_id;
            Ok(())
        })
    }
}