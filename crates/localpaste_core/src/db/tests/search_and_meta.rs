//! Search and metadata index behavior tests.

use super::*;
use crate::db::paste::{CURRENT_PASTES_META_SCHEMA_VERSION, META_SCHEMA_VERSION_KEY};
use crate::db::tables::{PASTES_META, PASTES_META_STATE};
use chrono::Duration;
use redb::ReadableDatabase;
use serde::{Deserialize, Serialize};

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
fn paste_search_meta_uses_persisted_metadata_and_derived_terms() {
    let (db, _temp) = setup_test_db();

    let mut by_name = Paste::new("hello".to_string(), "rust-note".to_string());
    by_name.language = None;
    by_name.tags = vec![];

    let mut by_tag = Paste::new("hello".to_string(), "misc".to_string());
    by_tag.language = None;
    by_tag.tags = vec!["rusty".to_string()];

    let mut by_derived = Paste::new("rust appears in content".to_string(), "plain".to_string());
    by_derived.language = None;
    by_derived.tags = vec![];

    db.pastes.create(&by_name).expect("create");
    db.pastes.create(&by_tag).expect("create");
    db.pastes.create(&by_derived).expect("create");

    let results = db
        .pastes
        .search_meta("rust", 10, None, None)
        .expect("search");
    let ids: Vec<String> = results.into_iter().map(|m| m.id).collect();
    assert!(ids.contains(&by_name.id));
    assert!(ids.contains(&by_tag.id));
    assert!(ids.contains(&by_derived.id));
}

#[test]
fn paste_search_meta_multi_term_queries_rank_combined_metadata_hits() {
    let (db, _temp) = setup_test_db();

    let mut combined = Paste::new("body".to_string(), "docker compose".to_string());
    combined.language = Some("yaml".to_string());
    combined.language_is_manual = true;
    combined.tags = vec!["postgres".to_string()];

    let mut partial = Paste::new("body".to_string(), "docker".to_string());
    partial.language = Some("yaml".to_string());
    partial.language_is_manual = true;
    partial.tags = vec!["misc".to_string()];

    let mut weak = Paste::new("body".to_string(), "plain".to_string());
    weak.language = Some("yaml".to_string());
    weak.language_is_manual = true;
    weak.tags = vec!["postgres".to_string()];

    db.pastes.create(&combined).expect("create combined");
    db.pastes.create(&partial).expect("create partial");
    db.pastes.create(&weak).expect("create weak");

    let results = db
        .pastes
        .search_meta("docker postgres", 10, None, None)
        .expect("search");
    let ids: Vec<String> = results.into_iter().map(|meta| meta.id).collect();

    assert_eq!(ids.first().map(String::as_str), Some(combined.id.as_str()));
    assert!(ids.iter().position(|id| id == &partial.id) < ids.iter().position(|id| id == &weak.id));
}

#[test]
fn paste_search_meta_matches_derived_handle_and_terms() {
    let (db, _temp) = setup_test_db();

    let handle = Paste::new(
        "cargo test --package trainer\n".to_string(),
        "untamed-tundra".to_string(),
    );
    let terms = Paste::new(
        "fsdp2 validation failed after cublaslt retry\nfsdp2 validation repeated\n".to_string(),
        "silent-forest".to_string(),
    );
    let tag_only = Paste::new("body".to_string(), "misc".to_string());
    let mut tag_only = tag_only;
    tag_only.tags = vec!["fsdp2".to_string()];

    db.pastes.create(&handle).expect("create handle");
    db.pastes.create(&terms).expect("create terms");
    db.pastes.create(&tag_only).expect("create tag");

    let handle_results = db
        .pastes
        .search_meta("cargo test", 10, None, None)
        .expect("search");
    assert_eq!(
        handle_results.first().map(|meta| meta.id.as_str()),
        Some(handle.id.as_str())
    );

    let term_results = db
        .pastes
        .search_meta("fsdp2 cublaslt", 10, None, None)
        .expect("search");
    let ids: Vec<String> = term_results.into_iter().map(|meta| meta.id).collect();
    assert_eq!(ids.first().map(String::as_str), Some(terms.id.as_str()));
    assert!(ids.iter().any(|id| id == &tag_only.id));
}

