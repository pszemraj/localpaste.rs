//! Scoped search through worker requests, cache keys, and echoed response context.

use super::*;
use localpaste_core::models::paste::SearchScope;

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
    for _ in 0..2 {
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
            }
        }
    }
}
