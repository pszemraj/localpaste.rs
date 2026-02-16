//! Collection/search filtering tests for list projection and palette ranking.

use super::*;

#[test]
fn search_results_respect_collection_filter() {
    let mut harness = make_app();
    harness
        .app
        .set_active_collection(SidebarCollection::Unfiled);
    harness.app.set_search_query("rust".to_string());
    harness.app.search_last_sent = "rust".to_string();

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
        folder_id: None,
        language: None,
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
        folder_id: None,
        language: None,
        items: vec![stale],
    });
    assert_eq!(harness.app.pastes.len(), 1);
    assert_eq!(harness.app.pastes[0].id, unfiled.id);
}

#[test]
fn stale_search_results_with_old_language_filter_are_dropped() {
    let mut harness = make_app();
    harness
        .app
        .set_active_language_filter(Some("rust".to_string()));
    harness.app.set_search_query("term".to_string());
    harness.app.search_last_input_at =
        Some(Instant::now() - SEARCH_DEBOUNCE - Duration::from_millis(10));
    harness.app.maybe_dispatch_search();
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::SearchPastes {
            query, language, ..
        } => {
            assert_eq!(query, "term");
            assert_eq!(language.as_deref(), Some("rust"));
        }
        other => panic!("unexpected command: {:?}", other),
    }

    let stale = PasteSummary {
        id: "stale".to_string(),
        name: "stale-python".to_string(),
        language: Some("python".to_string()),
        content_len: 10,
        updated_at: Utc::now(),
        folder_id: None,
        tags: Vec::new(),
    };
    harness.app.apply_event(CoreEvent::SearchResults {
        query: "term".to_string(),
        folder_id: None,
        language: Some("python".to_string()),
        items: vec![stale],
    });

    assert_eq!(
        harness.app.query_perf.search_stale_drops, 1,
        "stale filter-mismatched response should be dropped"
    );
    assert!(
        harness.app.pastes.iter().all(|item| item.id != "stale"),
        "stale result set must not be applied"
    );

    let fresh = PasteSummary {
        id: "fresh".to_string(),
        name: "fresh-rust".to_string(),
        language: Some("rust".to_string()),
        content_len: 12,
        updated_at: Utc::now(),
        folder_id: None,
        tags: Vec::new(),
    };
    harness.app.apply_event(CoreEvent::SearchResults {
        query: "term".to_string(),
        folder_id: None,
        language: Some("rust".to_string()),
        items: vec![fresh.clone()],
    });

    assert_eq!(harness.app.query_perf.search_results_applied, 1);
    assert_eq!(harness.app.pastes.len(), 1);
    assert_eq!(harness.app.pastes[0].id, fresh.id);
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
fn paste_saved_reprojects_non_search_results_for_active_language_filter() {
    let mut harness = make_app();
    let now = Utc::now();
    harness.app.apply_event(CoreEvent::PasteList {
        items: vec![
            PasteSummary {
                id: "alpha".to_string(),
                name: "Alpha".to_string(),
                language: Some("rust".to_string()),
                content_len: 7,
                updated_at: now,
                folder_id: None,
                tags: Vec::new(),
            },
            PasteSummary {
                id: "beta".to_string(),
                name: "Beta".to_string(),
                language: Some("rust".to_string()),
                content_len: 7,
                updated_at: now,
                folder_id: None,
                tags: Vec::new(),
            },
        ],
    });
    harness
        .app
        .set_active_language_filter(Some("rust".to_string()));
    harness.app.save_status = SaveStatus::Saving;
    harness.app.save_in_flight = true;

    let mut saved = Paste::new("content".to_string(), "Alpha".to_string());
    saved.id = "alpha".to_string();
    saved.language = Some("python".to_string());
    saved.language_is_manual = true;
    harness
        .app
        .apply_event(CoreEvent::PasteSaved { paste: saved });

    assert_eq!(harness.app.pastes.len(), 1);
    assert_eq!(harness.app.pastes[0].id, "beta");
    assert_eq!(harness.app.selected_id.as_deref(), Some("beta"));
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::GetPaste { id } => assert_eq!(id, "beta"),
        other => panic!("unexpected command: {:?}", other),
    }
}

