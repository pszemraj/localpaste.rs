//! Unit tests for paste storage operations.

use super::{
    folder_matches_expected, push_ranked_meta_top_k, set_reconcile_failpoint, PasteDb,
    META_INDEX_FAULTED_KEY, META_INDEX_IN_PROGRESS_COUNT_KEY, META_INDEX_SCHEMA_VERSION,
    META_INDEX_VERSION_KEY,
};
use crate::models::paste::{Paste, UpdatePasteRequest};
use crate::AppError;
use chrono::Duration;
use std::collections::HashSet;
use std::sync::Arc;
use tempfile::TempDir;

fn setup_paste_db() -> (PasteDb, TempDir) {
    let dir = TempDir::new().expect("temp dir");
    let db_path = dir.path().join("db");
    let db = Arc::new(sled::open(db_path).expect("open sled"));
    let paste_db = PasteDb::new(db).expect("open paste db");
    (paste_db, dir)
}

#[test]
fn update_recomputes_markdown_without_hash_false_positives() {
    let (paste_db, _dir) = setup_paste_db();
    let paste = Paste::new("plain".to_string(), "name".to_string());
    let id = paste.id.clone();
    paste_db.create(&paste).expect("create paste");

    let updated = paste_db
        .update(
            id.as_str(),
            UpdatePasteRequest {
                content: Some("#[derive(Debug)]\nstruct Example;".to_string()),
                name: None,
                language: None,
                language_is_manual: None,
                folder_id: None,
                tags: None,
            },
        )
        .expect("update should succeed")
        .expect("paste should exist");

    assert!(
        !updated.is_markdown,
        "hash-prefixed non-markdown syntax should not set markdown flag"
    );
}

#[test]
fn folder_mismatch_state_tracks_latest_retry_attempt() {
    let mut mismatch = false;

    // First attempt mismatches expected folder.
    assert!(!folder_matches_expected(
        Some("folder-a"),
        Some("folder-b"),
        &mut mismatch
    ));
    assert!(mismatch);

    // Retry attempt matches; state must be cleared for final evaluation.
    assert!(folder_matches_expected(
        Some("folder-a"),
        Some("folder-a"),
        &mut mismatch
    ));
    assert!(!mismatch);
}

#[test]
fn needs_reconcile_detects_missing_marker() {
    let (paste_db, _dir) = setup_paste_db();
    assert!(paste_db.meta_tree.is_empty());
    assert!(paste_db.updated_tree.is_empty());
    assert!(paste_db
        .meta_state_tree
        .get(META_INDEX_VERSION_KEY)
        .expect("state lookup")
        .is_none());
    assert!(paste_db
        .needs_reconcile_meta_indexes(false)
        .expect("needs reconcile"));
}

#[test]
fn needs_reconcile_detects_missing_index_rows_with_marker() {
    let (paste_db, _dir) = setup_paste_db();
    paste_db
        .reconcile_meta_indexes()
        .expect("initial reconcile writes marker");
    let paste = Paste::new("hello".to_string(), "hello".to_string());
    paste_db.create(&paste).expect("create paste");

    paste_db.meta_tree.clear().expect("clear meta");
    assert!(paste_db
        .meta_state_tree
        .get(META_INDEX_VERSION_KEY)
        .expect("state lookup")
        .is_some());
    assert!(paste_db
        .needs_reconcile_meta_indexes(false)
        .expect("needs reconcile"));
}

#[test]
fn needs_reconcile_detects_partial_non_empty_metadata_drift() {
    let (paste_db, _dir) = setup_paste_db();
    paste_db
        .reconcile_meta_indexes()
        .expect("initial reconcile writes marker");
    let first = Paste::new("first".to_string(), "first".to_string());
    let second = Paste::new("second".to_string(), "second".to_string());
    paste_db.create(&first).expect("create first");
    paste_db.create(&second).expect("create second");

    paste_db
        .meta_tree
        .remove(first.id.as_bytes())
        .expect("remove first meta");
    paste_db
        .updated_tree
        .remove(super::helpers::index_key(
            first.updated_at,
            first.id.as_str(),
        ))
        .expect("remove first updated index");

    assert!(
        !paste_db.meta_tree.is_empty(),
        "meta tree should remain non-empty"
    );
    assert!(
        !paste_db.updated_tree.is_empty(),
        "updated tree should remain non-empty"
    );
    assert!(
        paste_db
            .meta_state_tree
            .get(META_INDEX_VERSION_KEY)
            .expect("state lookup")
            .is_some(),
        "version marker should remain present"
    );
    assert!(paste_db
        .needs_reconcile_meta_indexes(false)
        .expect("needs reconcile"));
}

