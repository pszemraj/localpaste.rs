//! Shared test-only helpers for localpaste_core.

#![cfg(test)]

use crate::Database;
use std::collections::HashMap;

pub(crate) fn assert_folder_counts_match_canonical(db: &Database) {
    let mut canonical_counts: HashMap<String, usize> = HashMap::new();
    db.pastes
        .scan_canonical_meta(|meta| {
            if let Some(folder_id) = meta.folder_id {
                *canonical_counts.entry(folder_id).or_insert(0) += 1;
            }
            Ok(())
        })
        .expect("scan canonical meta");

    for folder in db.folders.list().expect("list folders") {
        let expected = canonical_counts.remove(folder.id.as_str()).unwrap_or(0);
        assert_eq!(
            folder.paste_count, expected,
            "folder count drift for folder {}",
            folder.id
        );
    }
    assert!(
        canonical_counts.is_empty(),
        "canonical rows must not reference missing folders: {:?}",
        canonical_counts.keys().collect::<Vec<_>>()
    );
}
