//! Background syntect worker lifecycle and tests.

use super::{
    align_old_lines_by_hash, hash_bytes, line_hash_matches, line_start_state_matches,
    resolve_syntax, HighlightPatch, HighlightRender, HighlightRenderLine, HighlightRequest,
    HighlightSpan, HighlightStateSnapshot, HighlightStyle, HighlightWorkerReply,
    HighlightWorkerResult, SyntectSettings,
};
use crossbeam_channel::{Receiver, Sender};
use eframe::egui;
use localpaste_core::config::env_flag_enabled;
use std::fmt::Write as _;
use std::ops::Range;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::Instant;
use syntect::highlighting::{HighlightState, Highlighter};
use syntect::parsing::{ParseState, ScopeStack};
use tracing::info;

/// Background worker handles syntect highlighting off the UI thread.
pub(crate) struct HighlightWorker {
    pub(crate) tx: Sender<HighlightRequest>,
    pub(crate) rx: Receiver<HighlightWorkerReply>,
    /// Repaint target the worker thread wakes after each reply. It is set once
    /// the window exists, so workers without a window never wake anything.
    repaint: Arc<OnceLock<egui::Context>>,
}

impl HighlightWorker {
    /// Wraps request and reply channels in a worker handle with no repaint target.
    ///
    /// # Arguments
    /// - `tx`: Sender for highlight requests.
    /// - `rx`: Receiver for highlight replies.
    ///
    /// # Returns
    /// A worker handle whose replies wake nothing until a repaint context is attached.
    pub(crate) fn from_channels(
        tx: Sender<HighlightRequest>,
        rx: Receiver<HighlightWorkerReply>,
    ) -> Self {
        Self {
            tx,
            rx,
            repaint: Arc::new(OnceLock::new()),
        }
    }

    /// Lets the worker thread repaint `ctx` whenever it sends a reply.
    ///
    /// eframe repaints only on input or an explicit request, so without this
    /// wake a finished highlight pass would sit unread until the next input
    /// event. Only the first context is kept.
    ///
    /// # Arguments
    /// - `ctx`: Context of the window that displays the highlights.
    pub(crate) fn attach_repaint_context(&self, ctx: &egui::Context) {
        let _ = self.repaint.set(ctx.clone());
    }
}

#[derive(Default)]
struct HighlightWorkerCache {
    language_hint: String,
    theme_key: String,
    lines: Vec<HighlightWorkerLine>,
    last_revision: Option<u64>,
    last_text_len: Option<usize>,
}

#[derive(Clone)]
struct HighlightWorkerLine {
    hash: u64,
    len: usize,
    spans: Vec<HighlightSpan>,
    /// State after this line, shared with the previous line when unchanged.
    end_state: Rc<HighlightStateSnapshot>,
    /// Bound on the parser-state bytes `end_state` owns, from [`parse_state_bytes`].
    state_bytes: usize,
}

/// Returns a bound on the bytes `state` owns, or `None` once it exceeds `limit`.
///
/// syntect gives a context that uses external backreferences a copy of the
/// whole line that pushed it, so a parse state can own any number of source
/// lines: one for each open Markdown list item or fence, heredoc, and so on.
/// syntect exposes no size for a state, but its derived `Debug` writes every
/// stack frame and the full text of each captured line, so the length of that
/// output bounds what the state owns. Counting stops at `limit`, which keeps
/// the cost of measuring within the budget being enforced.
fn parse_state_bytes(state: &ParseState, limit: usize) -> Option<usize> {
    struct Counter {
        bytes: usize,
        limit: usize,
    }
    impl std::fmt::Write for Counter {
        fn write_str(&mut self, text: &str) -> std::fmt::Result {
            self.bytes += text.len();
            if self.bytes > self.limit {
                Err(std::fmt::Error)
            } else {
                Ok(())
            }
        }
    }
    let mut counter = Counter { bytes: 0, limit };
    write!(counter, "{state:?}").ok().map(|()| counter.bytes)
}

/// Parser-state bytes a pass over `text_len` bytes may retain in the cache.
///
/// Ordinary text stays far below this: deeply nested JSON, the heaviest case
/// measured, retains about 80 bytes of state per byte of text. A long opening
/// line copied into the state of every line after it can reach thousands.
fn retained_state_budget(text_len: usize) -> usize {
    text_len
        .saturating_mul(super::super::HIGHLIGHT_STATE_BYTES_PER_TEXT_BYTE)
        .max(super::super::HIGHLIGHT_STATE_BUDGET_FLOOR)
}