#[test]
fn needs_reconcile_skips_deep_scan_when_markers_are_clean() {
    let (paste_db, _dir) = setup_paste_db();
    paste_db
        .reconcile_meta_indexes()
        .expect("initial reconcile writes marker");
    let paste = Paste::new("hello".to_string(), "hello".to_string());
    paste_db.create(&paste).expect("create paste");

    paste_db
        .meta_tree
        .insert(paste.id.as_bytes(), b"corrupt-meta-row")
        .expect("insert corrupt meta");
    assert_eq!(paste_db.tree.len(), paste_db.meta_tree.len());
    assert_eq!(paste_db.tree.len(), paste_db.updated_tree.len());
    assert!(!paste_db
        .needs_reconcile_meta_indexes(false)
        .expect("needs reconcile"));
}

#[test]
fn list_meta_falls_back_to_canonical_when_index_is_inconsistent() {
    let (paste_db, _dir) = setup_paste_db();
    paste_db
        .reconcile_meta_indexes()
        .expect("initial reconcile writes marker");
    let paste = Paste::new("fallback body".to_string(), "fallback-name".to_string());
    let paste_id = paste.id.clone();
    paste_db.create(&paste).expect("create paste");

    paste_db
        .meta_tree
        .remove(paste_id.as_bytes())
        .expect("remove meta row");

    let metas = paste_db
        .list_meta(10, None)
        .expect("list metadata fallback");
    assert!(
        metas.into_iter().any(|meta| meta.id == paste_id),
        "canonical fallback should retain visibility of canonical rows"
    );
}

#[test]
fn list_meta_detects_semantic_meta_drift_and_falls_back_to_canonical() {
    let (paste_db, _dir) = setup_paste_db();
    paste_db
        .reconcile_meta_indexes()
        .expect("initial reconcile writes marker");
    let paste = Paste::new("body".to_string(), "old-name".to_string());
    let paste_id = paste.id.clone();
    paste_db.create(&paste).expect("create paste");

    let mut rewritten = paste.clone();
    rewritten.name = "new-name".to_string();
    rewritten.content = "rewritten body".to_string();
    rewritten.updated_at += Duration::seconds(1);
    paste_db
        .tree
        .insert(
            paste_id.as_bytes(),
            bincode::serialize(&rewritten).expect("serialize rewritten paste"),
        )
        .expect("rewrite canonical row only");

    let metas = paste_db
        .list_meta(10, None)
        .expect("list metadata fallback");
    let rewritten_meta = metas
        .iter()
        .find(|meta| meta.id == paste_id)
        .expect("rewritten metadata row should be visible");
    assert_eq!(
        rewritten_meta.name, "new-name",
        "stale derived metadata should not leak when canonical row changed"
    );
    assert_eq!(
        rewritten_meta.content_len,
        "rewritten body".len(),
        "fallback should expose canonical content length"
    );
}

#[test]
fn list_meta_omits_ghost_rows_when_canonical_row_is_missing() {
    let (paste_db, _dir) = setup_paste_db();
    paste_db
        .reconcile_meta_indexes()
        .expect("initial reconcile writes marker");
    let paste = Paste::new("ghost".to_string(), "ghost".to_string());
    let paste_id = paste.id.clone();
    paste_db.create(&paste).expect("create paste");

    paste_db
        .tree
        .remove(paste_id.as_bytes())
        .expect("remove canonical row");
    assert!(
        paste_db
            .meta_tree
            .get(paste_id.as_bytes())
            .expect("meta lookup")
            .is_some(),
        "meta row should remain to simulate ghost entry"
    );

    let metas = paste_db
        .list_meta(10, None)
        .expect("list metadata fallback");
    assert!(
        metas.into_iter().all(|meta| meta.id != paste_id),
        "canonical fallback should hide ghost metadata rows"
    );
}

