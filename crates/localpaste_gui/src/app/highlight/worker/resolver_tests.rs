//! Worker incremental-pass, grammar-resolution, and retained-state regressions.

use super::super::{resolve_syntax, syntect_language_hint, HighlightRequestText};
use super::{
    highlight_in_worker, highlight_within_budget, parse_state_bytes, retained_state_budget,
    HighlightRender, HighlightRequest, HighlightWorkerResult, SyntectSettings,
};
use crate::app::highlight::worker::HighlightWorkerCache;
use ropey::Rope;
use std::collections::HashSet;
use std::rc::Rc;

fn render_for_label(settings: &SyntectSettings, label: &str, text: &str) -> HighlightRender {
    let mut cache = HighlightWorkerCache::default();
    let req = HighlightRequest {
        buffer_epoch: 0,
        paste_id: "test".to_string(),
        revision: 1,
        text: HighlightRequestText::Rope(Rope::from_str(text)),
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
        buffer_epoch: 0,
        paste_id: "test".to_string(),
        revision,
        text: HighlightRequestText::Rope(Rope::from_str(text)),
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

fn markdown_request(revision: u64, text: &str, base: Option<&str>) -> HighlightRequest {
    let mut req = rust_request(
        revision,
        text,
        base.map(|_| revision - 1),
        base.map(str::len),
    );
    req.language_hint = "markdown".into();
    req
}

/// Parser-state bytes the cache retains, counting each shared state once.
fn retained_state_bytes(cache: &HighlightWorkerCache) -> usize {
    let mut seen = HashSet::new();
    cache
        .lines
        .iter()
        .filter(|line| seen.insert(Rc::as_ptr(&line.end_state)))
        .map(|line| parse_state_bytes(&line.end_state.parse, usize::MAX).expect("unbounded"))
        .sum()
}

/// A nested list item whose opening line syntect copies into its parse state.
fn long_nested_item(following: &str, count: usize) -> (String, usize) {
    let payload = "abcdefghijklmnop".repeat(8192 / 16);
    let text = format!("- parent\n    - {payload}\n{}", following.repeat(count));
    (text, payload.len())
}

/// A cold pass that may retain any amount of parser state.
fn unbounded_markdown_render(settings: &SyntectSettings, text: &str) -> HighlightRender {
    let mut cache = HighlightWorkerCache::default();
    match highlight_within_budget(
        settings,
        &mut cache,
        markdown_request(1, text, None),
        usize::MAX,
    ) {
        HighlightWorkerResult::Render(render) => render,
        HighlightWorkerResult::Patch(_) => panic!("cold pass"),
    }
}

#[test]
fn long_list_openers_keep_retained_parser_state_within_budget() {
    let settings = SyntectSettings::default();
    // Blank lines and continuations stay in the item that captured the long
    // opener; siblings replace it. Paragraphs alternating with blank lines
    // change the state on every line, so no two neighbors can share one.
    for following in [
        "\n",
        "      continued\n",
        "    - sibling\n",
        "      text\n\n",
    ] {
        for count in [10, 1_000] {
            let (text, payload) = long_nested_item(following, count);
            let mut cache = HighlightWorkerCache::default();
            let HighlightWorkerResult::Render(render) =
                highlight_in_worker(&settings, &mut cache, markdown_request(1, &text, None))
            else {
                panic!("cold pass")
            };
            let case = format!("{following:?} x {count}");
            assert!(
                render.lines == unbounded_markdown_render(&settings, &text).lines,
                "{case} must match an unbounded cold parse"
            );
            let retained = retained_state_bytes(&cache);
            assert!(
                retained <= retained_state_budget(text.len()),
                "{case}: {retained} bytes retained for {} bytes of text",
                text.len()
            );
            if count == 10 {
                assert_eq!(cache.lines.len(), render.lines.len(), "{case} stays cached");
                // The measure must see the copied opener to bound it at all.
                assert!(retained >= payload, "{case}: {retained}");
            }
        }
    }
    // The 9,208-byte review fixture would retain about 8 MiB if cached.
    let (fixture, _) = long_nested_item("\n", 1_000);
    assert_eq!(fixture.len(), 9_208);
    let mut cache = HighlightWorkerCache::default();
    let _ = highlight_in_worker(&settings, &mut cache, markdown_request(1, &fixture, None));
    assert!(cache.lines.is_empty());
}

#[test]
fn over_budget_passes_leave_no_patch_base_until_the_text_fits_again() {
    let settings = SyntectSettings::default();
    let payload = "abcdefghijklmnop".repeat(8192 / 16);
    let (before, _) = long_nested_item("      text\n\n", 200);
    let after = before.replacen("text", "edited", 1);
    let short = after.replacen(&payload, "short", 1);
    let mut cache = HighlightWorkerCache::default();
    let mut previous: Option<&str> = None;
    for (revision, text) in [(1, &before), (2, &after), (3, &short), (4, &after)] {
        let mut req = markdown_request(revision, text, previous);
        if revision == 2 {
            req.edit_hint = Some(super::super::VirtualEditHint {
                start_byte: before.find("text").unwrap(),
                touched_lines: 1,
                inserted_chars: "edited".len(),
                deleted_chars: "text".len(),
            });
        }
        let HighlightWorkerResult::Render(render) = highlight_in_worker(&settings, &mut cache, req)
        else {
            panic!("revision {revision} has no cached base to patch")
        };
        assert!(
            render.lines == unbounded_markdown_render(&settings, text).lines,
            "revision {revision} must match an unbounded cold parse"
        );
        if revision == 3 {
            // Once the text fits the budget again, the cache resumes.
            assert_eq!(cache.lines.len(), render.lines.len());
            assert_eq!(cache.last_revision, Some(3));
        } else {
            assert!(render.changed_line_range.is_none(), "revision {revision}");
            assert!(cache.lines.is_empty(), "revision {revision}");
            assert_eq!((cache.last_revision, cache.last_text_len), (None, None));
        }
        previous = Some(text);
    }
}

#[test]
fn multiline_edit_rechecks_changed_lines_after_unchanged_interior_line() {
    let settings = SyntectSettings::default();
    for separator in [
        "\r", "\n", "\r\n", "\u{b}", "\u{c}", "\u{85}", "\u{2028}", "\u{2029}",
    ] {
        let before =
            format!("let a = 1;{separator}{separator}let b = 2;{separator}let c = 3;{separator}");
        let after = format!(
            "    let a = 1;{separator}{separator}// let b = 2;{separator}let c = 3;{separator}"
        );
        let mut cache = HighlightWorkerCache::default();
        let HighlightWorkerResult::Render(mut base) =
            highlight_in_worker(&settings, &mut cache, rust_request(1, &before, None, None))
        else {
            panic!("cold render")
        };
        let mut next = rust_request(2, &after, Some(1), Some(before.len()));
        next.edit_hint = Some(super::super::VirtualEditHint {
            start_byte: 0,
            touched_lines: 3,
            inserted_chars: after.find("let c").unwrap(),
            deleted_chars: before.find("let c").unwrap(),
        });
        match highlight_in_worker(&settings, &mut cache, next) {
            HighlightWorkerResult::Render(render) => base = render,
            HighlightWorkerResult::Patch(patch) => {
                base.lines.splice(patch.line_range, patch.lines);
            }
        }
        let cold = render_for_label(&settings, "rust", &after);
        assert!(
            base.lines == cold.lines,
            "block edits must match a cold parse with {separator:?}"
        );
    }
}

#[test]
fn editor_line_hints_remain_safe_with_non_lf_line_breaks() {
    let settings = SyntectSettings::default();
    for separator in [
        "\r", "\n", "\r\n", "\u{b}", "\u{c}", "\u{85}", "\u{2028}", "\u{2029}",
    ] {
        let comment = format!("// comment{separator}let b = 2;\n");
        let fence = format!("```text{separator}body{separator}```{separator}prose\n");
        for (label, text, following_line, expected) in [
            ("rust", comment, 1, "let b = 2;\n"),
            ("markdown", fence, 3, "prose\n"),
        ] {
            let render = render_for_label(&settings, label, &text);
            let standalone = render_for_label(&settings, label, expected);
            assert!(
                render.lines[following_line] == standalone.lines[0],
                "stateful {label} with {separator:?}"
            );
        }
        let before = format!("let café = 1;{separator}let été = 2;\n");
        let after = before.replace('2', "200");
        let mut cache = HighlightWorkerCache::default();
        let HighlightWorkerResult::Render(mut base) =
            highlight_in_worker(&settings, &mut cache, rust_request(1, &before, None, None))
        else {
            panic!("cold render")
        };
        let rope = Rope::from_str(&before);
        assert_eq!(
            base.lines.len(),
            rope.len_lines(),
            "line separator {separator:?}"
        );
        for (highlighted, line) in base.lines.iter().zip(rope.lines()) {
            assert_eq!(highlighted.len, line.len_bytes());
            let source = line.to_string();
            for span in &highlighted.spans {
                assert!(source.is_char_boundary(span.range.start));
                assert!(source.is_char_boundary(span.range.end));
            }
        }
        let following = render_for_label(&settings, "rust", "let été = 2;\n");
        assert!(
            base.lines[1] == following.lines[0],
            "line separator {separator:?}"
        );
        let mut next = rust_request(2, &after, Some(1), Some(before.len()));
        next.edit_hint = Some(super::super::VirtualEditHint {
            start_byte: before.find('2').unwrap(),
            touched_lines: 1,
            inserted_chars: 3,
            deleted_chars: 1,
        });
        match highlight_in_worker(&settings, &mut cache, next) {
            HighlightWorkerResult::Render(render) => base = render,
            HighlightWorkerResult::Patch(patch) => {
                base.lines.splice(patch.line_range, patch.lines);
            }
        }
        let cold = render_for_label(&settings, "rust", &after);
        assert!(base.lines == cold.lines, "line separator {separator:?}");
    }
}

#[test]
fn crlf_boundary_edits_do_not_reuse_stale_prefix_lines() {
    let settings = SyntectSettings::default();
    for (before, old, new) in [
        ("a\rb\n# c\n", "b", ""),
        ("// alpha\r\nlet b=2;\n", "\n", "x"),
        ("// alpha\r\nlet b=2;\n", "\n", ""),
        ("// alpha\rxlet b=2;\n", "x", "\n"),
    ] {
        let after = before.replacen(old, new, 1);
        let mut cache = HighlightWorkerCache::default();
        let HighlightWorkerResult::Render(mut base) =
            highlight_in_worker(&settings, &mut cache, rust_request(1, before, None, None))
        else {
            panic!("cold render")
        };
        let mut next = rust_request(2, &after, Some(1), Some(before.len()));
        next.edit_hint = Some(super::super::VirtualEditHint {
            start_byte: before.find(old).unwrap(),
            touched_lines: 2,
            inserted_chars: new.chars().count(),
            deleted_chars: old.chars().count(),
        });
        match highlight_in_worker(&settings, &mut cache, next) {
            HighlightWorkerResult::Render(render) => base = render,
            HighlightWorkerResult::Patch(patch) => {
                base.lines.splice(patch.line_range, patch.lines);
            }
        }
        assert!(base.lines == render_for_label(&settings, "rust", &after).lines);
    }
}

#[test]
fn markdown_fence_edit_updates_downstream_highlights_like_a_cold_parse() {
    let settings = SyntectSettings::default();
    for (before, old, new) in [
        (
            "```rust\nfn main() {}\n```\nprose after fence\n",
            "```rust\n",
            "",
        ),
        (
            "# Note\n```rust\nfn main() {}\n```\nprose after fence\nmore prose\n",
            "```\nprose",
            "prose",
        ),
        (
            "# Note\n```rust\nfn main() {}\n```\nprose after fence\n",
            "```rust\n",
            "",
        ),
        (
            "# Note\n```rust\nfn main() {}\nprose after fence\nmore prose\n",
            "prose after fence",
            "```\nprose after fence",
        ),
        (
            "# Note\nfn main() {}\nmore prose\n",
            "fn main() {}",
            "```rust\nfn main() {}",
        ),
        (
            "# Note\r\n```rust\r\nfn main() {}\r\n```\r\nprose after fence\r\n",
            "```\r\nprose",
            "prose",
        ),
        (
            "# Note\n```rust\nfn main() {}\n```\nprose after fence\n",
            "```\nprose",
            "~~~\nprose",
        ),
        ("```rust\ncode\n```\nprose\n", "rust", "python"),
        ("````rust\ncode\n```\nprose\n", "````rust", "```rust"),
        ("```rust\ncode\n```", "```rust", "````rust"),
        ("a\rb\n# c\n", "b", ""),
        ("a\u{2028}b\n# c\n", "b", "longer"),
        (
            "- item\n   ```sh\n     ls\n     ```\n  prose\n",
            "     ```",
            "      ```",
        ),
        (
            "1. item\n    ~~~sh\n      ls\n      ~~~\n   prose\n",
            "~~~sh",
            "~~~~sh",
        ),
        (
            "> ```text\n> code\n> # Inside quote\nOutside prose\n",
            "> # Inside quote",
            "# Outside quote",
        ),
    ] {
        let mut cache = HighlightWorkerCache::default();
        let after = before.replacen(old, new, 1);
        let mut first = rust_request(1, before, None, None);
        first.language_hint = "markdown".into();
        let HighlightWorkerResult::Render(mut base) =
            highlight_in_worker(&settings, &mut cache, first)
        else {
            panic!("cold render")
        };
        // Apply, undo, and redo against the same worker cache. Each step
        // must match an independent cold parse, including line lengths.
        for (step, (previous, current, removed, inserted)) in [
            (before, after.as_str(), old, new),
            (after.as_str(), before, new, old),
            (before, after.as_str(), old, new),
        ]
        .into_iter()
        .enumerate()
        {
            let revision = step as u64 + 2;
            let mut next =
                rust_request(revision, current, Some(revision - 1), Some(previous.len()));
            next.language_hint = "markdown".into();
            next.edit_hint = Some(super::super::VirtualEditHint {
                start_byte: before.find(old).unwrap(),
                touched_lines: removed
                    .matches('\n')
                    .count()
                    .max(inserted.matches('\n').count())
                    + 1,
                inserted_chars: inserted.chars().count(),
                deleted_chars: removed.chars().count(),
            });
            if step > 0 {
                // Actual history operations clear the single-edit hint.
                next.edit_hint = None;
            }
            match highlight_in_worker(&settings, &mut cache, next) {
                HighlightWorkerResult::Render(render) => base = render,
                HighlightWorkerResult::Patch(patch) => {
                    base.lines.splice(patch.line_range, patch.lines);
                }
            }
            let cold = render_for_label(&settings, "markdown", current);
            assert!(
                base.lines == cold.lines,
                "incremental step {step} must match cold parse: {old:?} -> {new:?}"
            );
        }
    }
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
    assert_eq!(syntect_language_hint("jsonl"), "json");
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
        buffer_epoch: 0,
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
