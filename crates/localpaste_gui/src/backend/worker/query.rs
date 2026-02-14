//! Metadata list/search command handlers and short-lived cache for the GUI backend worker.

use super::{send_error, WorkerState};
use crate::backend::{CoreErrorSource, CoreEvent, PasteSummary};
use std::time::{Duration, Instant};
use tracing::{error, info};

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
pub(super) struct QueryCache {
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
    pub(super) fn invalidate(&mut self) {
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

pub(super) fn handle_list_pastes(state: &mut WorkerState, limit: usize, folder_id: Option<String>) {
    let started = Instant::now();
    let key = ListCacheKey {
        limit,
        folder_id: folder_id.clone(),
    };
    if state.query_cache.list_key.as_ref() == Some(&key) {
        if let (Some(items), Some(cached_at)) = (
            state.query_cache.list_items.clone(),
            state.query_cache.list_cached_at,
        ) {
            if cached_at.elapsed() <= QUERY_CACHE_MAX_AGE {
                state.query_cache.list_hits = state.query_cache.list_hits.saturating_add(1);
                log_query_perf(
                    state.perf_log_enabled,
                    &state.query_cache,
                    "list",
                    true,
                    started.elapsed().as_secs_f64() * 1000.0,
                    items.len(),
                );
                let _ = state.evt_tx.send(CoreEvent::PasteList { items });
                return;
            }
        }
    }

    state.query_cache.list_misses = state.query_cache.list_misses.saturating_add(1);
    match state.db.pastes.list_meta(limit, folder_id) {
        Ok(metas) => {
            let items: Vec<PasteSummary> = metas.iter().map(PasteSummary::from_meta).collect();
            state.query_cache.list_key = Some(key);
            state.query_cache.list_items = Some(items.clone());
            state.query_cache.list_cached_at = Some(Instant::now());
            log_query_perf(
                state.perf_log_enabled,
                &state.query_cache,
                "list",
                false,
                started.elapsed().as_secs_f64() * 1000.0,
                items.len(),
            );
            let _ = state.evt_tx.send(CoreEvent::PasteList { items });
        }
        Err(err) => {
            error!("backend list failed: {}", err);
            send_error(
                &state.evt_tx,
                CoreErrorSource::Other,
                format!("List failed: {}", err),
            );
        }
    }
}

pub(super) fn handle_search_pastes(
    state: &mut WorkerState,
    query: String,
    limit: usize,
    folder_id: Option<String>,
    language: Option<String>,
) {
    let started = Instant::now();
    let key = SearchCacheKey {
        query: query.clone(),
        limit,
        folder_id: folder_id.clone(),
        language: language.clone(),
    };
    if state.query_cache.search_key.as_ref() == Some(&key) {
        if let (Some(items), Some(cached_at)) = (
            state.query_cache.search_items.clone(),
            state.query_cache.search_cached_at,
        ) {
            if cached_at.elapsed() <= QUERY_CACHE_MAX_AGE {
                state.query_cache.search_hits = state.query_cache.search_hits.saturating_add(1);
                log_query_perf(
                    state.perf_log_enabled,
                    &state.query_cache,
                    "search",
                    true,
                    started.elapsed().as_secs_f64() * 1000.0,
                    items.len(),
                );
                let _ = state.evt_tx.send(CoreEvent::SearchResults { query, items });
                return;
            }
        }
    }

    state.query_cache.search_misses = state.query_cache.search_misses.saturating_add(1);
    match state
        .db
        .pastes
        .search_meta(&query, limit, folder_id, language)
    {
        Ok(metas) => {
            let items: Vec<PasteSummary> = metas.iter().map(PasteSummary::from_meta).collect();
            state.query_cache.search_key = Some(key);
            state.query_cache.search_items = Some(items.clone());
            state.query_cache.search_cached_at = Some(Instant::now());
            log_query_perf(
                state.perf_log_enabled,
                &state.query_cache,
                "search",
                false,
                started.elapsed().as_secs_f64() * 1000.0,
                items.len(),
            );
            let _ = state.evt_tx.send(CoreEvent::SearchResults { query, items });
        }
        Err(err) => {
            error!("backend search failed: {}", err);
            send_error(
                &state.evt_tx,
                CoreErrorSource::Other,
                format!("Search failed: {}", err),
            );
        }
    }
}

pub(super) fn handle_palette_search(state: &mut WorkerState, query: String, limit: usize) {
    let started = Instant::now();
    let key = SearchCacheKey {
        query: query.clone(),
        limit,
        folder_id: None,
        language: None,
    };
    if state.query_cache.search_key.as_ref() == Some(&key) {
        if let (Some(items), Some(cached_at)) = (
            state.query_cache.search_items.clone(),
            state.query_cache.search_cached_at,
        ) {
            if cached_at.elapsed() <= QUERY_CACHE_MAX_AGE {
                state.query_cache.search_hits = state.query_cache.search_hits.saturating_add(1);
                log_query_perf(
                    state.perf_log_enabled,
                    &state.query_cache,
                    "palette_search",
                    true,
                    started.elapsed().as_secs_f64() * 1000.0,
                    items.len(),
                );
                let _ = state
                    .evt_tx
                    .send(CoreEvent::PaletteSearchResults { query, items });
                return;
            }
        }
    }

    state.query_cache.search_misses = state.query_cache.search_misses.saturating_add(1);
    match state.db.pastes.search_meta(&query, limit, None, None) {
        Ok(metas) => {
            let items: Vec<PasteSummary> = metas.iter().map(PasteSummary::from_meta).collect();
            state.query_cache.search_key = Some(key);
            state.query_cache.search_items = Some(items.clone());
            state.query_cache.search_cached_at = Some(Instant::now());
            log_query_perf(
                state.perf_log_enabled,
                &state.query_cache,
                "palette_search",
                false,
                started.elapsed().as_secs_f64() * 1000.0,
                items.len(),
            );
            let _ = state
                .evt_tx
                .send(CoreEvent::PaletteSearchResults { query, items });
        }
        Err(err) => {
            error!("backend palette search failed: {}", err);
            send_error(
                &state.evt_tx,
                CoreErrorSource::Other,
                format!("Palette search failed: {}", err),
            );
        }
    }
}