#[test]
fn list_meta_dedupes_duplicate_updated_index_entries() {
    let (paste_db, _dir) = setup_paste_db();
    paste_db
        .reconcile_meta_indexes()
        .expect("initial reconcile writes marker");
    let paste = Paste::new("body".to_string(), "duplicate-index".to_string());
    let paste_id = paste.id.clone();
    let updated_at = paste.updated_at;
    paste_db.create(&paste).expect("create paste");

    let stale_key =
        super::helpers::index_key(updated_at - Duration::seconds(60), paste_id.as_str());
    paste_db
        .updated_tree
        .insert(stale_key, paste_id.as_bytes())
        .expect("inject duplicate updated index entry");

    let metas = paste_db.list_meta(10, None).expect("list metadata");
    let duplicate_count = metas.iter().filter(|meta| meta.id == paste_id).count();
    assert_eq!(
        duplicate_count, 1,
        "duplicate updated entries must dedupe by id"
    );
}

#[test]
fn list_meta_treats_in_progress_marker_as_degraded_and_falls_back_to_canonical() {
    let (paste_db, _dir) = setup_paste_db();
    paste_db
        .reconcile_meta_indexes()
        .expect("initial reconcile writes marker");
    let paste = Paste::new("body".to_string(), "indexed-only".to_string());
    let paste_id = paste.id.clone();
    paste_db.create(&paste).expect("create paste");

    paste_db
        .meta_state_tree
        .insert(
            META_INDEX_IN_PROGRESS_COUNT_KEY,
            1u64.to_be_bytes().to_vec(),
        )
        .expect("set in-progress marker");

    // If list_meta falls back to canonical rows, this injected corruption would fail decode.
    paste_db
        .tree
        .insert(paste_id.as_bytes(), b"corrupt-canonical-row")
        .expect("corrupt canonical row");

    let err = paste_db
        .list_meta(10, None)
        .expect_err("in-progress marker should force canonical fallback");
    assert!(
        matches!(err, AppError::Serialization(_)),
        "fallback path should surface canonical decode errors"
    );
}

#[test]
fn search_meta_falls_back_to_canonical_when_meta_decode_fails() {
    let (paste_db, _dir) = setup_paste_db();
    paste_db
        .reconcile_meta_indexes()
        .expect("initial reconcile writes marker");
    let paste = Paste::new("body".to_string(), "needle-meta".to_string());
    let paste_id = paste.id.clone();
    paste_db.create(&paste).expect("create paste");

    paste_db
        .meta_tree
        .insert(paste_id.as_bytes(), b"corrupt-meta")
        .expect("corrupt metadata row");

    let metas = paste_db
        .search_meta("needle", 10, None, None)
        .expect("search metadata fallback");
    assert!(
        metas.into_iter().any(|meta| meta.id == paste_id),
        "canonical fallback should retain metadata search results"
    );
}

#[test]
fn metadata_top_k_accumulator_stays_bounded_by_limit() {
    let mut ranked: Vec<(
        i32,
        chrono::DateTime<chrono::Utc>,
        crate::models::paste::PasteMeta,
    )> = Vec::new();
    let limit = 5usize;

    for idx in 0..100 {
        let mut paste = Paste::new(format!("body-{idx}"), format!("name-{idx}"));
        paste.updated_at = chrono::Utc::now() + Duration::seconds(idx);
        let meta = crate::models::paste::PasteMeta::from(&paste);
        let score = if idx % 10 == 0 { 10 } else { 1 };
        push_ranked_meta_top_k(&mut ranked, (score, meta.updated_at, meta), limit);
        assert!(
            ranked.len() <= limit,
            "metadata top-k accumulator must stay bounded by limit"
        );
    }

    assert_eq!(ranked.len(), limit);
}

#[test]
fn list_canonical_ids_batch_honors_limit_and_folder_filter() {
    let (paste_db, _dir) = setup_paste_db();

    let mut in_folder_a = Paste::new("a".to_string(), "a".to_string());
    in_folder_a.folder_id = Some("folder-a".to_string());
    let in_folder_a_id = in_folder_a.id.clone();
    paste_db
        .create(&in_folder_a)
        .expect("create folder-a paste");

    let mut in_folder_b = Paste::new("b".to_string(), "b".to_string());
    in_folder_b.folder_id = Some("folder-b".to_string());
    let in_folder_b_id = in_folder_b.id.clone();
    paste_db
        .create(&in_folder_b)
        .expect("create folder-b paste");

    let mut in_folder_a_2 = Paste::new("c".to_string(), "c".to_string());
    in_folder_a_2.folder_id = Some("folder-a".to_string());
    let in_folder_a_2_id = in_folder_a_2.id.clone();
    paste_db
        .create(&in_folder_a_2)
        .expect("create second folder-a paste");

    let folder_a_ids = paste_db
        .list_canonical_ids_batch(1, Some("folder-a"))
        .expect("list folder-a ids");
    assert_eq!(folder_a_ids.len(), 1);
    assert!(
        folder_a_ids[0] == in_folder_a_id || folder_a_ids[0] == in_folder_a_2_id,
        "folder filter should only return matching canonical ids"
    );

    let folder_b_ids = paste_db
        .list_canonical_ids_batch(10, Some("folder-b"))
        .expect("list folder-b ids");
    assert_eq!(folder_b_ids, vec![in_folder_b_id]);

    let missing = paste_db
        .list_canonical_ids_batch(10, Some("missing-folder"))
        .expect("list missing folder ids");
    assert!(missing.is_empty());

    let zero_limit = paste_db
        .list_canonical_ids_batch(0, None)
        .expect("zero-limit ids");
    assert!(zero_limit.is_empty());
}

