//! Background worker thread for database access.

use crate::backend::{CoreCmd, CoreErrorSource, CoreEvent, PasteSummary};
use crossbeam_channel::{unbounded, Receiver, Sender};
use localpaste_core::{
    config::env_flag_enabled,
    db::TransactionOps,
    folder_ops::{delete_folder_tree_and_migrate, introduces_cycle},
    models::{folder::Folder, paste::UpdatePasteRequest},
    naming, Database,
};
use std::thread;
use std::time::{Duration, Instant};
use tracing::{error, info};

/// Handle for sending commands to, and receiving events from, the backend worker.
pub struct BackendHandle {
    pub cmd_tx: Sender<CoreCmd>,
    pub evt_rx: Receiver<CoreEvent>,
}

fn send_error(evt_tx: &Sender<CoreEvent>, source: CoreErrorSource, message: String) {
    let _ = evt_tx.send(CoreEvent::Error { source, message });
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ListCacheKey {
    limit: usize,
    folder_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SearchCacheKey {
    query: String,
    limit: usize,
    folder_id: Option<String>,
    language: Option<String>,
}

#[derive(Debug, Default)]
struct QueryCache {
    list_key: Option<ListCacheKey>,
    list_items: Option<Vec<PasteSummary>>,
    list_cached_at: Option<Instant>,
    search_key: Option<SearchCacheKey>,
    search_items: Option<Vec<PasteSummary>>,
    search_cached_at: Option<Instant>,
    list_hits: u64,
    list_misses: u64,
    search_hits: u64,
    search_misses: u64,
    invalidations: u64,
}

impl QueryCache {
    fn invalidate(&mut self) {
        if self.list_key.is_some()
            || self.list_items.is_some()
            || self.list_cached_at.is_some()
            || self.search_key.is_some()
            || self.search_items.is_some()
            || self.search_cached_at.is_some()
        {
            self.list_key = None;
            self.list_items = None;
            self.list_cached_at = None;
            self.search_key = None;
            self.search_items = None;
            self.search_cached_at = None;
            self.invalidations = self.invalidations.saturating_add(1);
        }
    }
}

// List/search cache entries intentionally expire quickly so out-of-band
// mutations (embedded API/CLI) become visible without requiring local invalidation.
const QUERY_CACHE_MAX_AGE: Duration = Duration::from_millis(500);

fn log_query_perf(
    enabled: bool,
    cache: &QueryCache,
    op: &str,
    cache_hit: bool,
    elapsed_ms: f64,
    items: usize,
) {
    if !enabled {
        return;
    }
    info!(
        target: "localpaste_gui::backend_perf",
        op = op,
        cache_hit = cache_hit,
        elapsed_ms = elapsed_ms,
        items = items,
        list_hits = cache.list_hits,
        list_misses = cache.list_misses,
        search_hits = cache.search_hits,
        search_misses = cache.search_misses,
        cache_invalidations = cache.invalidations,
        "backend list/search perf"
    );
}

/// Spawn the backend worker thread that performs blocking database access.
///
/// All I/O stays off the UI thread; the worker replies with [`CoreEvent`] values
/// that are polled each frame.
///
/// # Returns
/// A [`BackendHandle`] containing the command sender and event receiver.
///
/// # Panics
/// Panics if the worker thread cannot be spawned.
pub fn spawn_backend(db: Database) -> BackendHandle {
    let (cmd_tx, cmd_rx) = unbounded();
    let (evt_tx, evt_rx) = unbounded();

    thread::Builder::new()
        .name("localpaste-gui-backend".to_string())
        .spawn(move || {
            let perf_log_enabled = env_flag_enabled("LOCALPASTE_BACKEND_PERF_LOG");
            let mut query_cache = QueryCache::default();
            for cmd in cmd_rx.iter() {
                match cmd {
                    CoreCmd::ListPastes { limit, folder_id } => {
                        let started = Instant::now();
                        let key = ListCacheKey {
                            limit,
                            folder_id: folder_id.clone(),
                        };
                        if query_cache.list_key.as_ref() == Some(&key) {
                            if let (Some(items), Some(cached_at)) =
                                (query_cache.list_items.clone(), query_cache.list_cached_at)
                            {
                                if cached_at.elapsed() <= QUERY_CACHE_MAX_AGE {
                                    query_cache.list_hits = query_cache.list_hits.saturating_add(1);
                                    log_query_perf(
                                        perf_log_enabled,
                                        &query_cache,
                                        "list",
                                        true,
                                        started.elapsed().as_secs_f64() * 1000.0,
                                        items.len(),
                                    );
                                    let _ = evt_tx.send(CoreEvent::PasteList { items });
                                    continue;
                                }
                            }
                        }
                        query_cache.list_misses = query_cache.list_misses.saturating_add(1);
                        match db.pastes.list_meta(limit, folder_id) {
                            Ok(metas) => {
                                let items: Vec<PasteSummary> =
                                    metas.iter().map(PasteSummary::from_meta).collect();
                                query_cache.list_key = Some(key);
                                query_cache.list_items = Some(items.clone());
                                query_cache.list_cached_at = Some(Instant::now());
                                log_query_perf(
                                    perf_log_enabled,
                                    &query_cache,
                                    "list",
                                    false,
                                    started.elapsed().as_secs_f64() * 1000.0,
                                    items.len(),
                                );
                                let _ = evt_tx.send(CoreEvent::PasteList { items });
                            }
                            Err(err) => {
                                error!("backend list failed: {}", err);
                                send_error(
                                    &evt_tx,
                                    CoreErrorSource::Other,
                                    format!("List failed: {}", err),
                                );
                            }
                        }
                    }
                    CoreCmd::SearchPastes {
                        query,
                        limit,
                        folder_id,
                        language,
                    } => {
                        let started = Instant::now();
                        let key = SearchCacheKey {
                            query: query.clone(),
                            limit,
                            folder_id: folder_id.clone(),
                            language: language.clone(),
                        };
                        if query_cache.search_key.as_ref() == Some(&key) {
                            if let (Some(items), Some(cached_at)) = (
                                query_cache.search_items.clone(),
                                query_cache.search_cached_at,
                            ) {
                                if cached_at.elapsed() <= QUERY_CACHE_MAX_AGE {
                                    query_cache.search_hits =
                                        query_cache.search_hits.saturating_add(1);
                                    log_query_perf(
                                        perf_log_enabled,
                                        &query_cache,
                                        "search",
                                        true,
                                        started.elapsed().as_secs_f64() * 1000.0,
                                        items.len(),
                                    );
                                    let _ = evt_tx.send(CoreEvent::SearchResults { query, items });
                                    continue;
                                }
                            }
                        }
                        query_cache.search_misses = query_cache.search_misses.saturating_add(1);
                        match db.pastes.search_meta(&query, limit, folder_id, language) {
                            Ok(metas) => {
                                let items: Vec<PasteSummary> =
                                    metas.iter().map(PasteSummary::from_meta).collect();
                                query_cache.search_key = Some(key);
                                query_cache.search_items = Some(items.clone());
                                query_cache.search_cached_at = Some(Instant::now());
                                log_query_perf(
                                    perf_log_enabled,
                                    &query_cache,
                                    "search",
                                    false,
                                    started.elapsed().as_secs_f64() * 1000.0,
                                    items.len(),
                                );
                                let _ = evt_tx.send(CoreEvent::SearchResults { query, items });
                            }
                            Err(err) => {
                                error!("backend search failed: {}", err);
                                send_error(
                                    &evt_tx,
                                    CoreErrorSource::Other,
                                    format!("Search failed: {}", err),
                                );
                            }
                        }
                    }
                    CoreCmd::SearchPalette { query, limit } => {
                        let started = Instant::now();
                        let key = SearchCacheKey {
                            query: query.clone(),
                            limit,
                            folder_id: None,
                            language: None,
                        };
                        if query_cache.search_key.as_ref() == Some(&key) {
                            if let (Some(items), Some(cached_at)) = (
                                query_cache.search_items.clone(),
                                query_cache.search_cached_at,
                            ) {
                                if cached_at.elapsed() <= QUERY_CACHE_MAX_AGE {
                                    query_cache.search_hits =
                                        query_cache.search_hits.saturating_add(1);
                                    log_query_perf(
                                        perf_log_enabled,
                                        &query_cache,
                                        "palette_search",
                                        true,
                                        started.elapsed().as_secs_f64() * 1000.0,
                                        items.len(),
                                    );
                                    let _ = evt_tx
                                        .send(CoreEvent::PaletteSearchResults { query, items });
                                    continue;
                                }
                            }
                        }
                        query_cache.search_misses = query_cache.search_misses.saturating_add(1);
                        match db.pastes.search_meta(&query, limit, None, None) {
                            Ok(metas) => {
                                let items: Vec<PasteSummary> =
                                    metas.iter().map(PasteSummary::from_meta).collect();
                                query_cache.search_key = Some(key);
                                query_cache.search_items = Some(items.clone());
                                query_cache.search_cached_at = Some(Instant::now());
                                log_query_perf(
                                    perf_log_enabled,
                                    &query_cache,
                                    "palette_search",
                                    false,
                                    started.elapsed().as_secs_f64() * 1000.0,
                                    items.len(),
                                );
                                let _ =
                                    evt_tx.send(CoreEvent::PaletteSearchResults { query, items });
                            }
                            Err(err) => {
                                error!("backend palette search failed: {}", err);
                                send_error(
                                    &evt_tx,
                                    CoreErrorSource::Other,
                                    format!("Palette search failed: {}", err),
                                );
                            }
                        }
                    }
                    CoreCmd::GetPaste { id } => match db.pastes.get(&id) {
                        Ok(Some(paste)) => {
                            let _ = evt_tx.send(CoreEvent::PasteLoaded { paste });
                        }
                        Ok(None) => {
                            let _ = evt_tx.send(CoreEvent::PasteMissing { id });
                        }
                        Err(err) => {
                            error!("backend get failed: {}", err);
                            let _ = evt_tx.send(CoreEvent::PasteLoadFailed {
                                id,
                                message: format!("Get failed: {}", err),
                            });
                        }
                    },
                    CoreCmd::CreatePaste { content } => {
                        let inferred = localpaste_core::models::paste::detect_language(&content);
                        let name = naming::generate_name_for_content(&content, inferred.as_deref());
                        let paste = localpaste_core::models::paste::Paste::new(content, name);
                        match db.pastes.create(&paste) {
                            Ok(()) => {
                                query_cache.invalidate();
                                let _ = evt_tx.send(CoreEvent::PasteCreated { paste });
                            }
                            Err(err) => {
                                error!("backend create failed: {}", err);
                                send_error(
                                    &evt_tx,
                                    CoreErrorSource::Other,
                                    format!("Create failed: {}", err),
                                );
                            }
                        }
                    }
                    CoreCmd::UpdatePaste { id, content } => {
                        let update = UpdatePasteRequest {
                            content: Some(content),
                            name: None,
                            language: None,
                            language_is_manual: None,
                            folder_id: None,
                            tags: None,
                        };
                        match db.pastes.update(&id, update) {
                            Ok(Some(paste)) => {
                                query_cache.invalidate();
                                let _ = evt_tx.send(CoreEvent::PasteSaved { paste });
                            }
                            Ok(None) => {
                                query_cache.invalidate();
                                let _ = evt_tx.send(CoreEvent::PasteMissing { id });
                            }
                            Err(err) => {
                                error!("backend update failed: {}", err);
                                send_error(
                                    &evt_tx,
                                    CoreErrorSource::SaveContent,
                                    format!("Update failed: {}", err),
                                );
                            }
                        }
                    }
                    CoreCmd::UpdatePasteMeta {
                        id,
                        name,
                        language,
                        language_is_manual,
                        folder_id,
                        tags,
                    } => {
                        let _existing = match db.pastes.get(&id) {
                            Ok(Some(paste)) => paste,
                            Ok(None) => {
                                let _ = evt_tx.send(CoreEvent::PasteMissing { id });
                                continue;
                            }
                            Err(err) => {
                                error!("backend metadata load failed: {}", err);
                                send_error(
                                    &evt_tx,
                                    CoreErrorSource::SaveMetadata,
                                    format!("Metadata update failed: {}", err),
                                );
                                continue;
                            }
                        };

                        let normalized_folder_id = folder_id.map(|fid| {
                            let trimmed = fid.trim().to_string();
                            if trimmed.is_empty() {
                                String::new()
                            } else {
                                trimmed
                            }
                        });

                        if let Some(folder_id) =
                            normalized_folder_id.as_ref().filter(|fid| !fid.is_empty())
                        {
                            match db.folders.get(folder_id) {
                                Ok(Some(_)) => {}
                                Ok(None) => {
                                    send_error(
                                        &evt_tx,
                                        CoreErrorSource::SaveMetadata,
                                        format!(
                                            "Metadata update failed: folder '{}' does not exist",
                                            folder_id
                                        ),
                                    );
                                    continue;
                                }
                                Err(err) => {
                                    error!("backend folder lookup failed: {}", err);
                                    send_error(
                                        &evt_tx,
                                        CoreErrorSource::SaveMetadata,
                                        format!("Metadata update failed: {}", err),
                                    );
                                    continue;
                                }
                            }
                        }

                        let update = UpdatePasteRequest {
                            content: None,
                            name,
                            language,
                            language_is_manual,
                            folder_id: normalized_folder_id.clone(),
                            tags,
                        };

                        let result = if normalized_folder_id.is_some() {
                            let new_folder_id = normalized_folder_id.clone().and_then(|f| {
                                if f.is_empty() {
                                    None
                                } else {
                                    Some(f)
                                }
                            });
                            TransactionOps::move_paste_between_folders(
                                &db,
                                &id,
                                new_folder_id.as_deref(),
                                update,
                            )
                        } else {
                            db.pastes.update(&id, update)
                        };

                        match result {
                            Ok(Some(paste)) => {
                                query_cache.invalidate();
                                let _ = evt_tx.send(CoreEvent::PasteMetaSaved { paste });
                            }
                            Ok(None) => {
                                query_cache.invalidate();
                                let _ = evt_tx.send(CoreEvent::PasteMissing { id });
                            }
                            Err(err) => {
                                error!("backend metadata update failed: {}", err);
                                send_error(
                                    &evt_tx,
                                    CoreErrorSource::SaveMetadata,
                                    format!("Metadata update failed: {}", err),
                                );
                            }
                        }
                    }
                    CoreCmd::DeletePaste { id } => {
                        let _existing = match db.pastes.get(&id) {
                            Ok(Some(paste)) => paste,
                            Ok(None) => {
                                let _ = evt_tx.send(CoreEvent::PasteMissing { id });
                                continue;
                            }
                            Err(err) => {
                                error!("backend delete failed during lookup: {}", err);
                                send_error(
                                    &evt_tx,
                                    CoreErrorSource::Other,
                                    format!("Delete failed: {}", err),
                                );
                                continue;
                            }
                        };

                        let deleted = TransactionOps::delete_paste_with_folder(&db, &id);

                        match deleted {
                            Ok(true) => {
                                query_cache.invalidate();
                                let _ = evt_tx.send(CoreEvent::PasteDeleted { id });
                            }
                            Ok(false) => {
                                query_cache.invalidate();
                                let _ = evt_tx.send(CoreEvent::PasteMissing { id });
                            }
                            Err(err) => {
                                error!("backend delete failed: {}", err);
                                send_error(
                                    &evt_tx,
                                    CoreErrorSource::Other,
                                    format!("Delete failed: {}", err),
                                );
                            }
                        }
                    }
                    CoreCmd::ListFolders => match db.folders.list() {
                        Ok(items) => {
                            let _ = evt_tx.send(CoreEvent::FoldersLoaded { items });
                        }
                        Err(err) => {
                            error!("backend list folders failed: {}", err);
                            send_error(
                                &evt_tx,
                                CoreErrorSource::Other,
                                format!("List folders failed: {}", err),
                            );
                        }
                    },
                    CoreCmd::CreateFolder { name, parent_id } => {
                        let normalized_parent = parent_id
                            .map(|pid| pid.trim().to_string())
                            .filter(|pid| !pid.is_empty());
                        if let Some(parent_id) = normalized_parent.as_deref() {
                            match db.folders.get(parent_id) {
                                Ok(Some(_)) => {}
                                Ok(None) => {
                                    send_error(
                                        &evt_tx,
                                        CoreErrorSource::Other,
                                        format!(
                                            "Create folder failed: parent '{}' does not exist",
                                            parent_id
                                        ),
                                    );
                                    continue;
                                }
                                Err(err) => {
                                    send_error(
                                        &evt_tx,
                                        CoreErrorSource::Other,
                                        format!("Create folder failed: {}", err),
                                    );
                                    continue;
                                }
                            }
                        }

                        let folder = Folder::with_parent(name, normalized_parent);
                        match db.folders.create(&folder) {
                            Ok(()) => {
                                query_cache.invalidate();
                                let _ = evt_tx.send(CoreEvent::FolderSaved { folder });
                            }
                            Err(err) => {
                                error!("backend create folder failed: {}", err);
                                send_error(
                                    &evt_tx,
                                    CoreErrorSource::Other,
                                    format!("Create folder failed: {}", err),
                                );
                            }
                        }
                    }
                    CoreCmd::UpdateFolder {
                        id,
                        name,
                        parent_id,
                    } => {
                        // Preserve API semantics:
                        // - `None` => leave parent unchanged
                        // - `Some("")` => clear parent (top-level)
                        // - `Some("id")` => set explicit parent
                        let parent_update = parent_id.map(|pid| pid.trim().to_string());
                        let normalized_parent =
                            parent_update.as_ref().and_then(|pid| match pid.trim() {
                                "" => None,
                                trimmed => Some(trimmed),
                            });
                        if normalized_parent == Some(id.as_str()) {
                            send_error(
                                &evt_tx,
                                CoreErrorSource::Other,
                                "Update folder failed: folder cannot be its own parent".to_string(),
                            );
                            continue;
                        }

                        if let Some(parent_id) = normalized_parent {
                            let folders = match db.folders.list() {
                                Ok(folders) => folders,
                                Err(err) => {
                                    send_error(
                                        &evt_tx,
                                        CoreErrorSource::Other,
                                        format!("Update folder failed: {}", err),
                                    );
                                    continue;
                                }
                            };

                            if folders.iter().all(|f| f.id != parent_id) {
                                send_error(
                                    &evt_tx,
                                    CoreErrorSource::Other,
                                    format!(
                                        "Update folder failed: parent '{}' does not exist",
                                        parent_id
                                    ),
                                );
                                continue;
                            }

                            if introduces_cycle(&folders, &id, parent_id) {
                                send_error(
                                    &evt_tx,
                                    CoreErrorSource::Other,
                                    "Update folder failed: would create cycle".to_string(),
                                );
                                continue;
                            }
                        }

                        match db.folders.update(&id, name, parent_update) {
                            Ok(Some(folder)) => {
                                query_cache.invalidate();
                                let _ = evt_tx.send(CoreEvent::FolderSaved { folder });
                            }
                            Ok(None) => {
                                send_error(
                                    &evt_tx,
                                    CoreErrorSource::Other,
                                    "Update folder failed: folder not found".to_string(),
                                );
                            }
                            Err(err) => {
                                error!("backend update folder failed: {}", err);
                                send_error(
                                    &evt_tx,
                                    CoreErrorSource::Other,
                                    format!("Update folder failed: {}", err),
                                );
                            }
                        }
                    }
                    CoreCmd::DeleteFolder { id } => {
                        match delete_folder_tree_and_migrate(&db, &id) {
                            Ok(_) => {
                                query_cache.invalidate();
                                let _ = evt_tx.send(CoreEvent::FolderDeleted { id });
                            }
                            Err(err) => {
                                error!("backend delete folder failed: {}", err);
                                send_error(
                                    &evt_tx,
                                    CoreErrorSource::Other,
                                    format!("Delete folder failed: {}", err),
                                );
                            }
                        }
                    }
                }
            }
        })
        .expect("spawn backend thread");

    BackendHandle { cmd_tx, evt_rx }
}
