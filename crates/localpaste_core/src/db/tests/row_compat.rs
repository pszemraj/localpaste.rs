//! Database compatibility tests for canonical and projection row shapes.

use super::*;
use crate::db::{
    paste::{CURRENT_PASTES_META_SCHEMA_VERSION, META_SCHEMA_VERSION_KEY},
    tables::{PASTES, PASTES_BY_UPDATED, PASTES_META, PASTES_META_STATE},
};
use crate::semantic::DerivedMeta;
use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Debug, Serialize)]
struct LegacyPasteForTest {
    id: String,
    name: String,
    content: String,
    language: Option<String>,
    folder_id: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    tags: Vec<String>,
    is_markdown: bool,
}

#[derive(Debug, Serialize)]
struct LegacyPasteMetaForTest {
    id: String,
    name: String,
    language: Option<String>,
    folder_id: Option<String>,
    updated_at: DateTime<Utc>,
    tags: Vec<String>,
    content_len: usize,
    is_markdown: bool,
}

#[test]
fn get_and_update_decode_legacy_paste_rows_without_manual_language_flag() {
    let (db, _temp) = setup_test_db();
    let now = Utc::now();
    let legacy = LegacyPasteForTest {
        id: "legacy-paste-row".to_string(),
        name: "legacy paste".to_string(),
        content: "print('hello')\n".to_string(),
        language: Some("python".to_string()),
        folder_id: None,
        created_at: now,
        updated_at: now,
        tags: Vec::new(),
        is_markdown: false,
    };
    let current_for_meta = Paste {
        id: legacy.id.clone(),
        name: legacy.name.clone(),
        content: legacy.content.clone(),
        language: legacy.language.clone(),
        language_is_manual: false,
        folder_id: legacy.folder_id.clone(),
        created_at: legacy.created_at,
        updated_at: legacy.updated_at,
        tags: legacy.tags.clone(),
        is_markdown: legacy.is_markdown,
    };
    let encoded_legacy = bincode::serialize(&legacy).expect("serialize legacy paste");
    let encoded_meta = bincode::serialize(&PasteMeta::from(&current_for_meta)).expect("meta");
    let recency_key = crate::db::paste::reverse_timestamp_key(now);
    let write_txn = db.db.begin_write().expect("begin write");
    {
        let mut pastes = write_txn.open_table(PASTES).expect("open pastes");
        let mut metas = write_txn.open_table(PASTES_META).expect("open metas");
        let mut updated = write_txn
            .open_table(PASTES_BY_UPDATED)
            .expect("open recency index");
        pastes
            .insert(legacy.id.as_str(), encoded_legacy.as_slice())
            .expect("insert legacy paste");
        metas
            .insert(legacy.id.as_str(), encoded_meta.as_slice())
            .expect("insert meta");
        updated
            .insert((recency_key, legacy.id.as_str()), ())
            .expect("insert recency index");
    }
    write_txn.commit().expect("commit legacy paste");

    let loaded = db
        .pastes
        .get(&legacy.id)
        .expect("load legacy paste")
        .expect("paste exists");
    assert_eq!(loaded.language.as_deref(), Some("python"));
    assert!(!loaded.language_is_manual);

    let updated = update_existing_paste(
        &db,
        &legacy.id,
        update_request(
            Some("fn main() { println!(\"hello\"); }\n"),
            None,
            None,
            None,
        ),
        "update legacy paste",
    );
    assert_eq!(updated.language.as_deref(), Some("rust"));
    assert!(updated.language_is_manual);
}

#[test]
fn list_and_search_meta_decode_legacy_projection_rows_without_derived_fields() {
    let (db, _temp) = setup_test_db();
    let paste = Paste::new(
        "legacy projection body".to_string(),
        "legacy-meta".to_string(),
    );
    let paste_id = paste.id.clone();
    db.pastes.create(&paste).expect("create paste");

    let legacy_meta = LegacyPasteMetaForTest {
        id: paste_id.clone(),
        name: "legacy projection".to_string(),
        language: Some("python".to_string()),
        folder_id: None,
        updated_at: paste.updated_at,
        tags: vec!["compat".to_string()],
        content_len: paste.content.len(),
        is_markdown: false,
    };
    let encoded_meta = bincode::serialize(&legacy_meta).expect("serialize legacy meta");
    let encoded_schema =
        bincode::serialize(&CURRENT_PASTES_META_SCHEMA_VERSION).expect("schema version");
    let write_txn = db.db.begin_write().expect("begin write");
    {
        let mut metas = write_txn.open_table(PASTES_META).expect("open metas");
        let mut meta_state = write_txn
            .open_table(PASTES_META_STATE)
            .expect("open meta state");
        metas
            .insert(paste_id.as_str(), encoded_meta.as_slice())
            .expect("insert legacy meta");
        meta_state
            .insert(META_SCHEMA_VERSION_KEY, encoded_schema.as_slice())
            .expect("stamp schema current");
    }
    write_txn.commit().expect("commit legacy meta");

    let listed = db
        .pastes
        .list_meta(10, None)
        .expect("list metadata")
        .into_iter()
        .find(|meta| meta.id == paste_id)
        .expect("legacy meta row");
    assert_eq!(listed.name, "legacy projection");
    assert_eq!(listed.derived, DerivedMeta::default());

    let searched = db
        .pastes
        .search_meta("compat", 10, None, None)
        .expect("search metadata");
    assert_eq!(
        searched.first().map(|meta| meta.id.as_str()),
        Some(paste_id.as_str())
    );
    assert_eq!(searched[0].derived, DerivedMeta::default());
}