#[test]
fn scan_canonical_meta_streams_all_rows() {
    let (paste_db, _dir) = setup_paste_db();

    let mut with_folder = Paste::new("a".to_string(), "a".to_string());
    with_folder.folder_id = Some("folder-a".to_string());
    let with_folder_id = with_folder.id.clone();
    paste_db.create(&with_folder).expect("create folder paste");

    let without_folder = Paste::new("b".to_string(), "b".to_string());
    let without_folder_id = without_folder.id.clone();
    paste_db
        .create(&without_folder)
        .expect("create unfiled paste");

    let mut seen_ids = HashSet::new();
    let mut folder_rows = 0usize;
    paste_db
        .scan_canonical_meta(|meta| {
            if meta.folder_id.is_some() {
                folder_rows = folder_rows.saturating_add(1);
            }
            seen_ids.insert(meta.id);
            Ok(())
        })
        .expect("scan canonical meta");

    assert!(seen_ids.contains(&with_folder_id));
    assert!(seen_ids.contains(&without_folder_id));
    assert_eq!(folder_rows, 1);
}

struct ReconcileFailpointGuard;

impl Drop for ReconcileFailpointGuard {
    fn drop(&mut self) {
        set_reconcile_failpoint(false);
    }
}

#[test]
fn reconcile_failure_marks_faulted_and_clears_in_progress() {
    let _guard = ReconcileFailpointGuard;
    let (paste_db, _dir) = setup_paste_db();
    let paste = Paste::new("content".to_string(), "name".to_string());
    let paste_id = paste.id.clone();
    paste_db.create(&paste).expect("create paste");

    set_reconcile_failpoint(true);
    let err = paste_db
        .reconcile_meta_indexes()
        .expect_err("failpoint should force reconcile error");
    assert!(
        matches!(err, AppError::StorageMessage(ref message) if message.contains("Injected reconcile failpoint")),
        "unexpected reconcile error: {}",
        err
    );

    assert!(
        paste_db.meta_index_faulted().expect("faulted marker"),
        "failed reconcile must mark metadata indexes faulted"
    );
    assert_eq!(
        paste_db
            .meta_index_in_progress_count()
            .expect("in-progress marker"),
        0,
        "failed reconcile must clear in-progress state"
    );
    assert!(
        paste_db
            .needs_reconcile_meta_indexes(false)
            .expect("needs reconcile"),
        "failed reconcile should require follow-up reconcile"
    );

    let metas = paste_db
        .list_meta(10, None)
        .expect("metadata list via fallback");
    assert!(
        metas.iter().any(|meta| meta.id == paste_id),
        "faulted indexes should preserve visibility via canonical fallback"
    );
}

#[test]
fn create_commits_canonical_row_when_index_write_fails() {
    let (paste_db, _dir) = setup_paste_db();
    let paste = Paste::new("content".to_string(), "name".to_string());
    let id = paste.id.clone();

    let result = paste_db.create_inner(&paste, |_db, _paste| {
        Err(AppError::StorageMessage(
            "injected create index failure".to_string(),
        ))
    });
    assert!(result.is_ok(), "canonical create should still succeed");
    assert!(
        paste_db
            .tree
            .get(id.as_bytes())
            .expect("canonical lookup")
            .is_some(),
        "canonical row should remain committed"
    );
    assert!(
        paste_db
            .meta_tree
            .get(id.as_bytes())
            .expect("meta lookup")
            .is_none(),
        "derived metadata should remain stale until reconcile"
    );
    assert!(
        paste_db.meta_index_faulted().expect("faulted marker"),
        "failed index write should set faulted marker"
    );
    assert_eq!(
        paste_db
            .meta_index_in_progress_count()
            .expect("in-progress count"),
        0,
        "failed index write should still clear in-progress marker"
    );
    assert!(
        paste_db
            .needs_reconcile_meta_indexes(false)
            .expect("needs reconcile"),
        "faulted marker should force reconcile"
    );
    let metas = paste_db
        .list_meta(10, None)
        .expect("list metadata via canonical fallback");
    assert!(
        metas.iter().any(|meta| meta.id == id),
        "canonical fallback should keep committed paste visible"
    );
}

