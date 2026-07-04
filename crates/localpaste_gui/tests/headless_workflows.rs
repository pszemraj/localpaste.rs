//! Headless integration tests for GUI/backend workflows against the embedded API.

mod headless_support;

use headless_support::{recv_event, TestEnv, TEST_MAX_PASTE_SIZE};
use localpaste_core::{
    models::paste::{Paste, UpdatePasteRequest},
    Database,
};
use localpaste_gui::backend::{spawn_backend, CoreCmd, CoreEvent};
use localpaste_server::{LockOwnerId, PasteLockManager};
use ropey::Rope;
use serde_json::json;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::TempDir;
#[test]
fn api_updates_are_visible_to_backend_list() {
    let env = TestEnv::new();
    let locks = Arc::new(PasteLockManager::default());
    let server = env.start_server(locks);

    let client = reqwest::blocking::Client::new();
    let url = format!("http://{}/api/paste", server.addr());
    let created: Paste = client
        .post(&url)
        .json(&serde_json::json!({ "content": "hello from api" }))
        .send()
        .expect("create request")
        .json()
        .expect("parse response");

    let backend = env.spawn_backend();
    backend
        .cmd_tx
        .send(CoreCmd::ListPastes {
            limit: 10,
            folder_id: None,
        })
        .expect("send list");

    match recv_event(&backend.evt_rx) {
        CoreEvent::PasteList { items } => {
            assert!(items.iter().any(|item| item.id == created.id));
        }
        other => panic!("unexpected event: {:?}", other),
    }

    drop(server);
}

#[test]
fn backend_shutdown_drains_queued_update_and_persists_across_reopen() {
    let dir = TempDir::new().expect("temp dir");
    let db_path = dir.path().join("db");
    let db_path_str = db_path.to_string_lossy().to_string();
    let db = Database::new(&db_path_str).expect("db");

    let paste = Paste::new("before-close".to_string(), "shutdown-seed".to_string());
    let paste_id = paste.id.clone();
    db.pastes.create(&paste).expect("create seed paste");

    let mut backend = spawn_backend(db.share().expect("share db"), TEST_MAX_PASTE_SIZE);
    backend
        .cmd_tx
        .send(CoreCmd::UpdatePasteVirtual {
            id: paste_id.clone(),
            content: Rope::from_str("after-close"),
            protected_version_id_ms: None,
        })
        .expect("send update before shutdown");

    backend
        .shutdown_and_join(true, Duration::from_secs(5))
        .expect("shutdown backend");
    drop(backend);
    drop(db);

    let reopened = Database::new(&db_path_str).expect("reopen db");
    let persisted = reopened
        .pastes
        .get(&paste_id)
        .expect("read persisted paste")
        .expect("paste should exist");
    assert_eq!(persisted.content, "after-close");
}

