use super::*;

#[test]
fn search_results_respect_collection_filter() {
    let mut harness = make_app();
    harness
        .app
        .set_active_collection(SidebarCollection::Unfiled);
    harness.app.set_search_query("rust".to_string());

    let now = Utc::now();
    let with_folder = PasteSummary {
        id: "a".to_string(),
        name: "with-folder".to_string(),
        language: Some("rust".to_string()),
        content_len: 10,
        updated_at: now,
        folder_id: Some("folder-1".to_string()),
        tags: Vec::new(),
    };
    let unfiled = PasteSummary {
        id: "b".to_string(),
        name: "unfiled".to_string(),
        language: Some("rust".to_string()),
        content_len: 10,
        updated_at: now,
        folder_id: None,
        tags: Vec::new(),
    };

    harness.app.apply_event(CoreEvent::SearchResults {
        query: "rust".to_string(),
        items: vec![with_folder, unfiled.clone()],
    });

    assert_eq!(harness.app.pastes.len(), 1);
    assert_eq!(harness.app.pastes[0].id, unfiled.id);

    let stale = PasteSummary {
        id: "stale".to_string(),
        name: "stale-result".to_string(),
        language: Some("rust".to_string()),
        content_len: 2,
        updated_at: now,
        folder_id: None,
        tags: Vec::new(),
    };
    harness.app.set_search_query(String::new());
    harness.app.apply_event(CoreEvent::SearchResults {
        query: "rust".to_string(),
        items: vec![stale],
    });
    assert_eq!(harness.app.pastes.len(), 1);
    assert_eq!(harness.app.pastes[0].id, unfiled.id);
}

#[test]
fn paste_list_filters_recent_collection() {
    let mut harness = make_app();
    harness.app.set_active_collection(SidebarCollection::Recent);
    let old = PasteSummary {
        id: "old".to_string(),
        name: "old".to_string(),
        language: None,
        content_len: 3,
        updated_at: Utc::now() - chrono::Duration::days(30),
        folder_id: None,
        tags: Vec::new(),
    };
    let fresh = PasteSummary {
        id: "fresh".to_string(),
        name: "fresh".to_string(),
        language: None,
        content_len: 5,
        updated_at: Utc::now(),
        folder_id: None,
        tags: Vec::new(),
    };

    harness.app.apply_event(CoreEvent::PasteList {
        items: vec![old, fresh.clone()],
    });
    assert_eq!(harness.app.pastes.len(), 1);
    assert_eq!(harness.app.pastes[0].id, fresh.id);
}

#[test]
fn command_palette_ranking_prefers_prefix() {
    let mut harness = make_app();
    harness.app.all_pastes = vec![
        PasteSummary {
            id: "1".to_string(),
            name: "alpha parser".to_string(),
            language: Some("rust".to_string()),
            content_len: 100,
            updated_at: Utc::now(),
            folder_id: None,
            tags: vec!["core".to_string()],
        },
        PasteSummary {
            id: "2".to_string(),
            name: "parser alpha".to_string(),
            language: Some("rust".to_string()),
            content_len: 100,
            updated_at: Utc::now(),
            folder_id: None,
            tags: vec!["core".to_string()],
        },
    ];
    harness.app.command_palette_query = "alpha".to_string();

    let ranked = harness.app.rank_palette_results();
    assert_eq!(ranked.len(), 2);
    assert_eq!(ranked[0].id, "1");
}

#[test]
fn maybe_dispatch_search_requires_debounce_and_dedupes() {
    let mut harness = make_app();
    harness.app.set_search_query("rust".to_string());

    harness.app.search_last_input_at = Some(Instant::now());
    harness.app.maybe_dispatch_search();
    assert!(matches!(
        harness.cmd_rx.try_recv(),
        Err(TryRecvError::Empty)
    ));

    harness.app.search_last_input_at =
        Some(Instant::now() - SEARCH_DEBOUNCE - Duration::from_millis(10));
    harness.app.maybe_dispatch_search();
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::SearchPastes {
            query,
            limit,
            folder_id,
            language,
        } => {
            assert_eq!(query, "rust");
            assert_eq!(limit, 512);
            assert!(folder_id.is_none());
            assert!(language.is_none());
        }
        other => panic!("unexpected command: {:?}", other),
    }

    harness.app.maybe_dispatch_search();
    assert!(matches!(
        harness.cmd_rx.try_recv(),
        Err(TryRecvError::Empty)
    ));
}

