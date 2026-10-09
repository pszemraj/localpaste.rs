//! Metadata list and full-content search command handlers for the GUI backend worker.

use super::{send_error, WorkerState};
use crate::backend::{CoreErrorSource, CoreEvent, PasteSummary, SidebarCollection};
use localpaste_core::models::paste::{PasteMeta, ScopedSearchFilter, SearchOptions, SearchScope};
use std::{
    ops::Range,
    time::{Duration, Instant},
};
use tracing::{error, info};

#[derive(Debug, Clone, PartialEq, Eq)]
struct ListCacheKey {
    limit: usize,
    folder_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SearchCacheKey {
    collection: SidebarCollection,
    query: String,
    limit: usize,
    folder_id: Option<String>,
    language: Option<String>,
    case_sensitive: bool,
    scope: SearchScope,
    include_match_excerpt: bool,
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

fn run_cached_search<F, E, X>(
    state: &mut WorkerState,
    key: SearchCacheKey,
    op: &str,
    error_prefix: &str,
    fetch_items: F,
    to_event: E,
    to_error: X,
) where
    F: FnOnce(&WorkerState) -> Result<Vec<PasteSummary>, String>,
    E: Fn(Vec<PasteSummary>) -> CoreEvent,
    X: Fn(String) -> CoreEvent,
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
            let _ = state
                .evt_tx
                .send(to_error(format!("{} failed: {}", error_prefix, err)));
        }
    }
}

struct SearchVariant {
    collection: SidebarCollection,
    folder_id: Option<String>,
    language: Option<String>,
    op: &'static str,
    error_prefix: &'static str,
    include_body_match_excerpt: bool,
}

fn handle_search_variant<E, X>(
    state: &mut WorkerState,
    query: String,
    limit: usize,
    variant: SearchVariant,
    scope: SearchScope,
    to_event: E,
    to_error: X,
) where
    E: Fn(String, Option<String>, Option<String>, Vec<PasteSummary>) -> CoreEvent,
    X: Fn(String) -> CoreEvent,
{
    let SearchVariant {
        collection,
        folder_id,
        language,
        op,
        error_prefix,
        include_body_match_excerpt,
    } = variant;
    let include_match_excerpt =
        include_body_match_excerpt && matches!(scope, SearchScope::All | SearchScope::Body);
    let key = SearchCacheKey {
        collection: collection.clone(),
        query: query.clone(),
        limit,
        folder_id: folder_id.clone(),
        language: language.clone(),
        case_sensitive: state.search_case_sensitive,
        scope,
        include_match_excerpt,
    };
    let query_for_fetch = query.clone();
    let query_for_excerpt = query.clone();
    let folder_for_fetch = folder_id.clone();
    let language_for_fetch = language.clone();
    let options = SearchOptions {
        case_sensitive: state.search_case_sensitive,
    };
    let (today, week, recent) = crate::backend::collections::current_filter_cutoffs();
    run_cached_search(
        state,
        key,
        op,
        error_prefix,
        move |worker| {
            let collection_filter = |meta: &PasteMeta| {
                collection == SidebarCollection::All
                    || crate::backend::collections::matches_active_filters(
                        &PasteSummary::from_meta(meta),
                        &collection,
                        None,
                        today,
                        week,
                        recent,
                    )
            };
            worker
                .db
                .pastes
                .search_scoped_filtered_with_options(
                    &query_for_fetch,
                    limit,
                    folder_for_fetch,
                    language_for_fetch,
                    options,
                    ScopedSearchFilter {
                        scope,
                        predicate: &collection_filter,
                    },
                )
                .and_then(|metas| {
                    metas
                        .into_iter()
                        .map(|meta| {
                            let mut item = PasteSummary::from_meta(&meta);
                            if include_match_excerpt {
                                item.match_excerpt =
                                    worker.db.pastes.get(meta.id.as_str())?.and_then(|paste| {
                                        body_match_excerpt(
                                            paste.content.as_str(),
                                            query_for_excerpt.as_str(),
                                            options.case_sensitive,
                                        )
                                    });
                            }
                            Ok(item)
                        })
                        .collect()
                })
                .map_err(|err| err.to_string())
        },
        move |items| to_event(query.clone(), folder_id.clone(), language.clone(), items),
        to_error,
    );
}