/// Lines produced by one worker pass, cached only while their states fit the budget.
///
/// Every cached line keeps the parser state it ended in, and that state can
/// own copies of earlier source lines (see [`parse_state_bytes`]), so a long
/// opening line can be retained once per following line. Consecutive lines
/// with equal end states share one, though a state holding a captured line
/// never equals its own copy, because onig compares regions by address, so
/// those states rely on the budget alone. Distinct states are counted against
/// `budget` before they are cloned; once a pass would exceed it, the pass
/// keeps only its render lines and the caller returns them uncached.
/// Highlighter states own no source text, so only parser states are measured.
struct PassLines {
    budget: usize,
    state_bytes: usize,
    cached: Vec<HighlightWorkerLine>,
    /// Every line so far, once the budget has been exceeded.
    uncached: Option<Vec<HighlightRenderLine>>,
}

impl PassLines {
    /// Starts a pass that may retain `budget` bytes of parser state across `capacity` lines.
    fn new(budget: usize, capacity: usize) -> Self {
        Self {
            budget,
            state_bytes: 0,
            cached: Vec::with_capacity(capacity),
            uncached: None,
        }
    }

    /// Appends a line reused from the previous pass.
    fn push_cached(&mut self, line: HighlightWorkerLine) {
        if let Some(lines) = self.uncached.as_mut() {
            lines.push(worker_line_to_render_line(&line));
            return;
        }
        let shared = self
            .cached
            .last()
            .is_some_and(|prev| Rc::ptr_eq(&prev.end_state, &line.end_state));
        if !shared {
            self.state_bytes = self.state_bytes.saturating_add(line.state_bytes);
        }
        self.cached.push(line);
        if self.state_bytes > self.budget {
            self.stop_caching();
        }
    }

    /// Appends a freshly highlighted line that ended in the given states.
    fn push_parsed(
        &mut self,
        hash: u64,
        len: usize,
        spans: Vec<HighlightSpan>,
        parse: &ParseState,
        highlight: &HighlightState,
    ) {
        if let Some(lines) = self.uncached.as_mut() {
            lines.push(HighlightRenderLine { len, spans });
            return;
        }
        let (end_state, state_bytes) = match self.cached.last() {
            Some(prev)
                if prev.end_state.parse == *parse && prev.end_state.highlight == *highlight =>
            {
                (Rc::clone(&prev.end_state), prev.state_bytes)
            }
            _ => {
                let remaining = self.budget.saturating_sub(self.state_bytes);
                let Some(bytes) = parse_state_bytes(parse, remaining) else {
                    self.stop_caching();
                    self.push_parsed(hash, len, spans, parse, highlight);
                    return;
                };
                self.state_bytes += bytes;
                let snapshot = HighlightStateSnapshot {
                    parse: parse.clone(),
                    highlight: highlight.clone(),
                };
                (Rc::new(snapshot), bytes)
            }
        };
        self.cached.push(HighlightWorkerLine {
            hash,
            len,
            spans,
            end_state,
            state_bytes,
        });
    }

    /// Drops every retained state, keeping the lines so far as render lines.
    fn stop_caching(&mut self) {
        self.uncached = Some(worker_lines_to_render_lines(&self.cached));
        self.cached = Vec::new();
        self.state_bytes = 0;
    }
}

/// Spawns the syntect worker thread and returns its channel endpoints.
///
/// # Returns
/// Worker handle containing request/response channels.
///
/// # Panics
/// Panics if the highlight thread cannot be spawned.
pub(crate) fn spawn_highlight_worker() -> HighlightWorker {
    let (tx, rx_cmd) = crossbeam_channel::unbounded();
    let (tx_evt, rx_evt) = crossbeam_channel::unbounded();
    let worker = HighlightWorker::from_channels(tx, rx_evt);
    let repaint = Arc::clone(&worker.repaint);
    let trace_enabled = env_flag_enabled("LOCALPASTE_HIGHLIGHT_TRACE");

    thread::Builder::new()
        .name("localpaste-gui-highlight".to_string())
        .spawn(move || {
            let mut settings = None;
            let mut cache = HighlightWorkerCache::default();
            for req in rx_cmd.iter() {
                // A worker whose caller never requests highlighting needs no grammar.
                let settings = settings.get_or_insert_with(SyntectSettings::default);
                let mut latest: HighlightRequest = req;
                // Coalesce backlog bursts so stale highlight work is skipped.
                while let Ok(next) = rx_cmd.try_recv() {
                    latest = next;
                }
                let started = Instant::now();
                let trace_paste_id = latest.paste_id.clone();
                let trace_revision = latest.revision;
                let trace_len = latest.text.len_bytes();
                let buffer_epoch = latest.buffer_epoch;
                let render = highlight_in_worker(settings, &mut cache, latest);
                let sent = tx_evt.send(HighlightWorkerReply {
                    buffer_epoch,
                    result: render,
                });
                // Wake the UI only after the reply is queued so the next frame can drain it.
                if sent.is_ok() {
                    if let Some(ctx) = repaint.get() {
                        ctx.request_repaint();
                    }
                }
                if trace_enabled {
                    let elapsed_ms = started.elapsed().as_secs_f32() * 1000.0;
                    info!(
                        target: "localpaste_gui::highlight",
                        event = "worker_done",
                        paste_id = trace_paste_id.as_str(),
                        revision = trace_revision,
                        text_len = trace_len,
                        elapsed_ms = elapsed_ms,
                        "highlight worker pass"
                    );
                }
            }
        })
        .expect("spawn highlight worker");

    worker
}