#[test]
fn backend_delete_undo_restores_content_and_version_history_headlessly() {
    let env = TestEnv::new();
    let seed = Paste::new("version-one".to_string(), "undo-versioned".to_string());
    let paste_id = seed.id.clone();
    env.db.pastes.create(&seed).expect("create seed paste");
    env.db
        .pastes
        .update(
            &paste_id,
            UpdatePasteRequest {
                content: Some("version-two".to_string()),
                name: None,
                language: None,
                language_is_manual: None,
                folder_id: None,
                tags: None,
            },
        )
        .expect("update seed paste")
        .expect("paste should exist for update");
    let versions_before = env
        .db
        .pastes
        .list_versions(&paste_id, Some(10))
        .expect("list versions")
        .expect("paste should exist for version list");
    assert_eq!(
        versions_before.len(),
        1,
        "setup should archive the outgoing head before delete"
    );
    let archived_version_id = versions_before[0].version_id_ms;

    let backend = env.spawn_backend();
    backend
        .cmd_tx
        .send(CoreCmd::DeletePaste {
            id: paste_id.clone(),
        })
        .expect("send delete");
    let undo_token = match recv_event(&backend.evt_rx) {
        CoreEvent::PasteDeleted { id, undo_token } => {
            assert_eq!(id, paste_id);
            undo_token.expect("delete should include undo token")
        }
        other => panic!("expected PasteDeleted event, got {:?}", other),
    };
    assert!(
        env.db
            .pastes
            .get(&paste_id)
            .expect("get after delete")
            .is_none(),
        "delete should remove the live paste before undo"
    );

    backend
        .cmd_tx
        .send(CoreCmd::RestoreDeletedPaste {
            undo_token: undo_token.clone(),
        })
        .expect("send restore");
    match recv_event(&backend.evt_rx) {
        CoreEvent::PasteRestored {
            paste,
            undo_token: restored_token,
        } => {
            assert_eq!(paste.id, paste_id);
            assert_eq!(paste.content, "version-two");
            assert_eq!(restored_token, undo_token);
        }
        other => panic!("expected PasteRestored event, got {:?}", other),
    }

    backend
        .cmd_tx
        .send(CoreCmd::ListPasteVersions {
            id: paste_id.clone(),
            limit: 10,
        })
        .expect("send version list");
    match recv_event(&backend.evt_rx) {
        CoreEvent::PasteVersionsLoaded { id, items } => {
            assert_eq!(id, paste_id);
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].version_id_ms, archived_version_id);
        }
        other => panic!("expected PasteVersionsLoaded event, got {:?}", other),
    }

    backend
        .cmd_tx
        .send(CoreCmd::GetPasteVersion {
            id: paste_id.clone(),
            version_id_ms: archived_version_id,
        })
        .expect("send version fetch");
    match recv_event(&backend.evt_rx) {
        CoreEvent::PasteVersionLoaded { snapshot } => {
            assert_eq!(snapshot.paste_id, paste_id);
            assert_eq!(snapshot.version_id_ms, archived_version_id);
            assert_eq!(snapshot.content, "version-one");
        }
        other => panic!("expected PasteVersionLoaded event, got {:?}", other),
    }
}

#[test]
fn locked_paste_blocks_api_mutations_until_lock_is_released() {
    enum MutationKind {
        Delete,
        Update,
    }

    let env = TestEnv::new();
    let locks = Arc::new(PasteLockManager::default());
    let server = env.start_server(locks.clone());
    let client = reqwest::blocking::Client::new();

    let cases = [
        (MutationKind::Delete, "delete-owner"),
        (MutationKind::Update, "update-owner"),
    ];

    for (mutation, owner_id) in cases {
        let paste = Paste::new("locked content".to_string(), "locked".to_string());
        let paste_id = paste.id.clone();
        env.db.pastes.create(&paste).expect("create paste");
        let owner = LockOwnerId::new(owner_id.to_string());
        locks
            .acquire(&paste_id, &owner)
            .expect("acquire lock for mutation test");

        let url = format!("http://{}/api/paste/{}", server.addr(), paste_id);
        let blocked = match mutation {
            MutationKind::Delete => client.delete(&url).send().expect("delete request"),
            MutationKind::Update => client
                .put(&url)
                .json(&json!({ "content": "updated" }))
                .send()
                .expect("update request"),
        };
        assert_eq!(blocked.status(), reqwest::StatusCode::LOCKED);

        locks
            .release(&paste_id, &owner)
            .expect("release lock for mutation test");

        let allowed = match mutation {
            MutationKind::Delete => client.delete(&url).send().expect("delete request"),
            MutationKind::Update => client
                .put(&url)
                .json(&json!({ "content": "updated" }))
                .send()
                .expect("update request"),
        };
        assert!(allowed.status().is_success());
    }
}

#[test]
fn backend_delete_rejects_foreign_lock_holder_and_preserves_paste() {
    let env = TestEnv::new();
    let locks = Arc::new(PasteLockManager::default());
    let backend = env.spawn_backend_with_locks(locks.clone());

    backend
        .cmd_tx
        .send(CoreCmd::CreatePaste {
            content: "locked body".to_string(),
        })
        .expect("create paste");
    let paste_id = match recv_event(&backend.evt_rx) {
        CoreEvent::PasteCreated { paste } => paste.id,
        other => panic!("unexpected event: {:?}", other),
    };

    let foreign_owner = LockOwnerId::new("foreign-owner".to_string());
    locks
        .acquire(&paste_id, &foreign_owner)
        .expect("acquire foreign lock holder");

    backend
        .cmd_tx
        .send(CoreCmd::DeletePaste {
            id: paste_id.clone(),
        })
        .expect("send delete");
    match recv_event(&backend.evt_rx) {
        CoreEvent::Error { message, .. } => {
            assert!(
                message.contains("open for editing"),
                "expected lock rejection, got: {}",
                message
            );
        }
        other => panic!("unexpected event: {:?}", other),
    }

    backend
        .cmd_tx
        .send(CoreCmd::GetPaste {
            id: paste_id.clone(),
        })
        .expect("send get after rejected delete");
    match recv_event(&backend.evt_rx) {
        CoreEvent::PasteLoaded { paste } => assert_eq!(paste.id, paste_id),
        other => panic!("unexpected event: {:?}", other),
    }

    locks
        .release(&paste_id, &foreign_owner)
        .expect("release foreign lock holder");
}