/// Logical search pathways supported by backend query handlers.
pub(super) enum SearchRoute {
    Standard {
        collection: SidebarCollection,
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
    scope: SearchScope,
) {
    match route {
        SearchRoute::Standard {
            collection,
            folder_id,
            language,
        } => {
            let error_query = query.clone();
            let error_collection = collection.clone();
            let error_folder_id = folder_id.clone();
            let error_language = language.clone();
            handle_search_variant(
                state,
                query,
                limit,
                SearchVariant {
                    collection: collection.clone(),
                    folder_id,
                    language,
                    op: "search",
                    error_prefix: "Search",
                    include_body_match_excerpt: false,
                },
                scope,
                move |query, folder_id, language, items| CoreEvent::SearchResults {
                    collection: collection.clone(),
                    scope,
                    query,
                    folder_id,
                    language,
                    items,
                },
                move |message| CoreEvent::SearchFailed {
                    collection: error_collection.clone(),
                    scope,
                    query: error_query.clone(),
                    folder_id: error_folder_id.clone(),
                    language: error_language.clone(),
                    message,
                },
            )
        }
        SearchRoute::Palette => {
            let error_query = query.clone();
            handle_search_variant(
                state,
                query,
                limit,
                SearchVariant {
                    collection: SidebarCollection::All,
                    folder_id: None,
                    language: None,
                    op: "palette_search",
                    error_prefix: "Palette search",
                    include_body_match_excerpt: true,
                },
                scope,
                move |query, _folder_id, _language, items| CoreEvent::PaletteSearchResults {
                    query,
                    items,
                    scope,
                },
                move |message| CoreEvent::PaletteSearchFailed {
                    query: error_query.clone(),
                    scope,
                    message,
                },
            )
        }
    }
}

const MATCH_EXCERPT_MAX_CHARS: usize = 160;
const MATCH_EXCERPT_CONTEXT_CHARS: usize = 48;
const MATCH_EXCERPT_MATCH_CHARS: usize =
    MATCH_EXCERPT_MAX_CHARS - 2 * MATCH_EXCERPT_CONTEXT_CHARS - 2;

/// Builds a compact, original-text excerpt around the first raw-body match.
///
/// The case policy mirrors canonical scoped search. The returned text is capped
/// by character count so slicing never splits a Unicode scalar value.
fn body_match_excerpt(content: &str, query: &str, case_sensitive: bool) -> Option<String> {
    let range = find_search_range(content, query.trim(), case_sensitive)?;
    let before = &content[..range.start];
    let matched = &content[range.clone()];
    let after = &content[range.end..];
    let prefix = take_last_chars(before, MATCH_EXCERPT_CONTEXT_CHARS);
    let match_text = take_first_chars(matched, MATCH_EXCERPT_MATCH_CHARS);
    let suffix = if match_text.len() == matched.len() {
        take_first_chars(after, MATCH_EXCERPT_CONTEXT_CHARS)
    } else {
        String::new()
    };
    let prefix_clipped = before
        .chars()
        .rev()
        .nth(MATCH_EXCERPT_CONTEXT_CHARS)
        .is_some();
    let match_clipped = match_text.len() < matched.len();
    let suffix_clipped = match_clipped || after.chars().nth(MATCH_EXCERPT_CONTEXT_CHARS).is_some();

    let mut excerpt = String::with_capacity(MATCH_EXCERPT_MAX_CHARS);
    if prefix_clipped {
        excerpt.push('…');
    }
    append_compact_text(&mut excerpt, prefix.as_str());
    append_compact_text(&mut excerpt, match_text.as_str());
    append_compact_text(&mut excerpt, suffix.as_str());
    if suffix_clipped {
        excerpt.push('…');
    }
    Some(excerpt)
}

/// Finds the source-text byte range that canonical body search treats as a match.
fn find_search_range(content: &str, query: &str, case_sensitive: bool) -> Option<Range<usize>> {
    if query.is_empty() {
        return None;
    }
    if case_sensitive {
        return content.find(query).map(|start| start..start + query.len());
    }

    let normalized_query = query.to_lowercase();
    if normalized_query.is_ascii() {
        return localpaste_core::text::find_ascii_case_insensitive_range(
            content,
            &normalized_query,
        );
    }

    let normalized_content = content.to_lowercase();
    let normalized_start = normalized_content.find(normalized_query.as_str())?;
    let normalized_end = normalized_start + normalized_query.len();
    Some(source_range_for_normalized_match(
        content,
        normalized_start,
        normalized_end,
    ))
}

/// Maps a lowercased string range back to source-character boundaries.
fn source_range_for_normalized_match(
    content: &str,
    normalized_start: usize,
    normalized_end: usize,
) -> Range<usize> {
    let mut normalized_offset = 0;
    let mut source_start = None;
    for (source_offset, ch) in content.char_indices() {
        let normalized_width = ch.to_lowercase().map(char::len_utf8).sum::<usize>();
        let next_normalized_offset = normalized_offset + normalized_width;
        if source_start.is_none() && normalized_start < next_normalized_offset {
            source_start = Some(source_offset);
        }
        if normalized_end <= next_normalized_offset {
            return source_start.unwrap_or(source_offset)..source_offset + ch.len_utf8();
        }
        normalized_offset = next_normalized_offset;
    }
    content.len()..content.len()
}

/// Returns the final `max_chars` Unicode scalar values from `text`.
fn take_last_chars(text: &str, max_chars: usize) -> String {
    let mut chars: Vec<_> = text.chars().rev().take(max_chars).collect();
    chars.reverse();
    chars.into_iter().collect()
}

/// Returns the initial `max_chars` Unicode scalar values from `text`.
fn take_first_chars(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

/// Appends text as one visual line while preserving its matching content.
fn append_compact_text(output: &mut String, text: &str) {
    let mut previous_was_whitespace = output.chars().last().is_some_and(char::is_whitespace);
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !previous_was_whitespace {
                output.push(' ');
                previous_was_whitespace = true;
            }
        } else {
            output.push(ch);
            previous_was_whitespace = false;
        }
    }
}

