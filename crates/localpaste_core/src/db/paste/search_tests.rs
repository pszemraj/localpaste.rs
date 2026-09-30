//! Scoped search fixtures, including proof that metadata scopes never read bodies.

use super::*;
use tempfile::TempDir;

#[test]
fn scopes_match_only_their_fields_and_preserve_case_policy() {
    let dir = TempDir::new().unwrap();
    let db = crate::Database::new(dir.path().to_str().unwrap()).unwrap();
    let title = Paste::new("ordinary content".into(), "TitleNeedle".into());
    let mut metadata = Paste::new("different ordinary content".into(), "second".into());
    metadata.tags = vec!["TagNeedle".into()];
    let body = Paste::new("BodyNeedle some raw text".into(), "third".into());
    for paste in [&title, &metadata, &body] {
        db.pastes.create(paste).unwrap();
    }
    let search = |query, scope, case_sensitive| {
        db.pastes
            .search_scoped_with_options(
                query,
                100,
                None,
                None,
                SearchOptions { case_sensitive },
                scope,
            )
            .unwrap()
    };
    for (query, scope, id) in [
        ("titleneedle", SearchScope::Title, &title.id),
        ("tagneedle", SearchScope::Metadata, &metadata.id),
        ("bodyneedle", SearchScope::Body, &body.id),
    ] {
        assert_eq!(search(query, scope, false)[0].id, *id);
        assert_eq!(search(query, SearchScope::All, false)[0].id, *id);
        assert!(search(query, scope, true).is_empty());
    }
    assert!(search("TagNeedle", SearchScope::Title, true).is_empty());
    assert!(search("TitleNeedle", SearchScope::Body, true).is_empty());
    assert!(search("BodyNeedle", SearchScope::Title, true).is_empty());
    assert_eq!(search("TitleNeedle", SearchScope::Title, true).len(), 1);
    // Metadata deliberately includes the existing body-derived projection.
    assert_eq!(search("bodyneedle", SearchScope::Metadata, false).len(), 1);
}

#[test]
fn title_and_metadata_scopes_do_not_deserialize_canonical_bodies() {
    let dir = TempDir::new().unwrap();
    let raw = Arc::new(redb::Database::create(dir.path().join("test.redb")).unwrap());
    let db = PasteDb::new(raw.clone()).unwrap();
    let paste = Paste::new("payload".into(), "Find this title".into());
    db.create(&paste).unwrap();
    let txn = raw.begin_write().unwrap();
    txn.open_table(PASTES)
        .unwrap()
        .insert(paste.id.as_str(), &[255_u8][..])
        .unwrap();
    txn.commit().unwrap();
    for scope in [SearchScope::Title, SearchScope::Metadata] {
        let hits = db
            .search_scoped_with_options("title", 10, None, None, SearchOptions::default(), scope)
            .unwrap();
        assert_eq!(hits[0].id, paste.id);
    }
    assert!(db
        .search_scoped_with_options(
            "payload",
            10,
            None,
            None,
            SearchOptions::default(),
            SearchScope::Body
        )
        .is_err());
}
