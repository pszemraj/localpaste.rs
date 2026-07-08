//! Metadata list and full-content search command handlers for the GUI backend worker.

use super::{send_error, WorkerState};
use crate::backend::{CoreErrorSource, CoreEvent, PasteSummary};
use localpaste_core::models::paste::SearchOptions;
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
    case_sensitive: bool,
}

#[derive(Debug)]
struct CachedItems<K> {
    key: Option<K>,
    items: Option<Vec<PasteSummary>>,
    cached_at: Option<Instant>,
}

impl<K> Default for CachedItems<K> {
    fn default() -> Self {
        Self {
            key: None,
            items: None,
            cached_at: None,
        }
    }
}

impl<K: PartialEq> CachedItems<K> {
    fn is_populated(&self) -> bool {
        self.key.is_some() || self.items.is_some() || self.cached_at.is_some()
    }

    fn clear(&mut self) {
        self.key = None;
        self.items = None;
        self.cached_at = None;
    }

    fn fresh_items(&self, key: &K) -> Option<Vec<PasteSummary>> {
        if self.key.as_ref() != Some(key) {
            return None;
        }
        let (Some(items), Some(cached_at)) = (self.items.clone(), self.cached_at) else {
            return None;
        };
        if cached_at.elapsed() > QUERY_CACHE_MAX_AGE {
            return None;
        }
        Some(items)
    }

    fn store(&mut self, key: K, items: Vec<PasteSummary>) {
        self.key = Some(key);
        self.items = Some(items);
        self.cached_at = Some(Instant::now());
    }
}

#[derive(Debug, Default)]
/// Short-lived cache for list/search metadata queries in the backend worker.
pub(super) struct QueryCache {
    list: CachedItems<ListCacheKey>,
    search: CachedItems<SearchCacheKey>,
    list_hits: u64,
    list_misses: u64,
    search_hits: u64,
    search_misses: u64,
    invalidations: u64,
}

impl QueryCache {
    /// Clears cached list/search entries and increments invalidation metrics.
    pub(super) fn invalidate(&mut self) {
        if self.list.is_populated() || self.search.is_populated() {
            self.list.clear();
            self.search.clear();
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
    let items = state.query_cache.search.fresh_items(key)?;
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
    state.query_cache.search.store(key, items.clone());
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
        case_sensitive: state.search_case_sensitive,
    };
    let query_for_fetch = query.clone();
    let folder_for_fetch = folder_id.clone();
    let language_for_fetch = language.clone();
    let options = SearchOptions {
        case_sensitive: state.search_case_sensitive,
    };
    run_cached_search(
        state,
        key,
        op,
        error_prefix,
        move |worker| {
            worker
                .db
                .pastes
                .search_with_options(
                    &query_for_fetch,
                    limit,
                    folder_for_fetch,
                    language_for_fetch,
                    options,
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
    if let Some(items) = state.query_cache.list.fresh_items(&key) {
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

    state.query_cache.list_misses = state.query_cache.list_misses.saturating_add(1);
    match state.db.pastes.list_meta(limit, folder_id) {
        Ok(metas) => {
            let items: Vec<PasteSummary> = metas.iter().map(PasteSummary::from_meta).collect();
            state.query_cache.list.store(key, items.clone());
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

/// Runs full-content search and emits standard or palette search result events.
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
