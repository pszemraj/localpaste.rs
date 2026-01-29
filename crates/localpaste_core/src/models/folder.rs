//! Folder-related data models.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Folder metadata stored in the database and returned by the API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Folder {
    pub id: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub paste_count: usize,
    #[serde(default)]
    pub parent_id: Option<String>,
}

/// Request payload for creating a folder.
#[derive(Debug, Deserialize)]
pub struct CreateFolderRequest {
    pub name: String,
    #[serde(default)]
    pub parent_id: Option<String>,
}

/// Request payload for updating a folder.
#[derive(Debug, Deserialize)]
pub struct UpdateFolderRequest {
    pub name: String,
    #[serde(default)]
    pub parent_id: Option<String>,
}

impl Folder {
    /// Create a new folder with no parent.
    ///
    /// # Returns
    /// A new [`Folder`] with generated id.
    pub fn new(name: String) -> Self {
        Self::with_parent(name, None)
    }

    /// Create a new folder with an optional parent.
    ///
    /// # Arguments
    /// - `name`: Folder display name.
    /// - `parent_id`: Optional parent folder id.
    ///
    /// # Returns
    /// A new [`Folder`] with generated id.
    pub fn with_parent(name: String, parent_id: Option<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name,
            created_at: Utc::now(),
            paste_count: 0,
            parent_id,
        }
    }
}