#[test]
fn backend_update_paths_reject_foreign_lock_holder_and_preserve_paste() {
    let env = TestEnv::new();
    let locks = Arc::new(PasteLockManager::default());
    let backend = env.spawn_backend_with_locks(locks.clone());

    backend
        .cmd_tx
        .send(CoreCmd::CreatePaste {
            content: "locked body".to_string(),
        })
        .expect("create paste");
    let baseline = match recv_event(&backend.evt_rx) {
        CoreEvent::PasteCreated { paste } => paste,
        other => panic!("unexpected event: {:?}", other),
    };
    let paste_id = baseline.id.clone();

    let foreign_owner = LockOwnerId::new("foreign-owner".to_string());
    locks
        .acquire(&paste_id, &foreign_owner)
        .expect("acquire foreign lock holder");

    backend
        .cmd_tx
        .send(CoreCmd::UpdatePasteVirtual {
            id: paste_id.clone(),
            content: Rope::from_str("mutated-body"),
            protected_version_id_ms: None,
        })
        .expect("send content update");
    match recv_event(&backend.evt_rx) {
        CoreEvent::Error { message, .. } => {
            assert!(
                message.contains("open for editing"),
                "expected lock rejection, got: {}",
                message
            );
        }
        other => panic!("unexpected event: {:?}", other),
    }

    backend
        .cmd_tx
        .send(CoreCmd::UpdatePasteMeta {
            id: paste_id.clone(),
            name: Some("mutated-name".to_string()),
            language: None,
            language_is_manual: None,
            folder_id: None,
            tags: None,
        })
        .expect("send metadata update");
    match recv_event(&backend.evt_rx) {
        CoreEvent::Error { message, .. } => {
            assert!(
                message.contains("open for editing"),
                "expected lock rejection, got: {}",
                message
            );
        }
        other => panic!("unexpected event: {:?}", other),
    }

    backend
        .cmd_tx
        .send(CoreCmd::GetPaste {
            id: paste_id.clone(),
        })
        .expect("send get after rejected updates");
    match recv_event(&backend.evt_rx) {
        CoreEvent::PasteLoaded { paste } => {
            assert_eq!(paste.id, paste_id);
            assert_eq!(paste.content, baseline.content);
            assert_eq!(paste.name, baseline.name);
        }
        other => panic!("unexpected event: {:?}", other),
    }

    locks
        .release(&paste_id, &foreign_owner)
        .expect("release foreign lock holder");
}