#[test]
fn palette_search_results_are_query_scoped_and_can_exceed_list_window() {
    let mut harness = make_app();
    harness.app.command_palette_open = true;
    harness.app.set_command_palette_query("legacy".to_string());
    harness.app.all_pastes = vec![PasteSummary {
        id: "alpha".to_string(),
        name: "Alpha".to_string(),
        language: None,
        content_len: 7,
        updated_at: Utc::now(),
        folder_id: None,
        tags: Vec::new(),
    }];

    harness.app.apply_event(CoreEvent::PaletteSearchResults {
        query: "other".to_string(),
        items: vec![PasteSummary {
            id: "stale".to_string(),
            name: "Stale".to_string(),
            language: None,
            content_len: 1,
            updated_at: Utc::now(),
            folder_id: None,
            tags: Vec::new(),
        }],
    });
    assert!(harness.app.palette_search_results.is_empty());

    harness.app.apply_event(CoreEvent::PaletteSearchResults {
        query: "legacy".to_string(),
        items: vec![PasteSummary {
            id: "old-id".to_string(),
            name: "Legacy note".to_string(),
            language: None,
            content_len: 8,
            updated_at: Utc::now(),
            folder_id: None,
            tags: Vec::new(),
        }],
    });

    assert_eq!(harness.app.palette_search_results.len(), 1);
    assert_eq!(harness.app.palette_search_results[0].id, "old-id");
}