fn worker_line_to_render_line(line: &HighlightWorkerLine) -> HighlightRenderLine {
    HighlightRenderLine {
        len: line.len,
        spans: line.spans.clone(),
    }
}

fn worker_lines_to_render_lines(lines: &[HighlightWorkerLine]) -> Vec<HighlightRenderLine> {
    lines.iter().map(worker_line_to_render_line).collect()
}

fn highlight_line_spans(
    settings: &SyntectSettings,
    highlighter: &Highlighter<'_>,
    parse_state: &mut ParseState,
    highlight_state: &mut HighlightState,
    line: &str,
) -> Vec<HighlightSpan> {
    // Syntect grammars expect LF terminators. Normalize only the parser's view;
    // render spans still use the original Rope line bytes, including Unicode.
    let normalized = line
        .chars()
        .next_back()
        .filter(|last| {
            matches!(
                last,
                '\r' | '\u{b}' | '\u{c}' | '\u{85}' | '\u{2028}' | '\u{2029}'
            )
        })
        .map(|last| format!("{}\n", &line[..line.len() - last.len_utf8()]));
    let parse_line = normalized.as_deref().unwrap_or(line);
    let mut spans = Vec::new();
    if let Ok(ops) = parse_state.parse_line(parse_line, &settings.ps) {
        let iter = syntect::highlighting::RangedHighlightIterator::new(
            highlight_state,
            &ops[..],
            parse_line,
            highlighter,
        );
        for (style, _token, range) in iter {
            if range.is_empty() {
                continue;
            }
            spans.push(HighlightSpan {
                range: range.start..if range.end == parse_line.len() {
                    line.len()
                } else {
                    range.end
                },
                style: HighlightStyle {
                    color: [
                        style.foreground.r,
                        style.foreground.g,
                        style.foreground.b,
                        style.foreground.a,
                    ],
                    italics: style
                        .font_style
                        .contains(syntect::highlighting::FontStyle::ITALIC),
                    underline: style
                        .font_style
                        .contains(syntect::highlighting::FontStyle::UNDERLINE),
                },
            });
        }
    }
    spans
}

fn changed_line_range_for_render(
    changed_start: Option<usize>,
    changed_end: usize,
    total_lines: usize,
    edit_hint_start: Option<usize>,
) -> Option<Range<usize>> {
    let start = changed_start?;
    let hinted_start = edit_hint_start
        .map(|hint| hint.min(total_lines.saturating_sub(1)))
        .unwrap_or(start);
    let start = start.min(hinted_start);
    let end_exclusive = changed_end.saturating_add(1);
    if end_exclusive > start && end_exclusive <= total_lines {
        Some(start..end_exclusive)
    } else {
        None
    }
}

fn highlight_in_worker(
    settings: &SyntectSettings,
    cache: &mut HighlightWorkerCache,
    req: HighlightRequest,
) -> HighlightWorkerResult {
    let super::HighlightRequestText::Rope(rope) = &req.text;
    let state_budget = retained_state_budget(rope.len_bytes());
    highlight_within_budget(settings, cache, req, state_budget)
}