#[test]
fn locked_descendant_blocks_backend_folder_delete() {
    let env = TestEnv::new();
    let locks = Arc::new(PasteLockManager::default());
    let backend = env.spawn_backend_with_locks(locks.clone());

    backend
        .cmd_tx
        .send(CoreCmd::CreateFolder {
            name: "locked-root".to_string(),
            parent_id: None,
        })
        .expect("create folder");
    let folder_id = match recv_event(&backend.evt_rx) {
        CoreEvent::FolderSaved { folder } => folder.id,
        other => panic!("unexpected event: {:?}", other),
    };

    backend
        .cmd_tx
        .send(CoreCmd::CreatePaste {
            content: "locked body".to_string(),
        })
        .expect("create paste");
    let paste_id = match recv_event(&backend.evt_rx) {
        CoreEvent::PasteCreated { paste } => paste.id,
        other => panic!("unexpected event: {:?}", other),
    };

    backend
        .cmd_tx
        .send(CoreCmd::UpdatePasteMeta {
            id: paste_id.clone(),
            name: None,
            language: None,
            language_is_manual: None,
            folder_id: Some(folder_id.clone()),
            tags: None,
        })
        .expect("assign folder");
    match recv_event(&backend.evt_rx) {
        CoreEvent::PasteMetaSaved { paste } => {
            assert_eq!(paste.folder_id.as_deref(), Some(folder_id.as_str()));
        }
        other => panic!("unexpected event: {:?}", other),
    }

    let owner = LockOwnerId::new("folder-owner".to_string());
    locks
        .acquire(&paste_id, &owner)
        .expect("acquire lock for folder delete test");
    backend
        .cmd_tx
        .send(CoreCmd::DeleteFolder {
            id: folder_id.clone(),
        })
        .expect("delete folder");
    match recv_event(&backend.evt_rx) {
        CoreEvent::Error { message, .. } => {
            assert!(
                message.contains("locked paste"),
                "expected lock rejection, got: {}",
                message
            );
        }
        other => panic!("unexpected event: {:?}", other),
    }

    backend
        .cmd_tx
        .send(CoreCmd::GetPaste {
            id: paste_id.clone(),
        })
        .expect("get locked paste");
    match recv_event(&backend.evt_rx) {
        CoreEvent::PasteLoaded { paste } => {
            assert_eq!(paste.id, paste_id);
            assert_eq!(paste.folder_id.as_deref(), Some(folder_id.as_str()));
        }
        other => panic!("unexpected event: {:?}", other),
    }
}

#[test]
fn metadata_update_persists_and_manual_auto_language_transitions_work() {
    let env = TestEnv::new();
    let locks = Arc::new(PasteLockManager::default());
    let _server = env.start_server(locks);
    let backend = env.spawn_backend();

    backend
        .cmd_tx
        .send(CoreCmd::CreateFolder {
            name: "Scripts".to_string(),
            parent_id: None,
        })
        .expect("create folder");
    let folder_id = match recv_event(&backend.evt_rx) {
        CoreEvent::FolderSaved { folder } => folder.id,
        other => panic!("unexpected event: {:?}", other),
    };

    backend
        .cmd_tx
        .send(CoreCmd::CreatePaste {
            content: "print('hello')".to_string(),
        })
        .expect("create paste");
    let paste_id = match recv_event(&backend.evt_rx) {
        CoreEvent::PasteCreated { paste } => paste.id,
        other => panic!("unexpected event: {:?}", other),
    };

    backend
        .cmd_tx
        .send(CoreCmd::UpdatePasteMeta {
            id: paste_id.clone(),
            name: Some("script-one".to_string()),
            language: Some("python".to_string()),
            language_is_manual: Some(true),
            folder_id: Some(folder_id.clone()),
            tags: Some(vec!["tooling".to_string(), "python".to_string()]),
        })
        .expect("update metadata manual");
    match recv_event(&backend.evt_rx) {
        CoreEvent::PasteMetaSaved { paste } => {
            assert_eq!(paste.name, "script-one");
            assert_eq!(paste.language.as_deref(), Some("python"));
            assert!(paste.language_is_manual);
            assert_eq!(paste.folder_id.as_deref(), Some(folder_id.as_str()));
            assert_eq!(
                paste.tags,
                vec!["tooling".to_string(), "python".to_string()]
            );
        }
        other => panic!("unexpected event: {:?}", other),
    }

    backend
        .cmd_tx
        .send(CoreCmd::UpdatePasteMeta {
            id: paste_id.clone(),
            name: Some("script-one".to_string()),
            language: None,
            language_is_manual: Some(false),
            folder_id: Some(folder_id.clone()),
            tags: Some(vec!["tooling".to_string()]),
        })
        .expect("update metadata auto");
    match recv_event(&backend.evt_rx) {
        CoreEvent::PasteMetaSaved { paste } => {
            assert!(!paste.language_is_manual);
            assert!(paste.language.is_none());
            assert_eq!(paste.tags, vec!["tooling".to_string()]);
        }
        other => panic!("unexpected event: {:?}", other),
    }

    backend
        .cmd_tx
        .send(CoreCmd::SearchPastes {
            query: "script".to_string(),
            limit: 10,
            folder_id: Some(folder_id),
            language: None,
        })
        .expect("search");
    match recv_event(&backend.evt_rx) {
        CoreEvent::SearchResults { items, .. } => {
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].id, paste_id);
        }
        other => panic!("unexpected event: {:?}", other),
    }
}

