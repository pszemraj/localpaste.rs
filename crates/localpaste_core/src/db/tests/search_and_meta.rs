//! Search and metadata index behavior tests.

use super::*;

#[test]
fn test_paste_list_meta_orders_by_updated_and_honors_limit() {
    let (db, _temp) = setup_test_db();
    let now = chrono::Utc::now();

    let mut older = Paste::new("old".to_string(), "old".to_string());
    older.updated_at = now - Duration::minutes(10);
    let older_id = older.id.clone();

    let mut newer = Paste::new("new".to_string(), "new".to_string());
    newer.updated_at = now;
    let newer_id = newer.id.clone();

    db.pastes.create(&older).unwrap();
    db.pastes.create(&newer).unwrap();

    let metas = db.pastes.list_meta(1, None).unwrap();
    assert_eq!(metas.len(), 1);
    assert_eq!(metas[0].id, newer_id);
    assert_ne!(metas[0].id, older_id);
}

#[test]
fn test_paste_search() {
    let (db, _temp) = setup_test_db();

    let paste1 = Paste::new("Rust is awesome".to_string(), "p1".to_string());
    let paste2 = Paste::new("Python is great".to_string(), "p2".to_string());
    let paste3 = Paste::new("JavaScript rocks".to_string(), "p3".to_string());

    db.pastes.create(&paste1).unwrap();
    db.pastes.create(&paste2).unwrap();
    db.pastes.create(&paste3).unwrap();

    // Search
    let results = db.pastes.search("rust", 10, None, None).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, paste1.id);
}

#[test]
fn test_paste_search_limit_keeps_best_ranked_matches() {
    let (db, _temp) = setup_test_db();

    let strongest = Paste::new(
        "no query term in body".to_string(),
        "needle-name".to_string(),
    );
    let medium = Paste::new(
        "contains needle in content".to_string(),
        "plain-a".to_string(),
    );
    let weak = Paste::new("also needle in content".to_string(), "plain-b".to_string());

    db.pastes.create(&strongest).unwrap();
    db.pastes.create(&medium).unwrap();
    db.pastes.create(&weak).unwrap();

    let results = db.pastes.search("needle", 1, None, None).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(
        results[0].id, strongest.id,
        "name/tag hits must outrank content-only hits under top-k limiting"
    );
}

#[test]
fn test_paste_search_meta_is_metadata_only() {
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

    db.pastes.create(&by_name).unwrap();
    db.pastes.create(&by_tag).unwrap();
    db.pastes.create(&content_only).unwrap();

    let results = db.pastes.search_meta("rust", 10, None, None).unwrap();
    let ids: Vec<String> = results.into_iter().map(|m| m.id).collect();
    assert!(ids.contains(&by_name.id));
    assert!(ids.contains(&by_tag.id));
    assert!(!ids.contains(&content_only.id));
}

#[test]
fn test_paste_search_language_filter_is_case_insensitive_and_trimmed() {
    let (db, _temp) = setup_test_db();

    let mut python = Paste::new("def run():\n    pass".to_string(), "py".to_string());
    python.language = Some("python".to_string());
    python.language_is_manual = true;

    let mut rust = Paste::new("fn run() {}".to_string(), "rs".to_string());
    rust.language = Some("rust".to_string());
    rust.language_is_manual = true;

    db.pastes.create(&python).unwrap();
    db.pastes.create(&rust).unwrap();

    let results = db
        .pastes
        .search("run", 10, None, Some("  PyThOn  ".to_string()))
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, python.id);
}

#[test]
fn test_paste_search_meta_language_filter_is_case_insensitive_and_trimmed() {
    let (db, _temp) = setup_test_db();

    let mut python = Paste::new("hello".to_string(), "python-note".to_string());
    python.language = Some("python".to_string());
    python.language_is_manual = true;
    python.tags = vec!["tips".to_string()];

    let mut rust = Paste::new("hello".to_string(), "rust-note".to_string());
    rust.language = Some("rust".to_string());
    rust.language_is_manual = true;
    rust.tags = vec!["tips".to_string()];

    db.pastes.create(&python).unwrap();
    db.pastes.create(&rust).unwrap();

    let results = db
        .pastes
        .search_meta("tips", 10, None, Some(" PYTHON ".to_string()))
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, python.id);
}

