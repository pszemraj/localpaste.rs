pub mod folder;
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

impl Database {
    pub fn new(path: &str) -> Result<Self, AppError> {
        // Try to open the database with retries for lock issues
        let db = match sled::open(path) {
            Ok(db) => Arc::new(db),
            Err(e) if e.to_string().contains("WouldBlock") => {
                // Database is locked, try to recover
                eprintln!("Database appears to be locked. Attempting recovery...");
                std::thread::sleep(std::time::Duration::from_millis(100));
                
                // Try once more after a brief delay
                match sled::open(path) {
                    Ok(db) => Arc::new(db),
                    Err(_) => {
                        // If still locked, clear and recreate
                        eprintln!("Could not acquire lock. Creating fresh database...");
                        let _ = std::fs::remove_dir_all(path);
                        Arc::new(sled::open(path)?)
                    }
                }
            }
            Err(e) => return Err(AppError::Database(e)),
        };

        Ok(Self {
            pastes: paste::PasteDb::new(db.clone())?,
            folders: folder::FolderDb::new(db.clone())?,
            db,
        })
    }
}