#[test]
fn maybe_dispatch_search_applies_collection_filters() {
    let mut harness = make_app();
    harness.app.set_active_collection(SidebarCollection::Code);
    harness.app.set_search_query("alpha".to_string());
    harness.app.search_last_input_at =
        Some(Instant::now() - SEARCH_DEBOUNCE - Duration::from_millis(10));
    harness.app.maybe_dispatch_search();
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::SearchPastes {
            folder_id,
            language,
            ..
        } => {
            assert!(folder_id.is_none());
            assert!(language.is_none());
        }
        other => panic!("unexpected command: {:?}", other),
    }

    harness.app.set_active_collection(SidebarCollection::All);
    harness
        .app
        .set_active_language_filter(Some("rust".to_string()));
    harness.app.set_search_query("beta".to_string());
    harness.app.search_last_input_at =
        Some(Instant::now() - SEARCH_DEBOUNCE - Duration::from_millis(10));
    harness.app.maybe_dispatch_search();
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::SearchPastes {
            folder_id,
            language,
            ..
        } => {
            assert!(folder_id.is_none());
            assert_eq!(language.as_deref(), Some("rust"));
        }
        other => panic!("unexpected command: {:?}", other),
    }

    harness.app.set_active_collection(SidebarCollection::Logs);
    harness
        .app
        .set_active_language_filter(Some("python".to_string()));
    harness.app.set_search_query("gamma".to_string());
    harness.app.search_last_input_at =
        Some(Instant::now() - SEARCH_DEBOUNCE - Duration::from_millis(10));
    harness.app.maybe_dispatch_search();
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::SearchPastes {
            folder_id,
            language,
            ..
        } => {
            assert!(folder_id.is_none());
            assert_eq!(language.as_deref(), Some("python"));
        }
        other => panic!("unexpected command: {:?}", other),
    }
}

#[test]
fn language_filter_stacks_with_primary_collection() {
    let mut harness = make_app();
    let now = Utc::now();
    let code_rust = PasteSummary {
        id: "code-rust".to_string(),
        name: "perf.rs".to_string(),
        language: Some("rust".to_string()),
        content_len: 12,
        updated_at: now,
        folder_id: None,
        tags: Vec::new(),
    };
    let code_python = PasteSummary {
        id: "code-python".to_string(),
        name: "script.py".to_string(),
        language: Some("python".to_string()),
        content_len: 12,
        updated_at: now,
        folder_id: None,
        tags: Vec::new(),
    };
    let config_rust = PasteSummary {
        id: "config-rust".to_string(),
        name: "config.toml".to_string(),
        language: Some("toml".to_string()),
        content_len: 12,
        updated_at: now,
        folder_id: None,
        tags: Vec::new(),
    };
    let config_yaml = PasteSummary {
        id: "config-yaml".to_string(),
        name: "deploy.yaml".to_string(),
        language: Some("yaml".to_string()),
        content_len: 12,
        updated_at: now,
        folder_id: None,
        tags: Vec::new(),
    };
    harness.app.apply_event(CoreEvent::PasteList {
        items: vec![
            code_rust.clone(),
            code_python.clone(),
            config_rust.clone(),
            config_yaml.clone(),
        ],
    });

    harness.app.set_active_collection(SidebarCollection::Code);
    harness
        .app
        .set_active_language_filter(Some("rust".to_string()));
    assert_eq!(harness.app.pastes.len(), 1);
    assert_eq!(harness.app.pastes[0].id, code_rust.id);

    harness.app.set_active_collection(SidebarCollection::Config);
    harness
        .app
        .set_active_language_filter(Some("toml".to_string()));
    assert_eq!(harness.app.pastes.len(), 1);
    assert_eq!(harness.app.pastes[0].id, config_rust.id);

    harness.app.set_active_language_filter(None);
    assert_eq!(harness.app.pastes.len(), 2);
}

#[test]
fn smart_collections_match_time_and_content_facets() {
    let mut harness = make_app();
    let now = Utc::now();
    harness.app.apply_event(CoreEvent::PasteList {
        items: vec![
            PasteSummary {
                id: "today-log".to_string(),
                name: "service.log".to_string(),
                language: Some("text".to_string()),
                content_len: 15,
                updated_at: now,
                folder_id: None,
                tags: vec!["log".to_string()],
            },
            PasteSummary {
                id: "week-link".to_string(),
                name: "https://example.com".to_string(),
                language: None,
                content_len: 20,
                updated_at: now - chrono::Duration::days(2),
                folder_id: None,
                tags: vec!["bookmark".to_string()],
            },
            PasteSummary {
                id: "old-config".to_string(),
                name: "service.toml".to_string(),
                language: Some("toml".to_string()),
                content_len: 10,
                updated_at: now - chrono::Duration::days(40),
                folder_id: None,
                tags: vec!["config".to_string()],
            },
        ],
    });

    harness.app.set_active_collection(SidebarCollection::Today);
    assert_eq!(harness.app.pastes.len(), 1);
    assert_eq!(harness.app.pastes[0].id, "today-log");

    harness.app.set_active_collection(SidebarCollection::Week);
    assert_eq!(harness.app.pastes.len(), 2);

    harness.app.set_active_collection(SidebarCollection::Logs);
    assert_eq!(harness.app.pastes.len(), 1);
    assert_eq!(harness.app.pastes[0].id, "today-log");

    harness.app.set_active_collection(SidebarCollection::Links);
    assert_eq!(harness.app.pastes.len(), 1);
    assert_eq!(harness.app.pastes[0].id, "week-link");
}