#[test]
fn reconcile_clears_faulted_marker_after_index_write_failure() {
    let (paste_db, _dir) = setup_paste_db();
    let paste = Paste::new("content".to_string(), "name".to_string());

    let result = paste_db.create_inner(&paste, |_db, _paste| {
        Err(AppError::StorageMessage(
            "injected create index failure".to_string(),
        ))
    });
    assert!(result.is_ok(), "canonical create should still succeed");
    assert!(
        paste_db.meta_index_faulted().expect("faulted marker"),
        "failure should mark indexes as faulted"
    );

    paste_db
        .reconcile_meta_indexes()
        .expect("reconcile should clear fault marker");
    assert!(
        !paste_db.meta_index_faulted().expect("faulted marker"),
        "reconcile should clear fault marker"
    );
}

#[test]
fn update_commits_canonical_row_when_index_write_fails() {
    let (paste_db, _dir) = setup_paste_db();
    let paste = Paste::new("before".to_string(), "name".to_string());
    let id = paste.id.clone();
    paste_db.create(&paste).expect("create paste");

    let updated = paste_db
        .update_inner(
            id.as_str(),
            UpdatePasteRequest {
                content: Some("after".to_string()),
                name: None,
                language: None,
                language_is_manual: None,
                folder_id: None,
                tags: None,
            },
            |_db, _paste, _previous| {
                Err(AppError::StorageMessage(
                    "injected update index failure".to_string(),
                ))
            },
        )
        .expect("update should still report success despite derived index failure")
        .expect("paste should exist");
    assert_eq!(updated.content, "after");

    let canonical = paste_db
        .get(id.as_str())
        .expect("get paste")
        .expect("paste should remain");
    assert_eq!(canonical.content, "after");
    assert!(
        paste_db.meta_index_faulted().expect("faulted marker"),
        "failed index write should set faulted marker"
    );
    assert_eq!(
        paste_db
            .meta_index_in_progress_count()
            .expect("in-progress count"),
        0
    );
}

#[test]
fn update_if_folder_matches_commits_when_index_write_fails() {
    let (paste_db, _dir) = setup_paste_db();
    let paste = Paste::new("before".to_string(), "name".to_string());
    let id = paste.id.clone();
    paste_db.create(&paste).expect("create paste");

    let moved = paste_db
        .update_if_folder_matches_inner(
            id.as_str(),
            None,
            UpdatePasteRequest {
                content: None,
                name: Some("moved".to_string()),
                language: None,
                language_is_manual: None,
                folder_id: Some("folder-x".to_string()),
                tags: None,
            },
            |_db, _paste, _previous| {
                Err(AppError::StorageMessage(
                    "injected cas index failure".to_string(),
                ))
            },
        )
        .expect("cas update should still report success")
        .expect("paste should exist");
    assert_eq!(moved.name, "moved");
    assert_eq!(moved.folder_id.as_deref(), Some("folder-x"));

    let canonical = paste_db
        .get(id.as_str())
        .expect("get paste")
        .expect("paste should remain");
    assert_eq!(canonical.folder_id.as_deref(), Some("folder-x"));
    assert!(
        paste_db.meta_index_faulted().expect("faulted marker"),
        "failed derived index write should set faulted marker"
    );
    assert_eq!(
        paste_db
            .meta_index_in_progress_count()
            .expect("in-progress count"),
        0
    );
}

#[test]
fn delete_commits_canonical_removal_when_index_delete_fails() {
    let (paste_db, _dir) = setup_paste_db();
    let paste = Paste::new("body".to_string(), "name".to_string());
    let id = paste.id.clone();
    paste_db.create(&paste).expect("create paste");

    let deleted = paste_db
        .delete_and_return_inner(id.as_str(), |_db, _meta| {
            Err(AppError::StorageMessage(
                "injected delete index failure".to_string(),
            ))
        })
        .expect("delete should still report success")
        .expect("paste should be returned");
    assert_eq!(deleted.id, id);
    assert!(
        paste_db.get(id.as_str()).expect("lookup").is_none(),
        "canonical row should remain deleted"
    );
    assert!(
        paste_db.meta_index_faulted().expect("faulted marker"),
        "failed index delete should set faulted marker"
    );
    assert_eq!(
        paste_db
            .meta_index_in_progress_count()
            .expect("in-progress count"),
        0
    );
}

