use crate::error::AppError;
use crate::models::paste::Paste;
use sled::Transactional;

/// Proper atomic transactions using Sled's transaction API
///
/// Note: These are example implementations showing how to use Sled's
/// transaction API for truly atomic operations. Currently not used in
/// production code but available for future migration.
pub struct AtomicOps;

impl AtomicOps {
    /// Example: Atomically swap two paste IDs
    /// This demonstrates a transaction that would be impossible without proper atomicity
    #[allow(dead_code)]
    pub fn swap_paste_ids(db: &sled::Db, id1: &str, id2: &str) -> Result<(), AppError> {
        let paste_tree = db.open_tree("pastes")?;

        (&paste_tree,)
            .transaction(|(pastes,)| {
                let key1 = id1.as_bytes();
                let key2 = id2.as_bytes();

                // Get both values
                let val1 = pastes.get(key1)?;
                let val2 = pastes.get(key2)?;

                // Swap them atomically
                if let (Some(v1), Some(v2)) = (val1, val2) {
                    pastes.insert(key1, v2)?;
                    pastes.insert(key2, v1)?;
                }

                Ok(())
            })
            .map_err(|e: sled::transaction::TransactionError| {
                AppError::DatabaseError(format!("Swap transaction failed: {}", e))
            })?;

        Ok(())
    }

    /// Example: Batch insert multiple pastes atomically
    #[allow(dead_code)]
    pub fn batch_insert_pastes(
        db: &sled::Db,
        pastes_to_insert: Vec<Paste>,
    ) -> Result<(), AppError> {
        let paste_tree = db.open_tree("pastes")?;

        // Serialize all pastes first (outside transaction for better error handling)
        let serialized: Result<Vec<_>, _> = pastes_to_insert
            .iter()
            .map(|p| bincode::serialize(p).map(|bytes| (p.id.as_bytes().to_vec(), bytes)))
            .collect();

        let serialized = serialized
            .map_err(|e| AppError::DatabaseError(format!("Serialization failed: {}", e)))?;

        // Now do the atomic batch insert
        (&paste_tree,)
            .transaction(|(pastes,)| {
                for (key, value) in &serialized {
                    pastes.insert(key.as_slice(), value.as_slice())?;
                }
                Ok(())
            })
            .map_err(|e: sled::transaction::TransactionError| {
                AppError::DatabaseError(format!("Batch insert failed: {}", e))
            })?;

        Ok(())
    }
}