#[test]
fn maybe_dispatch_search_flows_require_debounce_and_dedupe_matrix() {
    enum DispatchKind {
        SidebarSearch,
        PaletteSearch,
    }

    for kind in [DispatchKind::SidebarSearch, DispatchKind::PaletteSearch] {
        let mut harness = make_app();
        match kind {
            DispatchKind::PaletteSearch => {
                harness.app.command_palette_open = true;
                harness.app.set_command_palette_query("alpha".to_string());

                harness.app.palette_search_last_input_at = Some(Instant::now());
                harness.app.maybe_dispatch_palette_search();
                assert!(matches!(
                    harness.cmd_rx.try_recv(),
                    Err(TryRecvError::Empty)
                ));

                harness.app.palette_search_last_input_at =
                    Some(Instant::now() - SEARCH_DEBOUNCE - Duration::from_millis(10));
                harness.app.maybe_dispatch_palette_search();
                match recv_cmd(&harness.cmd_rx) {
                    CoreCmd::SearchPalette { query, limit } => {
                        assert_eq!(query, "alpha");
                        assert_eq!(limit, PALETTE_SEARCH_LIMIT);
                    }
                    other => panic!("unexpected command: {:?}", other),
                }

                harness.app.maybe_dispatch_palette_search();
                assert!(matches!(
                    harness.cmd_rx.try_recv(),
                    Err(TryRecvError::Empty)
                ));
            }
            DispatchKind::SidebarSearch => {
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
                        assert_eq!(limit, localpaste_core::DEFAULT_SEARCH_PASTES_LIMIT);
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

                let now = Utc::now();
                harness.app.apply_event(CoreEvent::PasteList {
                    items: vec![
                        PasteSummary {
                            id: "alpha".to_string(),
                            name: "Alpha".to_string(),
                            language: None,
                            content_len: 7,
                            updated_at: now,
                            folder_id: None,
                            tags: Vec::new(),
                        },
                        PasteSummary {
                            id: "beta".to_string(),
                            name: "Beta".to_string(),
                            language: Some("rust".to_string()),
                            content_len: 4,
                            updated_at: now,
                            folder_id: None,
                            tags: Vec::new(),
                        },
                    ],
                });
                assert!(
                    harness.app.search_last_sent.is_empty(),
                    "list refresh during active search should invalidate cached query"
                );

                harness.app.maybe_dispatch_search();
                match recv_cmd(&harness.cmd_rx) {
                    CoreCmd::SearchPastes { query, .. } => assert_eq!(query, "rust"),
                    other => panic!("unexpected command: {:?}", other),
                }
            }
        }
    }
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
fn clearing_search_restores_list_even_after_cached_query_was_invalidated() {
    let mut harness = make_app();
    let now = Utc::now();
    harness.app.all_pastes = vec![
        PasteSummary {
            id: "alpha".to_string(),
            name: "Alpha".to_string(),
            language: Some("rust".to_string()),
            content_len: 7,
            updated_at: now,
            folder_id: None,
            tags: Vec::new(),
        },
        PasteSummary {
            id: "beta".to_string(),
            name: "Beta".to_string(),
            language: Some("rust".to_string()),
            content_len: 4,
            updated_at: now,
            folder_id: None,
            tags: Vec::new(),
        },
    ];
    harness.app.pastes = vec![PasteSummary {
        id: "search-only".to_string(),
        name: "Search only".to_string(),
        language: Some("rust".to_string()),
        content_len: 3,
        updated_at: now,
        folder_id: None,
        tags: Vec::new(),
    }];
    harness.app.search_query = "rust".to_string();
    harness.app.search_last_sent.clear();

    harness.app.set_search_query(String::new());
    harness.app.maybe_dispatch_search();

    assert_eq!(harness.app.pastes.len(), 2);
    assert_eq!(harness.app.pastes[0].id, "alpha");
    assert_eq!(harness.app.pastes[1].id, "beta");
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
fn language_filter_input_is_normalized_before_search_dispatch() {
    let mut harness = make_app();
    harness
        .app
        .set_active_language_filter(Some("  PyThOn  ".to_string()));
    harness.app.set_search_query("script".to_string());
    harness.app.search_last_input_at =
        Some(Instant::now() - SEARCH_DEBOUNCE - Duration::from_millis(10));

    harness.app.maybe_dispatch_search();
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::SearchPastes { language, .. } => {
            assert_eq!(language.as_deref(), Some("python"));
        }
        other => panic!("unexpected command: {:?}", other),
    }
}

#[test]
fn language_filter_options_dedupe_case_variants() {
    let mut harness = make_app();
    let now = Utc::now();
    harness.app.apply_event(CoreEvent::PasteList {
        items: vec![
            PasteSummary {
                id: "a".to_string(),
                name: "one".to_string(),
                language: Some("python".to_string()),
                content_len: 10,
                updated_at: now,
                folder_id: None,
                tags: Vec::new(),
            },
            PasteSummary {
                id: "b".to_string(),
                name: "two".to_string(),
                language: Some("Python".to_string()),
                content_len: 10,
                updated_at: now,
                folder_id: None,
                tags: Vec::new(),
            },
            PasteSummary {
                id: "c".to_string(),
                name: "three".to_string(),
                language: Some("  PYTHON  ".to_string()),
                content_len: 10,
                updated_at: now,
                folder_id: None,
                tags: Vec::new(),
            },
        ],
    });

    assert_eq!(
        harness.app.language_filter_options(),
        vec!["python".to_string()]
    );
}

#[test]
fn language_filter_aliases_match_in_client_projection() {
    let mut harness = make_app();
    let now = Utc::now();
    harness.app.apply_event(CoreEvent::PasteList {
        items: vec![
            PasteSummary {
                id: "legacy-csharp".to_string(),
                name: "legacy".to_string(),
                language: Some("csharp".to_string()),
                content_len: 10,
                updated_at: now,
                folder_id: None,
                tags: Vec::new(),
            },
            PasteSummary {
                id: "new-cs".to_string(),
                name: "new".to_string(),
                language: Some("cs".to_string()),
                content_len: 10,
                updated_at: now,
                folder_id: None,
                tags: Vec::new(),
            },
        ],
    });

    harness
        .app
        .set_active_language_filter(Some("cs".to_string()));
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
