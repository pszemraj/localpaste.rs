//! Scoped search through worker requests, cache keys, and echoed response context.

use super::*;
use localpaste_core::db::tables::PASTES;
use localpaste_core::models::paste::SearchScope;

#[test]
fn worker_filters_collections_before_top_k_and_separates_cached_collections() {
    let TestDb { _dir: _guard, db } = setup_db();
    let mut document = Paste::new("Needle notes for tomorrow".into(), "Needle notes".into());
    document.language = Some("markdown".into());
    document.updated_at = Utc::now() - chrono::Duration::days(1);
    let mut code = Paste::new("// Needle\nfn main() {}".into(), "Needle code".into());
    code.language = Some("rust".into());
    for paste in [&document, &code] {
        db.pastes.create(paste).unwrap();
    }
    let backend = spawn_backend(db, 10 * 1024 * 1024);
    for (collection, expected) in [
        (SidebarCollection::All, &code.id),
        (SidebarCollection::Documents, &document.id),
        (SidebarCollection::Code, &code.id),
    ] {
        backend
            .cmd_tx
            .send(CoreCmd::SearchPastes {
                collection: collection.clone(),
                query: "Needle".into(),
                scope: SearchScope::Title,
                limit: 1,
                folder_id: None,
                language: None,
            })
            .unwrap();
        match recv_event(&backend.evt_rx) {
            CoreEvent::SearchResults {
                collection: returned,
                items,
                ..
            } => {
                assert_eq!(returned, collection);
                assert_eq!(
                    items.iter().map(|item| &item.id).collect::<Vec<_>>(),
                    vec![expected]
                );
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}

#[test]
fn worker_cache_separates_scopes_and_searches_beyond_the_loaded_list() {
    let TestDb { _dir: _guard, db } = setup_db();
    let title = Paste::new("ordinary body".into(), "Needle".into());
    let mut metadata = Paste::new("unrelated body".into(), "tagged".into());
    metadata.tags = vec!["Needle".into()];
    let body = Paste::new("Needle body".into(), "plain".into());
    for paste in [&title, &metadata, &body] {
        db.pastes.create(paste).unwrap();
    }
    let backend = spawn_backend(db, 10 * 1024 * 1024);
    backend
        .cmd_tx
        .send(CoreCmd::ListPastes {
            limit: 1,
            folder_id: None,
        })
        .unwrap();
    assert!(
        matches!(recv_event(&backend.evt_rx), CoreEvent::PasteList { items } if items.len() == 1)
    );
    for (scope, expected) in [
        (SearchScope::Title, vec![&title.id]),
        (SearchScope::Body, vec![&body.id]),
        (
            SearchScope::Metadata,
            vec![&title.id, &metadata.id, &body.id],
        ),
        (SearchScope::All, vec![&title.id, &metadata.id, &body.id]),
    ] {
        for picker in [false, true] {
            let command = if picker {
                CoreCmd::SearchPalette {
                    query: "Needle".into(),
                    scope,
                    limit: 10,
                }
            } else {
                CoreCmd::SearchPastes {
                    collection: crate::backend::SidebarCollection::All,
                    query: "Needle".into(),
                    scope,
                    limit: 10,
                    folder_id: None,
                    language: None,
                }
            };
            backend.cmd_tx.send(command).unwrap();
            let (returned_scope, items) = match recv_event(&backend.evt_rx) {
                CoreEvent::SearchResults { scope, items, .. }
                | CoreEvent::PaletteSearchResults { scope, items, .. } => (scope, items),
                other => panic!("unexpected {other:?}"),
            };
            assert_eq!(returned_scope, scope);
            let mut ids: Vec<_> = items.iter().map(|item| &item.id).collect();
            let mut expected = expected.clone();
            ids.sort();
            expected.sort();
            assert_eq!(ids, expected);
            if picker && matches!(scope, SearchScope::All | SearchScope::Body) {
                assert!(items
                    .iter()
                    .find(|item| item.id == body.id)
                    .and_then(|item| item.match_excerpt.as_deref())
                    .is_some_and(|excerpt| excerpt.contains("Needle")));
                if scope == SearchScope::All {
                    assert!(items
                        .iter()
                        .filter(|item| item.id != body.id)
                        .all(|item| item.match_excerpt.is_none()));
                }
            } else {
                assert!(items.iter().all(|item| item.match_excerpt.is_none()));
            }
        }
    }
}

#[test]
fn picker_metadata_scopes_do_not_deserialize_canonical_bodies() {
    let TestDb { _dir: _guard, db } = setup_db();
    let mut paste = Paste::new("ordinary body".into(), "Needle title".into());
    paste.tags = vec!["needle tag".into()];
    db.pastes.create(&paste).unwrap();

    let write_txn = db.db.begin_write().unwrap();
    write_txn
        .open_table(PASTES)
        .unwrap()
        .insert(paste.id.as_str(), &[255_u8][..])
        .unwrap();
    write_txn.commit().unwrap();

    let backend = spawn_backend(db, 10 * 1024 * 1024);
    for scope in [SearchScope::Title, SearchScope::Metadata] {
        backend
            .cmd_tx
            .send(CoreCmd::SearchPalette {
                query: "needle".into(),
                scope,
                limit: 10,
            })
            .unwrap();
        match recv_event(&backend.evt_rx) {
            CoreEvent::PaletteSearchResults { items, .. } => {
                assert_eq!(items.len(), 1);
                assert_eq!(items[0].id, paste.id);
                assert!(items[0].match_excerpt.is_none());
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}