#[test]
fn paste_search_meta_keeps_name_above_tags_above_language() {
    let (db, _temp) = setup_test_db();

    let mut by_name = Paste::new("body".to_string(), "python-guide".to_string());
    by_name.language = None;
    by_name.tags = Vec::new();

    let mut by_tag = Paste::new("body".to_string(), "misc".to_string());
    by_tag.language = None;
    by_tag.tags = vec!["python".to_string()];

    let mut by_language = Paste::new("body".to_string(), "plain".to_string());
    by_language.language = Some("python".to_string());
    by_language.language_is_manual = true;
    by_language.tags = Vec::new();

    db.pastes.create(&by_name).expect("create by_name");
    db.pastes.create(&by_tag).expect("create by_tag");
    db.pastes.create(&by_language).expect("create by_language");

    let results = db
        .pastes
        .search_meta("python", 10, None, None)
        .expect("search");
    let ids: Vec<String> = results.into_iter().map(|meta| meta.id).collect();

    assert_eq!(ids.first().map(String::as_str), Some(by_name.id.as_str()));
    assert!(
        ids.iter().position(|id| id == &by_tag.id)
            < ids.iter().position(|id| id == &by_language.id)
    );
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

#[derive(Serialize, Deserialize)]
struct LegacyPasteMetaWire {
    id: String,
    name: String,
    language: Option<String>,
    folder_id: Option<String>,
    updated_at: chrono::DateTime<chrono::Utc>,
    tags: Vec<String>,
    content_len: usize,
    is_markdown: bool,
}

#[test]
fn database_new_rebuilds_legacy_meta_rows_with_derived_fields() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let db_path = temp_dir.path().join("db");
    let db_path_str = db_path.to_str().expect("db path").to_string();

    let db = open_test_database(&db_path_str);
    let paste = Paste::new(
        "cargo test --package trainer\n".to_string(),
        "legacy-meta".to_string(),
    );
    let paste_id = paste.id.clone();
    db.pastes.create(&paste).expect("create");

    let legacy_meta = LegacyPasteMetaWire {
        id: paste_id.clone(),
        name: paste.name.clone(),
        language: paste.language.clone(),
        folder_id: None,
        updated_at: paste.updated_at,
        tags: Vec::new(),
        content_len: paste.content.len(),
        is_markdown: paste.is_markdown,
    };
    let encoded = bincode::serialize(&legacy_meta).expect("serialize");
    let write_txn = db.db.begin_write().expect("begin write");
    {
        let mut metas = write_txn.open_table(PASTES_META).expect("open metas");
        let mut meta_state = write_txn
            .open_table(PASTES_META_STATE)
            .expect("open meta state");
        metas
            .insert(paste_id.as_str(), encoded.as_slice())
            .expect("overwrite legacy meta");
        let _ = meta_state
            .remove(META_SCHEMA_VERSION_KEY)
            .expect("remove schema marker");
    }
    write_txn.commit().expect("commit");
    drop(db);

    let reopened = open_test_database(&db_path_str);
    let meta = reopened
        .pastes
        .list_meta(10, None)
        .expect("list")
        .into_iter()
        .find(|meta| meta.id == paste_id)
        .expect("meta row");
    assert_eq!(meta.derived.kind, crate::semantic::PasteKind::Code);
    assert_eq!(meta.derived.handle.as_deref(), Some("cargo test"));
}

#[test]
fn database_new_rebuilds_markerless_current_meta_rows() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let db_path = temp_dir.path().join("db");
    let db_path_str = db_path.to_str().expect("db path").to_string();

    let db = open_test_database(&db_path_str);
    let paste = Paste::new(
        "cargo test --package trainer\n".to_string(),
        "stable-meta".to_string(),
    );
    let paste_id = paste.id.clone();
    db.pastes.create(&paste).expect("create");

    let mut current_meta = PasteMeta::from(&paste);
    current_meta.derived = crate::semantic::DerivedMeta {
        kind: crate::semantic::PasteKind::Other,
        handle: Some("frozen-handle".to_string()),
        terms: vec!["frozen-term".to_string()],
    };
    let encoded = bincode::serialize(&current_meta).expect("serialize");
    let write_txn = db.db.begin_write().expect("begin write");
    {
        let mut metas = write_txn.open_table(PASTES_META).expect("open metas");
        let mut meta_state = write_txn
            .open_table(PASTES_META_STATE)
            .expect("open meta state");
        metas
            .insert(paste_id.as_str(), encoded.as_slice())
            .expect("overwrite current meta");
        let _ = meta_state
            .remove(META_SCHEMA_VERSION_KEY)
            .expect("remove schema marker");
    }
    write_txn.commit().expect("commit");
    drop(db);

    let reopened = open_test_database(&db_path_str);
    let first_meta = reopened
        .pastes
        .list_meta(10, None)
        .expect("list")
        .into_iter()
        .find(|meta| meta.id == paste_id)
        .expect("meta row");
    assert_eq!(first_meta.derived.kind, crate::semantic::PasteKind::Code);
    assert_eq!(first_meta.derived.handle.as_deref(), Some("cargo test"));
    assert!(
        !first_meta
            .derived
            .terms
            .iter()
            .any(|term| term == "frozen-term"),
        "marker-less databases must rebuild derived metadata before stamping it current"
    );

    let read_txn = reopened.db.begin_read().expect("begin read");
    let meta_state = read_txn
        .open_table(PASTES_META_STATE)
        .expect("open meta state");
    let stored_version = meta_state
        .get(META_SCHEMA_VERSION_KEY)
        .expect("schema lookup")
        .expect("schema row");
    let stored_version: u64 =
        bincode::deserialize(stored_version.value()).expect("decode schema version");
    assert_eq!(stored_version, CURRENT_PASTES_META_SCHEMA_VERSION);
    drop(read_txn);
    drop(reopened);

    let reopened_again = open_test_database(&db_path_str);
    let second_meta = reopened_again
        .pastes
        .list_meta(10, None)
        .expect("list again")
        .into_iter()
        .find(|meta| meta.id == paste_id)
        .expect("meta row");
    assert_eq!(second_meta.derived.kind, crate::semantic::PasteKind::Code);
    assert_eq!(second_meta.derived.handle.as_deref(), Some("cargo test"));
    assert!(
        !second_meta
            .derived
            .terms
            .iter()
            .any(|term| term == "frozen-term"),
        "rebuilt metadata should remain current on subsequent opens"
    );
}