#[test]
fn update_error_does_not_leave_meta_index_state_marked() {
    let (paste_db, _dir) = setup_paste_db();
    let paste = Paste::new("body".to_string(), "name".to_string());
    let paste_id = paste.id.clone();
    paste_db.create(&paste).expect("create paste");

    paste_db
        .tree
        .insert(paste_id.as_bytes(), b"corrupt-paste-row")
        .expect("corrupt canonical row");

    let update = UpdatePasteRequest {
        content: Some("updated".to_string()),
        name: None,
        language: None,
        language_is_manual: None,
        folder_id: None,
        tags: None,
    };
    let err = paste_db
        .update(paste_id.as_str(), update)
        .expect_err("corrupt row should fail update");
    assert!(
        matches!(err, AppError::Serialization(_)),
        "expected serialization error for corrupt canonical row"
    );
    assert_eq!(
        paste_db
            .meta_index_in_progress_count()
            .expect("in-progress count"),
        0,
        "update errors without index mutation should not leave in-progress marker set"
    );
    assert!(
        !paste_db.meta_index_faulted().expect("faulted marker"),
        "update errors without index mutation should not mark indexes as faulted"
    );
}

#[test]
fn update_if_folder_mismatch_error_does_not_leave_meta_index_state_marked() {
    let (paste_db, _dir) = setup_paste_db();
    let paste = Paste::new("body".to_string(), "name".to_string());
    let paste_id = paste.id.clone();
    paste_db.create(&paste).expect("create paste");

    paste_db
        .tree
        .insert(paste_id.as_bytes(), b"corrupt-paste-row")
        .expect("corrupt canonical row");

    let update = UpdatePasteRequest {
        content: Some("updated".to_string()),
        name: None,
        language: None,
        language_is_manual: None,
        folder_id: None,
        tags: None,
    };
    let err = paste_db
        .update_if_folder_matches(paste_id.as_str(), None, update)
        .expect_err("corrupt row should fail update");
    assert!(
        matches!(err, AppError::Serialization(_)),
        "expected serialization error for corrupt canonical row"
    );
    assert_eq!(
        paste_db
            .meta_index_in_progress_count()
            .expect("in-progress count"),
        0,
        "failed CAS update without index mutation should not leave in-progress marker set"
    );
    assert!(
        !paste_db.meta_index_faulted().expect("faulted marker"),
        "failed CAS update without index mutation should not mark indexes as faulted"
    );
}

#[test]
fn needs_reconcile_returns_false_when_marker_and_indexes_are_healthy() {
    let (paste_db, _dir) = setup_paste_db();
    paste_db
        .reconcile_meta_indexes()
        .expect("initial reconcile writes marker");
    let marker = paste_db
        .meta_state_tree
        .get(META_INDEX_VERSION_KEY)
        .expect("state lookup")
        .expect("state marker");
    assert_eq!(
        u32::from_be_bytes(marker.as_ref().try_into().expect("version bytes")),
        META_INDEX_SCHEMA_VERSION
    );
    let in_progress = paste_db
        .meta_state_tree
        .get(META_INDEX_IN_PROGRESS_COUNT_KEY)
        .expect("in-progress lookup")
        .expect("in-progress marker");
    assert_eq!(
        u64::from_be_bytes(in_progress.as_ref().try_into().expect("in-progress bytes")),
        0
    );
    let faulted = paste_db
        .meta_state_tree
        .get(META_INDEX_FAULTED_KEY)
        .expect("faulted lookup")
        .expect("faulted marker");
    assert_eq!(faulted.as_ref(), &[0u8]);
    assert!(!paste_db
        .needs_reconcile_meta_indexes(false)
        .expect("needs reconcile"));
}

#[test]
fn needs_reconcile_honors_force_reindex_flag() {
    let (paste_db, _dir) = setup_paste_db();
    paste_db
        .reconcile_meta_indexes()
        .expect("initial reconcile writes marker");
    assert!(paste_db
        .needs_reconcile_meta_indexes(true)
        .expect("needs reconcile"));
}