/// Runs one worker pass whose cache may retain at most `state_budget` bytes of parser state.
fn highlight_within_budget(
    settings: &SyntectSettings,
    cache: &mut HighlightWorkerCache,
    req: HighlightRequest,
    state_budget: usize,
) -> HighlightWorkerResult {
    let HighlightRequest {
        buffer_epoch: _,
        paste_id,
        revision,
        text,
        language_hint,
        theme_key,
        edit_hint,
        patch_base_revision,
        patch_base_text_len,
    } = req;
    let super::HighlightRequestText::Rope(rope) = text;
    // Render rows and edit hints must use the same physical lines as the editor,
    // including bare CR and Ropey's additional Unicode line separators.
    // A CRLF edit can move the boundary behind the edit byte. Recheck that
    // boundary's preceding row too, without scanning the unchanged prefix.
    let edit_start_line = edit_hint
        .map(|hint| rope.byte_to_line(hint.start_byte.saturating_sub(1).min(rope.len_bytes())));
    let text = rope.to_string();
    let text_len = text.len();
    let mut start = 0;
    let lines: Vec<&str> = rope
        .lines()
        .map(|line| {
            let end = start + line.len_bytes();
            let line = &text[start..end];
            start = end;
            line
        })
        .collect();

    if cache.language_hint != language_hint || cache.theme_key != theme_key {
        cache.language_hint = language_hint.clone();
        cache.theme_key = theme_key.clone();
        cache.lines.clear();
        cache.last_revision = None;
        cache.last_text_len = None;
    }
    let cache_base_revision = cache.last_revision;
    let cache_base_text_len = cache.last_text_len;

    let syntax = resolve_syntax(&settings.ps, language_hint.as_str());
    let theme = settings
        .ts
        .themes
        .get(theme_key.as_str())
        .or_else(|| settings.ts.themes.values().next());
    let Some(theme) = theme else {
        let lines = lines
            .iter()
            .map(|line| HighlightRenderLine {
                len: line.len(),
                spans: Vec::new(),
            })
            .collect();
        cache.lines.clear();
        cache.last_revision = Some(revision);
        cache.last_text_len = Some(text_len);
        return HighlightWorkerResult::Render(HighlightRender {
            paste_id,
            revision,
            text_len,
            base_revision: cache_base_revision,
            base_text_len: cache_base_text_len,
            language_hint,
            theme_key,
            changed_line_range: None,
            lines,
        });
    };

    let had_cached_lines = !cache.lines.is_empty();
    let cached_line_count = cache.lines.len();
    let old_cached_lines = std::mem::take(&mut cache.lines);

    let highlighter = Highlighter::new(theme);
    let mut parse_state = ParseState::new(syntax);
    let mut highlight_state = HighlightState::new(&highlighter, ScopeStack::new());
    let default_state = HighlightStateSnapshot {
        parse: parse_state.clone(),
        highlight: highlight_state.clone(),
    };

    let mut new_lines = PassLines::new(state_budget, lines.len().max(1));
    let mut changed_start: Option<usize> = None;
    let mut changed_end: usize = 0;
    let same_len_single_step = had_cached_lines
        && old_cached_lines.len() == lines.len()
        && patch_base_revision == cache_base_revision
        && patch_base_text_len == cache_base_text_len
        && cache_base_revision
            .map(|base| revision == base.wrapping_add(1))
            .unwrap_or(false)
        && edit_hint.is_some();
    if same_len_single_step {
        let mut old_lines: Vec<Option<(usize, HighlightWorkerLine)>> =
            old_cached_lines.into_iter().enumerate().map(Some).collect();
        let start_line = edit_start_line.unwrap_or(0).min(lines.len());
        // The boundary check can start one row before the edit's Rope delta.
        let edit_end_line = edit_hint
            .map(|hint| {
                start_line
                    .saturating_add(hint.touched_lines)
                    .saturating_add(1)
            })
            .unwrap_or(lines.len());

        for old_line_slot in old_lines.iter_mut().take(start_line) {
            let (_, old_line) = old_line_slot
                .take()
                .expect("single-step path requires same-length cached line");
            parse_state = old_line.end_state.parse.clone();
            highlight_state = old_line.end_state.highlight.clone();
            new_lines.push_cached(old_line);
        }

        let mut idx = start_line;
        while idx < lines.len() {
            let line = lines[idx];
            let line_hash = hash_bytes(line.as_bytes());
            let can_reuse =
                line_start_state_matches(
                    idx,
                    None,
                    &old_lines,
                    &parse_state,
                    &highlight_state,
                    (&default_state.parse, &default_state.highlight),
                    |line: &HighlightWorkerLine| (&line.end_state.parse, &line.end_state.highlight),
                ) && line_hash_matches(&old_lines, idx, line_hash, |line: &HighlightWorkerLine| {
                    line.hash
                });
            if can_reuse {
                // Matching an interior line (for example a blank line during
                // block indentation) does not prove the rest of the edit is unchanged.
                if changed_start.is_some() && idx >= edit_end_line {
                    for old_line_slot in old_lines.iter_mut().take(lines.len()).skip(idx) {
                        let (_, old_line) = old_line_slot
                            .take()
                            .expect("single-step path requires same-length cached tail");
                        new_lines.push_cached(old_line);
                    }
                    break;
                }
                let (_, old_line) = old_lines[idx]
                    .take()
                    .expect("single-step path requires same-length cached line");
                parse_state = old_line.end_state.parse.clone();
                highlight_state = old_line.end_state.highlight.clone();
                new_lines.push_cached(old_line);
                idx = idx.saturating_add(1);
                continue;
            }

            if changed_start.is_none() {
                changed_start = Some(idx);
            }
            changed_end = idx;
            let spans = highlight_line_spans(
                settings,
                &highlighter,
                &mut parse_state,
                &mut highlight_state,
                line,
            );
            new_lines.push_parsed(line_hash, line.len(), spans, &parse_state, &highlight_state);
            idx = idx.saturating_add(1);
        }
    } else {
        let new_hashes: Vec<u64> = lines
            .iter()
            .map(|line| hash_bytes(line.as_bytes()))
            .collect();
        let mut old_lines =
            align_old_lines_by_hash(old_cached_lines, &new_hashes, |line| line.hash);
        let mut prev_reused_index = None;

        for (idx, line) in lines.iter().enumerate() {
            let line_hash = new_hashes[idx];
            if line_start_state_matches(
                idx,
                prev_reused_index,
                &old_lines,
                &parse_state,
                &highlight_state,
                (&default_state.parse, &default_state.highlight),
                |line: &HighlightWorkerLine| (&line.end_state.parse, &line.end_state.highlight),
            ) && line_hash_matches(&old_lines, idx, line_hash, |line: &HighlightWorkerLine| {
                line.hash
            }) {
                let (old_idx, old_line) = old_lines[idx].take().expect("checked Some");
                parse_state = old_line.end_state.parse.clone();
                highlight_state = old_line.end_state.highlight.clone();
                new_lines.push_cached(old_line);
                prev_reused_index = Some(old_idx);
                continue;
            }
            if changed_start.is_none() {
                changed_start = Some(idx);
            }
            changed_end = idx;
            let spans = highlight_line_spans(
                settings,
                &highlighter,
                &mut parse_state,
                &mut highlight_state,
                line,
            );
            new_lines.push_parsed(line_hash, line.len(), spans, &parse_state, &highlight_state);
            prev_reused_index = None;
        }
    }

    if let Some(lines) = new_lines.uncached {
        // Nothing retained can seed or patch a later pass, so the next one starts cold.
        cache.lines.clear();
        cache.last_revision = None;
        cache.last_text_len = None;
        return HighlightWorkerResult::Render(HighlightRender {
            paste_id,
            revision,
            text_len,
            base_revision: cache_base_revision,
            base_text_len: cache_base_text_len,
            language_hint,
            theme_key,
            changed_line_range: None,
            lines,
        });
    }
    cache.lines = new_lines.cached;
    cache.last_revision = Some(revision);
    cache.last_text_len = Some(text_len);
    let total_lines = cache.lines.len();
    let changed_line_range =
        changed_line_range_for_render(changed_start, changed_end, total_lines, edit_start_line);

    if had_cached_lines
        && cached_line_count == total_lines
        && patch_base_revision == cache_base_revision
        && patch_base_text_len == cache_base_text_len
    {
        if let Some(changed) = changed_line_range.clone() {
            if changed.len() < total_lines {
                return HighlightWorkerResult::Patch(HighlightPatch {
                    paste_id: paste_id.clone(),
                    revision,
                    text_len,
                    base_revision: cache_base_revision
                        .expect("cached worker base revision required for patch"),
                    base_text_len: cache_base_text_len
                        .expect("cached worker base text length required for patch"),
                    language_hint: language_hint.clone(),
                    theme_key: theme_key.clone(),
                    total_lines,
                    line_range: changed.clone(),
                    lines: cache.lines[changed]
                        .iter()
                        .map(worker_line_to_render_line)
                        .collect(),
                });
            }
        }
    }
    HighlightWorkerResult::Render(HighlightRender {
        paste_id,
        revision,
        text_len,
        base_revision: cache_base_revision,
        base_text_len: cache_base_text_len,
        language_hint,
        theme_key,
        changed_line_range,
        lines: worker_lines_to_render_lines(&cache.lines),
    })
}

#[cfg(test)]
mod resolver_tests;
