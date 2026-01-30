//! Protocol types for the native GUI backend worker.

use localpaste_core::models::paste::Paste;

#[derive(Debug)]
pub enum CoreCmd {
    ListAll { limit: usize },
    GetPaste { id: String },
}

#[derive(Debug)]
pub enum CoreEvent {
    PasteList { items: Vec<PasteSummary> },
    PasteLoaded { paste: Paste },
    PasteMissing { id: String },
    Error { message: String },
}

#[derive(Debug, Clone)]
pub struct PasteSummary {
    pub id: String,
    pub name: String,
    pub language: Option<String>,
}

impl PasteSummary {
    pub fn from_paste(paste: &Paste) -> Self {
        Self {
            id: paste.id.clone(),
            name: paste.name.clone(),
            language: paste.language.clone(),
        }
    }
}
