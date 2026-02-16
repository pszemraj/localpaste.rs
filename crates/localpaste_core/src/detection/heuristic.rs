//! Heuristic language detection fallback for text content.

use crate::models::paste::is_markdown_content;

/// Best-effort language detection based on simple heuristics.
pub(crate) fn detect(content: &str) -> Option<String> {
    const SAMPLE_MAX_BYTES: usize = 64 * 1024;
    const SAMPLE_MAX_LINES: usize = 512;

    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }
    let sample = utf8_prefix(trimmed, SAMPLE_MAX_BYTES);
    let lower = sample.to_ascii_lowercase();
    let lines = || sample.lines().take(SAMPLE_MAX_LINES);

    // JSON: structural check without full parsing (avoids expensive serde_json).
    if sample.starts_with('{') || sample.starts_with('[') {
        // When sampling truncates very large JSON payloads, the prefix may not end
        // with the final closing delimiter. Keep large-document detection stable.
        let sample_truncated = sample.len() < trimmed.len();
        let looks_closed = sample.ends_with('}') || sample.ends_with(']');
        if sample.contains('"')
            && (sample.contains(':') || sample.starts_with('['))
            && (looks_closed || sample_truncated)
        {
            return Some("json".to_string());
        }
    }

    // HTML before generic XML so we don't mis-classify.
    // Require either a strong root marker or multiple tag hits near document-like syntax.
    let html_tag_hits = [
        "<html", "<head", "<body", "<div", "<span", "<script", "<style",
    ]
    .iter()
    .filter(|tag| lower.contains(**tag))
    .count();
    if lower.contains("<!doctype html")
        || lower.contains("<html")
        || (sample.trim_start().starts_with('<') && html_tag_hits >= 2)
    {
        return Some("html".to_string());
    }

    if lower.starts_with("<?xml")
        || (sample.trim_start().starts_with('<')
            && lower.contains("</")
            && !lower.contains("<html")
            && !lower.contains("<!doctype html"))
    {
        return Some("xml".to_string());
    }

    let perl_hits = ["use strict;", "use warnings;", "my $", "sub ", "package "]
        .iter()
        .filter(|kw| lower.contains(**kw))
        .count();
    if lower.starts_with("#!/usr/bin/env perl")
        || lower.starts_with("#!/usr/bin/perl")
        || perl_hits >= 2
    {
        return Some("perl".to_string());
    }

    let has_ps_command = lower.contains("write-host")
        || lower.contains("$psversiontable")
        || lower.contains("set-strictmode")
        || lower.contains("get-childitem");
    let has_param_block = lines().any(|line| {
        let trimmed = line.trim_start();
        trimmed.starts_with("param(") || trimmed.starts_with("param (")
    });
    let has_ps_variable = lower.contains("$env:")
        || lower.contains("$ps")
        || lower.contains("$_")
        || lower.contains("$true")
        || lower.contains("$false");
    if has_ps_command || (has_param_block && has_ps_variable) {
        return Some("powershell".to_string());
    }

    let shell_hits = [
        (lower.contains("echo ") && lower.contains('$')),
        lower.contains("\nfi"),
        lower.contains("\ndone"),
        lower.contains("if ["),
        lower.contains(" then\n"),
        (lower.contains("case ") && lower.contains(" esac")),
    ]
    .iter()
    .filter(|hit| **hit)
    .count();
    if lower.starts_with("#!/bin/")
        || lower.starts_with("#!/usr/bin/env bash")
        || (shell_hits >= 2 && lower.contains('\n'))
    {
        return Some("shell".to_string());
    }

    let yaml_pairs = lines()
        .filter(|l| {
            let t = l.trim();
            if t.is_empty() || t.starts_with('#') {
                return false;
            }
            (t.starts_with("- ") || t.contains(": ")) && !t.contains('{')
        })
        .count();
    if ((lower.starts_with("---") && yaml_pairs >= 1) || yaml_pairs >= 2) && !sample.contains('{') {
        return Some("yaml".to_string());
    }

    let has_toml_header = lines().any(|l| {
        let t = l.trim();
        t.starts_with('[') && t.ends_with(']') && t.len() > 2
    });
    let toml_assignments = lines()
        .filter(|l| {
            let t = l.trim();
            if t.is_empty() || t.starts_with('#') || t.starts_with('[') {
                return false;
            }
            t.contains('=') && !t.contains("==")
        })
        .count();
    if has_toml_header && toml_assignments >= 1 {
        return Some("toml".to_string());
    }

    if is_markdown_content(sample) {
        return Some("markdown".to_string());
    }

    let latex_hits = [
        "\\begin{",
        "\\end{",
        "\\usepackage",
        "\\section",
        "\\subsection",
    ]
    .iter()
    .filter(|kw| lower.contains(**kw))
    .count();
    if lower.contains("\\documentclass") || latex_hits >= 2 {
        return Some("latex".to_string());
    }

    if lower.contains('{') && lower.contains('}') && lower.contains(':') && lower.contains(';') {
        let css_tokens = [
            "color:",
            "background",
            "margin",
            "padding",
            "font-",
            "display",
            "position",
            "flex",
            "grid",
        ];
        if css_tokens.iter().any(|token| lower.contains(token)) {
            return Some("css".to_string());
        }
    }

    let keyword_hits =
        |keywords: &[&str]| -> usize { keywords.iter().filter(|kw| lower.contains(*kw)).count() };

    // Specialized checks for languages with distinctive constructs.
    let cs_has_using_system = lower.contains("using system");
    let cs_has_namespace_class = lower.contains("namespace ") && lower.contains("class ");
    let cs_has_console = lower.contains("console.");
    if (cs_has_using_system && (cs_has_namespace_class || cs_has_console))
        || (cs_has_namespace_class && cs_has_console)
    {
        return Some("csharp".to_string());
    }

    let cpp_has_std_scope = lower.contains("std::");
    let cpp_has_usage = lower.contains("cout")
        || lower.contains("vector<")
        || lower.contains("::iterator")
        || lower.contains("int main(");
    if lower.contains("using namespace std")
        || lower.contains("template <")
        || (cpp_has_std_scope && cpp_has_usage)
    {
        return Some("cpp".to_string());
    }

    if lower.contains("#include") && (lower.contains("int main") || lower.contains("printf")) {
        return Some("c".to_string());
    }

    let swift_has_import = lower.contains("import foundation");
    let swift_has_func = lower.contains("func ");
    let swift_has_shape = lower.contains("print(")
        || lower.contains("guard let")
        || lower.contains("protocol ")
        || lower.contains("let ")
        || lower.contains("var ");
    if (swift_has_import && swift_has_func && swift_has_shape)
        || (swift_has_func && (lower.contains("guard let") || lower.contains("protocol ")))
    {
        return Some("swift".to_string());
    }

    let scored_languages: &[(&str, &[&str], usize)] = &[
        (
            "rust",
            &[
                "fn ", "impl", "crate::", "let ", "mut ", "pub ", "struct ", "enum", "match ",
                "trait", "println!",
            ],
            2,
        ),
        (
            "python",
            &[
                "def ",
                "import ",
                "from ",
                "class ",
                "self",
                "async def",
                "elif",
                "print(",
            ],
            2,
        ),
        (
            "javascript",
            &[
                "function",
                "const ",
                "let ",
                "=>",
                "console.",
                "document.",
                "export ",
                "import ",
            ],
            2,
        ),
        (
            "typescript",
            &[
                "interface ",
                " type ",
                ": string",
                ": number",
                "implements ",
                " enum ",
                "<t>",
                "readonly ",
            ],
            2,
        ),
        (
            "go",
            &[
                "package ",
                "func ",
                "fmt.",
                "defer ",
                "go ",
                "chan",
                "interface",
                "select {",
            ],
            2,
        ),
        (
            "java",
            &[
                "public class",
                "import java.",
                "system.out",
                " implements ",
                " extends ",
                " void main",
            ],
            2,
        ),
        (
            "csharp",
            &[
                "using system",
                "namespace ",
                "public class",
                "console.",
                " async ",
                " task<",
                " get;",
            ],
            2,
        ),
        (
            "sql",
            &[
                "select ",
                "insert ",
                "update ",
                "delete ",
                " from ",
                " where ",
                " join ",
                "create table",
            ],
            2,
        ),
        (
            "latex",
            &[
                "\\begin{",
                "\\end{",
                "\\usepackage",
                "\\documentclass",
                "\\section",
            ],
            2,
        ),
        (
            "kotlin",
            &[
                "fun ",
                "data class",
                "companion object",
                "val ",
                "var ",
                "when (",
                "println(",
            ],
            2,
        ),
        (
            "swift",
            &[
                "import foundation",
                "guard let",
                "protocol ",
                "func ",
                "let ",
                "var ",
            ],
            2,
        ),
        (
            "dart",
            &[
                "void main()",
                "import 'package:",
                "class ",
                "final ",
                "future<",
                "=>",
            ],
            2,
        ),
        (
            "zig",
            &[
                "const std = @import",
                "pub fn main(",
                "comptime",
                "@import(",
                "var ",
            ],
            2,
        ),
        (
            "lua",
            &["local ", "function ", "end", "require(", "then", "elseif"],
            2,
        ),
        (
            "perl",
            &["use strict;", "use warnings;", "my $", "sub ", "package "],
            2,
        ),
        (
            "elixir",
            &["defmodule ", "defp ", "fn ", "|>", "end", "io.puts"],
            2,
        ),
        (
            "powershell",
            &[
                "write-host",
                "get-childitem",
                "$psversiontable",
                "param(",
                "set-strictmode",
            ],
            2,
        ),
    ];

    let mut best_match: Option<(&str, usize)> = None;
    for (lang, keywords, threshold) in scored_languages {
        let hits = keyword_hits(keywords);
        if hits >= *threshold {
            match best_match {
                Some((_, best_hits)) if best_hits >= hits => {}
                _ => best_match = Some((*lang, hits)),
            }
        }
    }

    if let Some((lang, _)) = best_match {
        return Some(lang.to_string());
    }

    None
}

fn utf8_prefix(content: &str, max_bytes: usize) -> &str {
    if content.len() <= max_bytes {
        return content;
    }
    let mut end = max_bytes;
    while end > 0 && !content.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    &content[..end]
}
