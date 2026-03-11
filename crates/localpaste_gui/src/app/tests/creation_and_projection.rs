//! Paste-creation tests for selection and sidebar projection behavior.

use super::*;

#[test]
fn paste_created_hidden_by_active_filter_keeps_visible_selection_and_projection() {
    let mut harness = make_app();
    harness.app.all_pastes = vec![test_summary("alpha", "Alpha", Some("rust"), 7)];
    harness.app.pastes = harness.app.all_pastes.clone();
    harness
        .app
        .set_active_language_filter(Some("rust".to_string()));

    let mut created = Paste::new("print('hi')".to_string(), "new-note".to_string());
    created.id = "new-id".to_string();
    created.language = Some("python".to_string());
    created.language_is_manual = true;

    harness
        .app
        .apply_event(CoreEvent::PasteCreated { paste: created });

    assert!(
        harness
            .app
            .all_pastes
            .iter()
            .any(|item| item.id == "new-id"),
        "canonical cache should include the newly created paste"
    );
    assert_eq!(
        harness
            .app
            .pastes
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        vec!["alpha"],
        "visible projection should continue honoring the active language filter"
    );
    assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));
    assert_eq!(
        harness
            .app
            .status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some("Created new paste; current filters keep it hidden.")
    );
    assert!(matches!(
        harness.cmd_rx.try_recv(),
        Err(TryRecvError::Empty)
    ));
}

#[test]
fn paste_created_during_active_search_keeps_visible_projection_and_invalidates_search() {
    let mut harness = make_app();
    harness.app.search_query = "alpha".to_string();
    harness.app.search_last_sent = "alpha".to_string();
    harness.app.search_last_input_at = None;
    harness.app.pastes = vec![test_summary("alpha", "Alpha", None, 7)];
    harness.app.all_pastes = harness.app.pastes.clone();

    let mut created = Paste::new("new-content".to_string(), "Gamma".to_string());
    created.id = "new-id".to_string();

    harness
        .app
        .apply_event(CoreEvent::PasteCreated { paste: created });

    assert!(
        harness
            .app
            .all_pastes
            .iter()
            .any(|item| item.id == "new-id"),
        "canonical cache should include the newly created paste"
    );
    assert_eq!(
        harness
            .app
            .pastes
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        vec!["alpha"],
        "active search results should remain the visible projection until refreshed"
    );
    assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));
    assert!(
        harness.app.search_last_sent.is_empty(),
        "create should invalidate active-search cache"
    );
    assert!(
        harness.app.search_last_input_at.is_some(),
        "search dispatch timestamp should be rewound so refresh happens immediately"
    );
    assert_eq!(
        harness
            .app
            .status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some("Created new paste; refreshing search results.")
    );

    harness.app.maybe_dispatch_search();
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::SearchPastes { query, .. } => assert_eq!(query, "alpha"),
        other => panic!("unexpected command: {:?}", other),
    }
}
