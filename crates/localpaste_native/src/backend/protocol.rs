//! Protocol types for the native GUI backend worker.

use localpaste_core::models::paste::Paste;

/// Commands issued by the UI thread for the backend worker to execute.
#[derive(Debug)]
pub enum CoreCmd {
    /// Fetch a snapshot of the most recent pastes, capped by `limit`.
    ListAll { limit: usize },
    /// Load a single paste by id for display in the editor pane.
    GetPaste { id: String },
}

/// Events produced by the backend worker and polled by the UI thread.
#[derive(Debug)]
pub enum CoreEvent {
    /// Response containing the current paste list snapshot.
    PasteList { items: Vec<PasteSummary> },
    /// Response containing the full paste payload requested by id.
    PasteLoaded { paste: Paste },
    /// The requested paste id no longer exists in the database.
    PasteMissing { id: String },
    /// A backend failure occurred (database error, etc).
    Error { message: String },
}

/// Lightweight summary used for list rendering in the UI.
#[derive(Debug, Clone)]
pub struct PasteSummary {
    pub id: String,
    pub name: String,
    pub language: Option<String>,
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
        }
    }
}
