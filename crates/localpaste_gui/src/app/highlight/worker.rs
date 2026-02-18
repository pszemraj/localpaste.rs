//! Background syntect worker lifecycle and tests.

use super::super::util::env_flag_enabled;
use super::{
    align_old_lines_by_hash, hash_bytes, line_hash_matches, line_start_state_matches,
    resolve_syntax, HighlightPatch, HighlightRender, HighlightRenderLine, HighlightRequest,
    HighlightSpan, HighlightStateSnapshot, HighlightStyle, HighlightWorkerResult, SyntectSettings,
};
use crossbeam_channel::{Receiver, Sender};
use std::ops::Range;
use std::thread;
use std::time::Instant;
use syntect::highlighting::{HighlightState, Highlighter};
use syntect::parsing::{ParseState, ScopeStack};
use syntect::util::LinesWithEndings;
use tracing::info;

/// Background worker handles syntect highlighting off the UI thread.
pub(crate) struct HighlightWorker {
    pub(crate) tx: Sender<HighlightRequest>,
    pub(crate) rx: Receiver<HighlightWorkerResult>,
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
    end_state: HighlightStateSnapshot,
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
    let trace_enabled = env_flag_enabled("LOCALPASTE_HIGHLIGHT_TRACE");

    thread::Builder::new()
        .name("localpaste-gui-highlight".to_string())
        .spawn(move || {
            let settings = SyntectSettings::default();
            let mut cache = HighlightWorkerCache::default();
            for req in rx_cmd.iter() {
                let mut latest: HighlightRequest = req;
                // Coalesce backlog bursts so stale highlight work is skipped.
                while let Ok(next) = rx_cmd.try_recv() {
                    latest = next;
                }
                let started = Instant::now();
                let trace_paste_id = latest.paste_id.clone();
                let trace_revision = latest.revision;
                let trace_len = latest.text.len_bytes();
                let render = highlight_in_worker(&settings, &mut cache, latest);
                let _ = tx_evt.send(render);
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

    HighlightWorker { tx, rx: rx_evt }
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
    let mut spans = Vec::new();
    if let Ok(ops) = parse_state.parse_line(line, &settings.ps) {
        let iter = syntect::highlighting::RangedHighlightIterator::new(
            highlight_state,
            &ops[..],
            line,
            highlighter,
        );
        for (style, _token, range) in iter {
            if range.is_empty() {
                continue;
            }
            spans.push(HighlightSpan {
                range,
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
    let HighlightRequest {
        paste_id,
        revision,
        text,
        language_hint,
        theme_key,
        edit_hint,
        patch_base_revision,
        patch_base_text_len,
    } = req;
    let text = text.into_string();
    let text_len = text.len();

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
        let lines = LinesWithEndings::from(text.as_str())
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
    let lines: Vec<&str> = LinesWithEndings::from(text.as_str()).collect();
    let old_cached_lines = std::mem::take(&mut cache.lines);

    let highlighter = Highlighter::new(theme);
    let mut parse_state = ParseState::new(syntax);
    let mut highlight_state = HighlightState::new(&highlighter, ScopeStack::new());
    let default_state = HighlightStateSnapshot {
        parse: parse_state.clone(),
        highlight: highlight_state.clone(),
    };

    let mut new_lines = Vec::with_capacity(lines.len().max(1));
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
        let mut old_lines: Vec<Option<HighlightWorkerLine>> =
            old_cached_lines.into_iter().map(Some).collect();
        let start_line = edit_hint
            .map(|hint| hint.start_line)
            .unwrap_or(0)
            .min(lines.len());

        for old_line_slot in old_lines.iter_mut().take(start_line) {
            let old_line = old_line_slot
                .take()
                .expect("single-step path requires same-length cached line");
            parse_state = old_line.end_state.parse.clone();
            highlight_state = old_line.end_state.highlight.clone();
            new_lines.push(old_line);
        }

        let mut idx = start_line;
        while idx < lines.len() {
            let line = lines[idx];
            let line_hash = hash_bytes(line.as_bytes());
            let can_reuse =
                line_start_state_matches(
                    idx,
                    false,
                    &old_lines,
                    &parse_state,
                    &highlight_state,
                    (&default_state.parse, &default_state.highlight),
                    |line: &HighlightWorkerLine| (&line.end_state.parse, &line.end_state.highlight),
                ) && line_hash_matches(&old_lines, idx, line_hash, |line: &HighlightWorkerLine| {
                    line.hash
                });
            if can_reuse {
                if changed_start.is_some() {
                    for old_line_slot in old_lines.iter_mut().take(lines.len()).skip(idx) {
                        let old_line = old_line_slot
                            .take()
                            .expect("single-step path requires same-length cached tail");
                        new_lines.push(old_line);
                    }
                    break;
                }
                let old_line = old_lines[idx]
                    .take()
                    .expect("single-step path requires same-length cached line");
                parse_state = old_line.end_state.parse.clone();
                highlight_state = old_line.end_state.highlight.clone();
                new_lines.push(old_line);
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
            let end_state = HighlightStateSnapshot {
                parse: parse_state.clone(),
                highlight: highlight_state.clone(),
            };
            new_lines.push(HighlightWorkerLine {
                hash: line_hash,
                len: line.len(),
                spans,
                end_state,
            });
            idx = idx.saturating_add(1);
        }
    } else {
        let new_hashes: Vec<u64> = lines
            .iter()
            .map(|line| hash_bytes(line.as_bytes()))
            .collect();
        let mut old_lines =
            align_old_lines_by_hash(old_cached_lines, &new_hashes, |line| line.hash);
        let mut prev_line_reused = false;

        for (idx, line) in lines.iter().enumerate() {
            let line_hash = new_hashes[idx];
            if line_start_state_matches(
                idx,
                prev_line_reused,
                &old_lines,
                &parse_state,
                &highlight_state,
                (&default_state.parse, &default_state.highlight),
                |line: &HighlightWorkerLine| (&line.end_state.parse, &line.end_state.highlight),
            ) && line_hash_matches(&old_lines, idx, line_hash, |line: &HighlightWorkerLine| {
                line.hash
            }) {
                let old_line = old_lines[idx].take().expect("checked Some");
                parse_state = old_line.end_state.parse.clone();
                highlight_state = old_line.end_state.highlight.clone();
                new_lines.push(old_line);
                prev_line_reused = true;
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
            let end_state = HighlightStateSnapshot {
                parse: parse_state.clone(),
                highlight: highlight_state.clone(),
            };
            new_lines.push(HighlightWorkerLine {
                hash: line_hash,
                len: line.len(),
                spans,
                end_state,
            });
            prev_line_reused = false;
        }
    }

    cache.lines = new_lines;
    cache.last_revision = Some(revision);
    cache.last_text_len = Some(text_len);
    let total_lines = cache.lines.len();
    let changed_line_range = changed_line_range_for_render(
        changed_start,
        changed_end,
        total_lines,
        edit_hint.map(|hint| hint.start_line),
    );

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
mod resolver_tests {
    use super::super::{resolve_syntax, syntect_language_hint, HighlightRequestText};
    use super::{
        highlight_in_worker, HighlightRender, HighlightRequest, HighlightWorkerResult,
        SyntectSettings,
    };
    use crate::app::highlight::worker::HighlightWorkerCache;

    fn render_for_label(settings: &SyntectSettings, label: &str, text: &str) -> HighlightRender {
        let mut cache = HighlightWorkerCache::default();
        let req = HighlightRequest {
            paste_id: "test".to_string(),
            revision: 1,
            text: HighlightRequestText::Owned(text.to_string()),
            language_hint: label.to_string(),
            theme_key: "base16-mocha.dark".to_string(),
            edit_hint: None,
            patch_base_revision: None,
            patch_base_text_len: None,
        };
        match highlight_in_worker(settings, &mut cache, req) {
            HighlightWorkerResult::Render(render) => render,
            HighlightWorkerResult::Patch(_) => panic!("expected full render for cold worker cache"),
        }
    }

    fn has_non_default_coloring(settings: &SyntectSettings, render: &HighlightRender) -> bool {
        let theme = settings
            .ts
            .themes
            .get(render.theme_key.as_str())
            .expect("theme exists");
        let default = theme
            .settings
            .foreground
            .map(|color| [color.r, color.g, color.b, color.a]);
        render
            .lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .any(|span| Some(span.style.color) != default)
    }

    fn rust_request(
        revision: u64,
        text: &str,
        patch_base_revision: Option<u64>,
        patch_base_text_len: Option<usize>,
    ) -> HighlightRequest {
        HighlightRequest {
            paste_id: "test".to_string(),
            revision,
            text: HighlightRequestText::Owned(text.to_string()),
            language_hint: "rust".to_string(),
            theme_key: "base16-mocha.dark".to_string(),
            edit_hint: None,
            patch_base_revision,
            patch_base_text_len,
        }
    }

    fn seed_worker_cache(settings: &SyntectSettings, cache: &mut HighlightWorkerCache) -> usize {
        let text_v1 = "let a = 1;\nlet b = 2;\n";
        let first = highlight_in_worker(settings, cache, rust_request(1, text_v1, None, None));
        assert!(matches!(first, HighlightWorkerResult::Render(_)));
        text_v1.len()
    }

    #[test]
    fn resolve_syntax_handles_common_canonical_labels() {
        let settings = SyntectSettings::default();
        let cases = [
            "rust",
            "python",
            "javascript",
            "go",
            "java",
            "html",
            "css",
            "json",
            "yaml",
            "sql",
        ];
        for label in cases {
            let syntax = resolve_syntax(&settings.ps, label);
            assert_ne!(syntax.name, "Plain Text", "label: {label}");
        }
    }

    #[test]
    fn resolve_syntax_falls_back_to_plain_for_unknown_or_text() {
        let settings = SyntectSettings::default();
        assert_eq!(
            resolve_syntax(&settings.ps, "somethingtotallyunknown").name,
            "Plain Text"
        );
        assert_eq!(resolve_syntax(&settings.ps, "").name, "Plain Text");
        assert_eq!(resolve_syntax(&settings.ps, "text").name, "Plain Text");
        assert_eq!(resolve_syntax(&settings.ps, "txt").name, "Plain Text");
    }

    #[test]
    fn syntect_hint_uses_canonical_labels() {
        assert_eq!(syntect_language_hint(" csharp "), "cs");
        assert_eq!(syntect_language_hint("bash"), "shell");
        assert_eq!(syntect_language_hint(""), "text");
    }

    #[test]
    fn resolve_syntax_alias_and_unsupported_matrix() {
        let settings = SyntectSettings::default();
        let cases = [
            ("cs", false),
            ("shell", false),
            ("cpp", false),
            ("powershell", false),
            ("zig", true),
            ("scss", true),
            ("kotlin", true),
            ("elixir", true),
            ("dart", true),
        ];
        for (label, expect_plain) in cases {
            let syntax = resolve_syntax(&settings.ps, label);
            if expect_plain {
                assert_eq!(syntax.name, "Plain Text", "label: {label}");
            } else {
                assert_ne!(syntax.name, "Plain Text", "label: {label}");
            }
        }
    }

    #[test]
    fn high_priority_fallbacks_produce_non_default_coloring() {
        let settings = SyntectSettings::default();
        let cases = [
            (
                "typescript",
                "type User = { name: string };\nconst user: User = { name: \"ada\" };",
            ),
            (
                "toml",
                "[tool.localpaste]\nname = \"demo\"\nenabled = true\n",
            ),
            (
                "swift",
                "import Foundation\nstruct User { let id: Int }\nfunc main() { print(\"hi\") }\n",
            ),
            (
                "powershell",
                "param([string]$Name)\nWrite-Host \"hello $Name\"\n",
            ),
        ];
        for (label, content) in cases {
            let render = render_for_label(&settings, label, content);
            assert!(
                has_non_default_coloring(&settings, &render),
                "expected non-default coloring for {label}"
            );
        }
    }

    #[test]
    fn worker_emits_patch_only_when_ui_base_matches_worker_cache() {
        let settings = SyntectSettings::default();
        let mut cache = HighlightWorkerCache::default();
        let base_text_len = seed_worker_cache(&settings, &mut cache);

        let text_v2 = "let a = 1;\nlet b = 3;\n";
        let req_v2_patch = rust_request(2, text_v2, Some(1), Some(base_text_len));
        let second = highlight_in_worker(&settings, &mut cache, req_v2_patch);
        assert!(matches!(second, HighlightWorkerResult::Patch(_)));

        let text_v3 = "let a = 4;\nlet b = 3;\n";
        let req_v3_wrong_base = rust_request(3, text_v3, Some(0), Some(123));
        let third = highlight_in_worker(&settings, &mut cache, req_v3_wrong_base);
        assert!(matches!(third, HighlightWorkerResult::Render(_)));
    }

    #[test]
    fn worker_emits_full_render_when_line_count_changes() {
        let settings = SyntectSettings::default();
        let mut cache = HighlightWorkerCache::default();
        let base_text_len = seed_worker_cache(&settings, &mut cache);

        let text_v2 = "let a = 1;\nlet inserted = 9;\nlet b = 2;\n";
        let req_v2_line_insert = rust_request(2, text_v2, Some(1), Some(base_text_len));
        let second = highlight_in_worker(&settings, &mut cache, req_v2_line_insert);
        assert!(matches!(second, HighlightWorkerResult::Render(_)));
    }

    #[test]
    fn worker_accepts_rope_payload_requests() {
        let settings = SyntectSettings::default();
        let mut cache = HighlightWorkerCache::default();
        let req = HighlightRequest {
            paste_id: "test".to_string(),
            revision: 1,
            text: HighlightRequestText::Rope(ropey::Rope::from_str("let a = 1;\n")),
            language_hint: "rust".to_string(),
            theme_key: "base16-mocha.dark".to_string(),
            edit_hint: None,
            patch_base_revision: None,
            patch_base_text_len: None,
        };
        let out = highlight_in_worker(&settings, &mut cache, req);
        assert!(matches!(out, HighlightWorkerResult::Render(_)));
    }
}
