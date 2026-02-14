//! redb table definitions shared by storage modules.

use redb::TableDefinition;

/// File name for the redb database within the configured DB directory.
pub const REDB_FILE_NAME: &str = "data.redb";

/// Canonical paste rows (`Paste`, bincode-encoded).
pub const PASTES: TableDefinition<&str, &[u8]> = TableDefinition::new("pastes");
/// Paste metadata rows (`PasteMeta`, bincode-encoded).
pub const PASTES_META: TableDefinition<&str, &[u8]> = TableDefinition::new("pastes_meta");
/// Canonical folder rows (`Folder`, bincode-encoded).
pub const FOLDERS: TableDefinition<&str, &[u8]> = TableDefinition::new("folders");

/// Recency index ordered by reverse-millis then id.
pub const PASTES_BY_UPDATED: TableDefinition<(u64, &str), ()> =
    TableDefinition::new("pastes_by_updated");
/// In-progress folder-delete markers.
pub const FOLDERS_DELETING: TableDefinition<&str, ()> = TableDefinition::new("folders_deleting");
