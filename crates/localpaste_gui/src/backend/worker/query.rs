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
/// Short-lived cache for list/search metadata queries in the backend worker.
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
    /// Clears cached list/search entries and increments invalidation metrics.
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

fn try_cached_search_items(
    state: &mut WorkerState,
    key: &SearchCacheKey,
    op: &str,
    started: Instant,
) -> Option<Vec<PasteSummary>> {
    if state.query_cache.search_key.as_ref() != Some(key) {
        return None;
    }
    let (Some(items), Some(cached_at)) = (
        state.query_cache.search_items.clone(),
        state.query_cache.search_cached_at,
    ) else {
        return None;
    };
    if cached_at.elapsed() > QUERY_CACHE_MAX_AGE {
        return None;
    }
    state.query_cache.search_hits = state.query_cache.search_hits.saturating_add(1);
    log_query_perf(
        state.perf_log_enabled,
        &state.query_cache,
        op,
        true,
        started.elapsed().as_secs_f64() * 1000.0,
        items.len(),
    );
    Some(items)
}

fn store_search_items_in_cache(
    state: &mut WorkerState,
    key: SearchCacheKey,
    op: &str,
    started: Instant,
    items: Vec<PasteSummary>,
) -> Vec<PasteSummary> {
    state.query_cache.search_key = Some(key);
    state.query_cache.search_items = Some(items.clone());
    state.query_cache.search_cached_at = Some(Instant::now());
    log_query_perf(
        state.perf_log_enabled,
        &state.query_cache,
        op,
        false,
        started.elapsed().as_secs_f64() * 1000.0,
        items.len(),
    );
    items
}

fn run_cached_search<F, E>(
    state: &mut WorkerState,
    key: SearchCacheKey,
    op: &str,
    error_prefix: &str,
    fetch_items: F,
    to_event: E,
) where
    F: FnOnce(&WorkerState) -> Result<Vec<PasteSummary>, String>,
    E: Fn(Vec<PasteSummary>) -> CoreEvent,
{
    let started = Instant::now();
    if let Some(items) = try_cached_search_items(state, &key, op, started) {
        let _ = state.evt_tx.send(to_event(items));
        return;
    }

    state.query_cache.search_misses = state.query_cache.search_misses.saturating_add(1);
    match fetch_items(state) {
        Ok(items) => {
            let items = store_search_items_in_cache(state, key, op, started, items);
            let _ = state.evt_tx.send(to_event(items));
        }
        Err(err) => {
            error!("backend {} failed: {}", op, err);
            send_error(
                &state.evt_tx,
                CoreErrorSource::Other,
                format!("{} failed: {}", error_prefix, err),
            );
        }
    }
}

struct SearchVariant {
    folder_id: Option<String>,
    language: Option<String>,
    op: &'static str,
    error_prefix: &'static str,
}

fn handle_search_variant<E>(
    state: &mut WorkerState,
    query: String,
    limit: usize,
    variant: SearchVariant,
    to_event: E,
) where
    E: Fn(String, Option<String>, Option<String>, Vec<PasteSummary>) -> CoreEvent,
{
    let SearchVariant {
        folder_id,
        language,
        op,
        error_prefix,
    } = variant;
    let key = SearchCacheKey {
        query: query.clone(),
        limit,
        folder_id: folder_id.clone(),
        language: language.clone(),
    };
    let query_for_fetch = query.clone();
    let folder_for_fetch = folder_id.clone();
    let language_for_fetch = language.clone();
    run_cached_search(
        state,
        key,
        op,
        error_prefix,
        move |worker| {
            worker
                .db
                .pastes
                .search_meta(
                    &query_for_fetch,
                    limit,
                    folder_for_fetch,
                    language_for_fetch,
                )
                .map(|metas| metas.iter().map(PasteSummary::from_meta).collect())
                .map_err(|err| err.to_string())
        },
        move |items| to_event(query.clone(), folder_id.clone(), language.clone(), items),
    );
}

/// Logical search pathways supported by backend query handlers.
pub(super) enum SearchRoute {
    Standard {
        folder_id: Option<String>,
        language: Option<String>,
    },
    Palette,
}

/// Loads paste metadata list results, using cache when the key is still fresh.
///
/// # Arguments
/// - `state`: Worker state containing db/cache/event handles.
/// - `limit`: Maximum number of rows to return.
/// - `folder_id`: Optional folder filter.
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

/// Runs a metadata search and emits standard or palette search result events.
///
/// # Arguments
/// - `state`: Worker state containing db/cache/event handles.
/// - `route`: Search route selecting standard or command-palette behavior.
/// - `query`: Raw search text.
/// - `limit`: Maximum number of rows to return.
pub(super) fn handle_search(
    state: &mut WorkerState,
    route: SearchRoute,
    query: String,
    limit: usize,
) {
    match route {
        SearchRoute::Standard {
            folder_id,
            language,
        } => handle_search_variant(
            state,
            query,
            limit,
            SearchVariant {
                folder_id,
                language,
                op: "search",
                error_prefix: "Search",
            },
            |query, folder_id, language, items| CoreEvent::SearchResults {
                query,
                folder_id,
                language,
                items,
            },
        ),
        SearchRoute::Palette => handle_search_variant(
            state,
            query,
            limit,
            SearchVariant {
                folder_id: None,
                language: None,
                op: "palette_search",
                error_prefix: "Palette search",
            },
            |query, _folder_id, _language, items| CoreEvent::PaletteSearchResults { query, items },
        ),
    }
}