#[test]
fn test_paste_search_ignores_empty_or_whitespace_queries() {
    let (db, _temp) = setup_test_db();

    let paste = Paste::new("hello world".to_string(), "note".to_string());
    db.pastes.create(&paste).unwrap();

    let empty = db.pastes.search("", 10, None, None).unwrap();
    assert!(empty.is_empty());

    let whitespace = db.pastes.search("   ", 10, None, None).unwrap();
    assert!(whitespace.is_empty());

    let meta_empty = db.pastes.search_meta("", 10, None, None).unwrap();
    assert!(meta_empty.is_empty());

    let meta_whitespace = db.pastes.search_meta("   ", 10, None, None).unwrap();
    assert!(meta_whitespace.is_empty());
}

#[test]
fn test_meta_indexes_stay_consistent_after_update_and_delete() {
    let (db, _temp) = setup_test_db();
    let paste = Paste::new("one".to_string(), "alpha".to_string());
    let paste_id = paste.id.clone();
    db.pastes.create(&paste).unwrap();

    let update = UpdatePasteRequest {
        content: Some("updated body".to_string()),
        name: Some("beta".to_string()),
        language: None,
        language_is_manual: None,
        folder_id: None,
        tags: Some(vec!["tag".to_string()]),
    };
    db.pastes.update(&paste_id, update).unwrap().unwrap();

    let metas = db.pastes.list_meta(10, None).unwrap();
    let updated = metas.into_iter().find(|m| m.id == paste_id).unwrap();
    assert_eq!(updated.name, "beta");
    assert_eq!(updated.content_len, "updated body".len());
    assert_eq!(updated.tags, vec!["tag".to_string()]);

    db.pastes.delete(&paste_id).unwrap();
    let metas_after_delete = db.pastes.list_meta(10, None).unwrap();
    assert!(!metas_after_delete.into_iter().any(|m| m.id == paste_id));
}

#[test]
fn test_equal_length_index_mismatch_does_not_leak_stale_metadata() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let db_path_str = db_path.to_str().unwrap().to_string();

    let db = Database::new(&db_path_str).unwrap();
    let stale = Paste::new("stale body".to_string(), "alpha-stale".to_string());
    let stale_id = stale.id.clone();
    db.pastes.create(&stale).unwrap();

    let fresh = Paste::new("fresh body".to_string(), "beta-fresh".to_string());
    let fresh_id = fresh.id.clone();
    let canonical_tree = db.db.open_tree("pastes").unwrap();
    canonical_tree.remove(stale_id.as_bytes()).unwrap();
    canonical_tree
        .insert(
            fresh_id.as_bytes(),
            bincode::serialize(&fresh).expect("serialize fresh"),
        )
        .unwrap();
    drop(canonical_tree);
    drop(db);

    let reopened = Database::new(&db_path_str).unwrap();
    assert!(
        !reopened
            .pastes
            .needs_reconcile_meta_indexes(false)
            .expect("needs reconcile"),
        "startup marker/length checks currently miss equal-length semantic mismatches"
    );

    let listed = reopened.pastes.list_meta(10, None).expect("list meta");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, fresh_id);
    assert_eq!(listed[0].name, "beta-fresh");

    let stale_search = reopened
        .pastes
        .search_meta("alpha-stale", 10, None, None)
        .expect("search stale");
    assert!(
        stale_search.is_empty(),
        "stale metadata should not leak through search results"
    );

    let fresh_search = reopened
        .pastes
        .search_meta("beta-fresh", 10, None, None)
        .expect("search fresh");
    assert_eq!(fresh_search.len(), 1);
    assert_eq!(fresh_search[0].id, fresh_id);
}
