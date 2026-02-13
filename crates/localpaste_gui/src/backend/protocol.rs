//! Protocol types for the native GUI backend worker.

use chrono::{DateTime, Utc};
use localpaste_core::models::{
    folder::Folder,
    paste::{Paste, PasteMeta},
};

/// Commands issued by the UI thread for the backend worker to execute.
#[derive(Debug)]
pub enum CoreCmd {
    /// Fetch a snapshot of the most recent pastes, capped by `limit`.
    ListPastes {
        limit: usize,
        folder_id: Option<String>,
    },
    /// Search pastes with optional folder/language filters.
    SearchPastes {
        query: String,
        limit: usize,
        folder_id: Option<String>,
        language: Option<String>,
    },
    /// Load a single paste by id for display in the editor pane.
    GetPaste { id: String },
    /// Create a new paste with the provided content.
    CreatePaste { content: String },
    /// Persist updated content for an existing paste.
    UpdatePaste { id: String, content: String },
    /// Persist metadata changes for an existing paste.
    UpdatePasteMeta {
        id: String,
        name: Option<String>,
        language: Option<String>,
        language_is_manual: Option<bool>,
        folder_id: Option<String>,
        tags: Option<Vec<String>>,
    },
    /// Delete a paste by id.
    DeletePaste { id: String },
    /// Load all folders.
    ListFolders,
    /// Create a folder with optional parent.
    CreateFolder {
        name: String,
        parent_id: Option<String>,
    },
    /// Rename/re-parent a folder.
    UpdateFolder {
        id: String,
        name: String,
        /// `None` keeps current parent, `Some("")` clears parent, `Some(id)` re-parents.
        parent_id: Option<String>,
    },
    /// Delete a folder tree and migrate contained pastes to unfiled.
    DeleteFolder { id: String },
}

/// Events produced by the backend worker and polled by the UI thread.
#[derive(Debug)]
pub enum CoreEvent {
    /// Response containing the current paste list snapshot.
    PasteList { items: Vec<PasteSummary> },
    /// Response containing ranked search results.
    SearchResults {
        query: String,
        items: Vec<PasteSummary>,
    },
    /// Response containing the full paste payload requested by id.
    PasteLoaded { paste: Paste },
    /// Response containing a newly created paste.
    PasteCreated { paste: Paste },
    /// Response confirming a paste was updated.
    PasteSaved { paste: Paste },
    /// Response confirming a paste's metadata was updated.
    PasteMetaSaved { paste: Paste },
    /// Response confirming a paste was deleted.
    PasteDeleted { id: String },
    /// The requested paste id no longer exists in the database.
    PasteMissing { id: String },
    /// Response containing current folder list.
    FoldersLoaded { items: Vec<Folder> },
    /// Response confirming a folder was created/updated.
    FolderSaved { folder: Folder },
    /// Response confirming a folder tree was deleted.
    FolderDeleted { id: String },
    /// A backend failure occurred (database error, etc).
    Error { message: String },
}

/// Lightweight summary used for list rendering in the UI.
#[derive(Debug, Clone)]
pub struct PasteSummary {
    pub id: String,
    pub name: String,
    pub language: Option<String>,
    pub content_len: usize,
    pub updated_at: DateTime<Utc>,
    pub folder_id: Option<String>,
    pub tags: Vec<String>,
}

impl PasteSummary {
    /// Build a summary from a full paste record.
    ///
    /// # Returns
    /// A lightweight struct containing the fields needed to render list rows.
    pub fn from_paste(paste: &Paste) -> Self {
        Self {
            id: paste.id.clone(),
            name: paste.name.clone(),
            language: paste.language.clone(),
            content_len: paste.content.len(),
            updated_at: paste.updated_at,
            folder_id: paste.folder_id.clone(),
            tags: paste.tags.clone(),
        }
    }

    /// Build a summary from a metadata record.
    ///
    /// # Returns
    /// A list-row payload containing metadata-only fields.
    pub fn from_meta(meta: &PasteMeta) -> Self {
        Self {
            id: meta.id.clone(),
            name: meta.name.clone(),
            language: meta.language.clone(),
            content_len: meta.content_len,
            updated_at: meta.updated_at,
            folder_id: meta.folder_id.clone(),
            tags: meta.tags.clone(),
        }
    }
}
