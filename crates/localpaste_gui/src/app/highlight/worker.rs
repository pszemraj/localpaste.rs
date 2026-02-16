//! Background syntect worker lifecycle and tests.

use super::super::util::env_flag_enabled;
use super::{
    align_old_lines_by_hash, hash_bytes, line_hash_matches, line_start_state_matches,
    resolve_syntax, HighlightRender, HighlightRenderLine, HighlightRequest, HighlightSpan,
    HighlightStateSnapshot, HighlightStyle, SyntectSettings,
};
use crossbeam_channel::{Receiver, Sender};
use std::thread;
use std::time::Instant;
use syntect::highlighting::{HighlightState, Highlighter};
use syntect::parsing::{ParseState, ScopeStack};
use syntect::util::LinesWithEndings;
use tracing::info;

/// Background worker handles syntect highlighting off the UI thread.
pub(crate) struct HighlightWorker {
    pub(crate) tx: Sender<HighlightRequest>,
    pub(crate) rx: Receiver<HighlightRender>,
}

#[derive(Default)]
struct HighlightWorkerCache {
    language_hint: String,
    theme_key: String,
    lines: Vec<HighlightWorkerLine>,
}

#[derive(Clone)]
struct HighlightWorkerLine {
    hash: u64,
    len: usize,
    spans: Vec<HighlightSpan>,
    end_state: HighlightStateSnapshot,
}

/// Spawns the syntect worker thread and returns its channel endpoints.
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
                let trace_len = latest.text.len();
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

fn highlight_in_worker(
    settings: &SyntectSettings,
    cache: &mut HighlightWorkerCache,
    req: HighlightRequest,
) -> HighlightRender {
    if cache.language_hint != req.language_hint || cache.theme_key != req.theme_key {
        cache.language_hint = req.language_hint.clone();
        cache.theme_key = req.theme_key.clone();
        cache.lines.clear();
    }

    let syntax = resolve_syntax(&settings.ps, &req.language_hint);
    let theme = settings
        .ts
        .themes
        .get(req.theme_key.as_str())
        .or_else(|| settings.ts.themes.values().next());
    let Some(theme) = theme else {
        let lines = LinesWithEndings::from(req.text.as_str())
            .map(|line| HighlightRenderLine {
                len: line.len(),
                spans: Vec::new(),
            })
            .collect();
        return HighlightRender {
            paste_id: req.paste_id,
            revision: req.revision,
            text_len: req.text.len(),
            content_hash: req.content_hash,
            language_hint: req.language_hint,
            theme_key: req.theme_key,
            lines,
        };
    };

    let lines: Vec<&str> = LinesWithEndings::from(req.text.as_str()).collect();
    let new_hashes: Vec<u64> = lines
        .iter()
        .map(|line| hash_bytes(line.as_bytes()))
        .collect();
    let mut old_lines =
        align_old_lines_by_hash(std::mem::take(&mut cache.lines), &new_hashes, |line| {
            line.hash
        });

    let highlighter = Highlighter::new(theme);
    let mut parse_state = ParseState::new(syntax);
    let mut highlight_state = HighlightState::new(&highlighter, ScopeStack::new());
    let default_state = HighlightStateSnapshot {
        parse: parse_state.clone(),
        highlight: highlight_state.clone(),
    };

    let mut new_lines = Vec::with_capacity(lines.len().max(1));
    let mut render_lines = Vec::with_capacity(lines.len().max(1));
    let mut prev_line_reused = false;

    for (idx, line) in lines.iter().enumerate() {
        let line_hash = new_hashes[idx];
        if line_start_state_matches(
            idx,
            prev_line_reused,
            &old_lines,
            &parse_state,
            &highlight_state,
            &default_state,
            |line: &HighlightWorkerLine| &line.end_state,
        ) && line_hash_matches(&old_lines, idx, line_hash, |line: &HighlightWorkerLine| {
            line.hash
        }) {
            let old_line = old_lines[idx].take().expect("checked Some");
            parse_state = old_line.end_state.parse.clone();
            highlight_state = old_line.end_state.highlight.clone();
            render_lines.push(HighlightRenderLine {
                len: old_line.len,
                spans: old_line.spans.clone(),
            });
            new_lines.push(old_line);
            prev_line_reused = true;
            continue;
        }

        let mut spans = Vec::new();
        if let Ok(ops) = parse_state.parse_line(line, &settings.ps) {
            let iter = syntect::highlighting::RangedHighlightIterator::new(
                &mut highlight_state,
                &ops[..],
                line,
                &highlighter,
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

        let end_state = HighlightStateSnapshot {
            parse: parse_state.clone(),
            highlight: highlight_state.clone(),
        };
        let line_len = line.len();
        new_lines.push(HighlightWorkerLine {
            hash: line_hash,
            len: line_len,
            spans: spans.clone(),
            end_state,
        });
        render_lines.push(HighlightRenderLine {
            len: line_len,
            spans,
        });
        prev_line_reused = false;
    }

    cache.lines = new_lines;

    HighlightRender {
        paste_id: req.paste_id,
        revision: req.revision,
        text_len: req.text.len(),
        content_hash: req.content_hash,
        language_hint: req.language_hint,
        theme_key: req.theme_key,
        lines: render_lines,
    }
}

#[cfg(test)]
mod resolver_tests {
    use super::super::{resolve_syntax, syntect_language_hint};
    use super::{
        hash_bytes, highlight_in_worker, HighlightRender, HighlightRequest, SyntectSettings,
    };
    use crate::app::highlight::worker::HighlightWorkerCache;

    fn render_for_label(settings: &SyntectSettings, label: &str, text: &str) -> HighlightRender {
        let mut cache = HighlightWorkerCache::default();
        let req = HighlightRequest {
            paste_id: "test".to_string(),
            revision: 1,
            text: text.to_string(),
            content_hash: hash_bytes(text.as_bytes()),
            language_hint: label.to_string(),
            theme_key: "base16-mocha.dark".to_string(),
        };
        highlight_in_worker(settings, &mut cache, req)
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
    fn resolve_syntax_handles_alias_labels() {
        let settings = SyntectSettings::default();
        for label in ["cs", "shell", "cpp", "powershell"] {
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
    fn resolve_syntax_leaves_known_unsupported_labels_as_plain_text() {
        let settings = SyntectSettings::default();
        for label in ["zig", "scss", "kotlin", "elixir", "dart"] {
            let syntax = resolve_syntax(&settings.ps, label);
            assert_eq!(syntax.name, "Plain Text", "label: {label}");
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
}
