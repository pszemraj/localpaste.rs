pub mod paste;
pub mod folder;

use crate::error::AppError;
use sled::Db;
use std::sync::Arc;

pub struct Database {
    db: Arc<Db>,
    pub pastes: paste::PasteDb,
    pub folders: folder::FolderDb,
}

impl Database {
    pub fn new(path: &str) -> Result<Self, AppError> {
        let db = Arc::new(sled::open(path)?);
        
        Ok(Self {
            pastes: paste::PasteDb::new(db.clone())?,
            folders: folder::FolderDb::new(db.clone())?,
            db,
        })
    }
}