#[test]
fn backend_search_matches_full_content_and_derived_metadata() {
    let env = TestEnv::new();

    let content_only = Paste::new(
        "searchable exact substring only in content".to_string(),
        "plain-title".to_string(),
    );
    let mut metadata_only = Paste::new("plain body".to_string(), "docker compose".to_string());
    metadata_only.tags = vec!["postgres".to_string()];
    let derived_terms = Paste::new(
        "fsdp2 validation failed after cublaslt retry\nfsdp2 validation repeated\n".to_string(),
        "derived-terms".to_string(),
    );

    env.db
        .pastes
        .create(&content_only)
        .expect("create content-only");
    env.db
        .pastes
        .create(&metadata_only)
        .expect("create metadata-only");
    env.db
        .pastes
        .create(&derived_terms)
        .expect("create derived-terms");

    let backend = env.spawn_backend();
    backend
        .cmd_tx
        .send(CoreCmd::SearchPastes {
            query: "SEARCHABLE EXACT SUBSTRING".to_string(),
            limit: 10,
            folder_id: None,
            language: None,
        })
        .expect("send search");

    match recv_event(&backend.evt_rx) {
        CoreEvent::SearchResults { query, items, .. } => {
            let ids: Vec<&str> = items.iter().map(|item| item.id.as_str()).collect();
            assert_eq!(query, "SEARCHABLE EXACT SUBSTRING");
            assert_eq!(ids, vec![content_only.id.as_str()]);
        }
        other => panic!("unexpected event: {:?}", other),
    }

    backend
        .cmd_tx
        .send(CoreCmd::SearchPastes {
            query: "fsdp2 cublaslt".to_string(),
            limit: 10,
            folder_id: None,
            language: None,
        })
        .expect("send derived search");

    match recv_event(&backend.evt_rx) {
        CoreEvent::SearchResults { query, items, .. } => {
            let ids: Vec<&str> = items.iter().map(|item| item.id.as_str()).collect();
            assert_eq!(query, "fsdp2 cublaslt");
            assert_eq!(ids, vec![derived_terms.id.as_str()]);
        }
        other => panic!("unexpected event: {:?}", other),
    }
}

#[test]
fn list_and_search_latency_stay_within_reasonable_headless_budget() {
    let env = TestEnv::new();

    for idx in 0..1500 {
        let content = if idx % 250 == 0 {
            format!("payload {} needle marker", idx)
        } else {
            format!("payload {} filler", idx)
        };
        let name = if idx % 250 == 0 {
            format!("needle-item-{}", idx)
        } else {
            format!("item-{}", idx)
        };
        let paste = Paste::new(content, name);
        env.db.pastes.create(&paste).expect("seed paste");
    }

    let backend = env.spawn_backend();

    let list_start = Instant::now();
    backend
        .cmd_tx
        .send(CoreCmd::ListPastes {
            limit: 512,
            folder_id: None,
        })
        .expect("send list");
    let list_elapsed = list_start.elapsed();
    match recv_event(&backend.evt_rx) {
        CoreEvent::PasteList { items } => assert_eq!(items.len(), 512),
        other => panic!("unexpected event: {:?}", other),
    }

    let search_start = Instant::now();
    backend
        .cmd_tx
        .send(CoreCmd::SearchPastes {
            query: "needle".to_string(),
            limit: 32,
            folder_id: None,
            language: None,
        })
        .expect("send search");
    let search_elapsed = search_start.elapsed();
    match recv_event(&backend.evt_rx) {
        CoreEvent::SearchResults { items, .. } => {
            assert!(!items.is_empty());
            assert!(items.len() <= 32);
        }
        other => panic!("unexpected event: {:?}", other),
    }

    assert!(
        list_elapsed < Duration::from_secs(5),
        "list exceeded budget: {:?}",
        list_elapsed
    );
    assert!(
        search_elapsed < Duration::from_secs(5),
        "search exceeded budget: {:?}",
        search_elapsed
    );
}