#[cfg(test)]
mod excerpt_tests {
    use super::{body_match_excerpt, find_search_range, MATCH_EXCERPT_MAX_CHARS};

    #[test]
    fn body_excerpt_preserves_unicode_case_and_compacts_newlines() {
        let excerpt = body_match_excerpt("before\nÉCOLE after", "école", false)
            .expect("Unicode case-insensitive match");
        assert!(excerpt.contains("ÉCOLE"));
        assert!(!excerpt.contains('\n'));
        assert_eq!(
            find_search_range("before İ after", "i\u{307}", false),
            Some(7..9)
        );
    }

    #[test]
    fn body_excerpt_keeps_ascii_matching_compatible_with_canonical_search() {
        assert!(body_match_excerpt("İstanbul", "i", false).is_none());
        assert!(body_match_excerpt("Needle", " ", false).is_none());
        assert_eq!(
            find_search_range("é NeEdLe needle", "needle", false),
            Some(3..9)
        );
    }

    #[test]
    fn body_excerpt_is_bounded_while_retaining_the_match() {
        let content = format!("{}Needle{}", "a".repeat(400), "b".repeat(400));
        let excerpt =
            body_match_excerpt(content.as_str(), "needle", false).expect("case-insensitive match");
        assert!(excerpt.contains("Needle"));
        assert!(excerpt.chars().count() <= MATCH_EXCERPT_MAX_CHARS);
        assert!(excerpt.starts_with('…'));
        assert!(excerpt.ends_with('…'));
    }
}
