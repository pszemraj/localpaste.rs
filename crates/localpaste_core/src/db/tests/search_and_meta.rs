//! Search and metadata index behavior tests.

use super::*;
use chrono::Duration;

#[test]
fn paste_list_and_list_meta_order_by_updated_and_honor_limit() {
    let (db, _temp) = setup_test_db();
    let now = chrono::Utc::now();

    let mut older = Paste::new("old".to_string(), "old".to_string());
    older.updated_at = now - Duration::minutes(10);
    let older_id = older.id.clone();

    let mut newer = Paste::new("new".to_string(), "new".to_string());
    newer.updated_at = now;
    let newer_id = newer.id.clone();

    db.pastes.create(&older).expect("create older");
    db.pastes.create(&newer).expect("create newer");

    let rows = db.pastes.list(1, None).expect("list canonical");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, newer_id);
    assert_ne!(rows[0].id, older_id);

    let metas = db.pastes.list_meta(1, None).expect("list");
    assert_eq!(metas.len(), 1);
    assert_eq!(metas[0].id, newer_id);
    assert_ne!(metas[0].id, older_id);
}

#[test]
fn paste_search_respects_exact_match_and_top_k_ranking() {
    enum SearchCase {
        ExactMatch,
        TopKRanking,
    }

    let cases = [SearchCase::ExactMatch, SearchCase::TopKRanking];
    for case in cases {
        let (db, _temp) = setup_test_db();
        match case {
            SearchCase::ExactMatch => {
                let paste1 = Paste::new("Rust is awesome".to_string(), "p1".to_string());
                let paste2 = Paste::new("Python is great".to_string(), "p2".to_string());
                let paste3 = Paste::new("JavaScript rocks".to_string(), "p3".to_string());

                db.pastes.create(&paste1).expect("create");
                db.pastes.create(&paste2).expect("create");
                db.pastes.create(&paste3).expect("create");

                let results = db.pastes.search("rust", 10, None, None).expect("search");
                assert_eq!(results.len(), 1);
                assert_eq!(results[0].id, paste1.id);
            }
            SearchCase::TopKRanking => {
                let strongest = Paste::new(
                    "no query term in body".to_string(),
                    "needle-name".to_string(),
                );
                let medium = Paste::new(
                    "contains needle in content".to_string(),
                    "plain-a".to_string(),
                );
                let weak = Paste::new("also needle in content".to_string(), "plain-b".to_string());

                db.pastes.create(&strongest).expect("create");
                db.pastes.create(&medium).expect("create");
                db.pastes.create(&weak).expect("create");

                let results = db.pastes.search("needle", 1, None, None).expect("search");
                assert_eq!(results.len(), 1);
                assert_eq!(results[0].id, strongest.id);
            }
        }
    }
}

#[test]
fn paste_search_meta_is_metadata_only() {
    let (db, _temp) = setup_test_db();

    let mut by_name = Paste::new("hello".to_string(), "rust-note".to_string());
    by_name.language = None;
    by_name.tags = vec![];

    let mut by_tag = Paste::new("hello".to_string(), "misc".to_string());
    by_tag.language = None;
    by_tag.tags = vec!["rusty".to_string()];

    let mut content_only = Paste::new("rust appears in content".to_string(), "plain".to_string());
    content_only.language = None;
    content_only.tags = vec![];

    db.pastes.create(&by_name).expect("create");
    db.pastes.create(&by_tag).expect("create");
    db.pastes.create(&content_only).expect("create");

    let results = db
        .pastes
        .search_meta("rust", 10, None, None)
        .expect("search");
    let ids: Vec<String> = results.into_iter().map(|m| m.id).collect();
    assert!(ids.contains(&by_name.id));
    assert!(ids.contains(&by_tag.id));
    assert!(!ids.contains(&content_only.id));
}

#[test]
fn search_language_filters_are_case_insensitive_and_trimmed_for_full_and_meta_queries() {
    enum SearchKind {
        Full,
        Meta,
    }

    let cases = [SearchKind::Full, SearchKind::Meta];
    for case in cases {
        let (db, _temp) = setup_test_db();
        match case {
            SearchKind::Full => {
                let mut python = Paste::new("def run():\n    pass".to_string(), "py".to_string());
                python.language = Some("python".to_string());
                python.language_is_manual = true;

                let mut rust = Paste::new("fn run() {}".to_string(), "rs".to_string());
                rust.language = Some("rust".to_string());
                rust.language_is_manual = true;

                db.pastes.create(&python).expect("create");
                db.pastes.create(&rust).expect("create");

                let results = db
                    .pastes
                    .search("run", 10, None, Some("  PyThOn  ".to_string()))
                    .expect("search");
                assert_eq!(results.len(), 1);
                assert_eq!(results[0].id, python.id);
            }
            SearchKind::Meta => {
                let mut python = Paste::new("hello".to_string(), "python-note".to_string());
                python.language = Some("python".to_string());
                python.language_is_manual = true;
                python.tags = vec!["tips".to_string()];

                let mut rust = Paste::new("hello".to_string(), "rust-note".to_string());
                rust.language = Some("rust".to_string());
                rust.language_is_manual = true;
                rust.tags = vec!["tips".to_string()];

                db.pastes.create(&python).expect("create");
                db.pastes.create(&rust).expect("create");

                let results = db
                    .pastes
                    .search_meta("tips", 10, None, Some(" PYTHON ".to_string()))
                    .expect("search");
                assert_eq!(results.len(), 1);
                assert_eq!(results[0].id, python.id);
            }
        }
    }
}

#[test]
fn paste_search_ignores_empty_or_whitespace_queries() {
    let (db, _temp) = setup_test_db();
    let paste = Paste::new("hello world".to_string(), "note".to_string());
    db.pastes.create(&paste).expect("create");

    let empty = db.pastes.search("", 10, None, None).expect("search");
    assert!(empty.is_empty());

    let whitespace = db.pastes.search("   ", 10, None, None).expect("search");
    assert!(whitespace.is_empty());

    let meta_empty = db.pastes.search_meta("", 10, None, None).expect("search");
    assert!(meta_empty.is_empty());

    let meta_whitespace = db
        .pastes
        .search_meta("   ", 10, None, None)
        .expect("search");
    assert!(meta_whitespace.is_empty());
}

#[test]
fn meta_indexes_stay_consistent_after_update_and_delete() {
    let (db, _temp) = setup_test_db();
    let paste = Paste::new("one".to_string(), "alpha".to_string());
    let paste_id = paste.id.clone();
    db.pastes.create(&paste).expect("create");

    let update = UpdatePasteRequest {
        content: Some("updated body".to_string()),
        name: Some("beta".to_string()),
        language: None,
        language_is_manual: None,
        folder_id: None,
        tags: Some(vec!["tag".to_string()]),
    };
    db.pastes
        .update(&paste_id, update)
        .expect("update")
        .expect("row");

    let metas = db.pastes.list_meta(10, None).expect("list");
    let updated = metas.into_iter().find(|m| m.id == paste_id).expect("meta");
    assert_eq!(updated.name, "beta");
    assert_eq!(updated.content_len, "updated body".len());
    assert_eq!(updated.tags, vec!["tag".to_string()]);

    db.pastes.delete(&paste_id).expect("delete");
    let metas_after_delete = db.pastes.list_meta(10, None).expect("list");
    assert!(!metas_after_delete.into_iter().any(|m| m.id == paste_id));
}
