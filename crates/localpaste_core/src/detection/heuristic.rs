//! Heuristic language detection fallback for text content.

use super::looks_like_yaml;
use crate::models::paste::is_markdown_content;

/// Best-effort language detection based on simple heuristics.
///
/// # Returns
/// Canonical-friendly language label when a strong pattern is found, otherwise
/// `None`.
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
    let shebang = shebang_interpreter(sample);

    if matches!(
        shebang.as_deref(),
        Some("python" | "python2" | "python3" | "pypy" | "pypy3")
    ) {
        return Some("python".to_string());
    }
    if matches!(shebang.as_deref(), Some("node" | "nodejs" | "deno" | "bun")) {
        return Some("javascript".to_string());
    }

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
    if matches!(shebang.as_deref(), Some("perl")) || perl_hits >= 2 {
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
    let shebang_is_shell = matches!(
        shebang.as_deref(),
        Some("sh" | "bash" | "zsh" | "ksh" | "dash" | "fish" | "ash")
    );
    if shebang_is_shell || (shell_hits >= 2 && lower.contains('\n')) {
        return Some("shell".to_string());
    }

    if looks_like_yaml(sample) {
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

    if looks_like_python_from_import(sample) {
        return Some("python".to_string());
    }

    if looks_like_sql(sample) {
        return Some("sql".to_string());
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
            &[
                "local ",
                "function ",
                "require(",
                "elseif",
                "pairs(",
                "ipairs(",
            ],
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

fn shebang_interpreter(sample: &str) -> Option<String> {
    let first_line = sample.lines().next()?.trim();
    let interpreter_line = first_line.strip_prefix("#!")?.trim();
    if interpreter_line.is_empty() {
        return None;
    }

    let mut parts = interpreter_line.split_whitespace();
    let first = parts.next()?;
    let mut interpreter = first;
    if path_basename(first).eq_ignore_ascii_case("env") {
        for arg in parts {
            if arg.starts_with('-') {
                continue;
            }
            interpreter = arg;
            break;
        }
    }

    let basename = path_basename(interpreter).trim();
    if basename.is_empty() {
        return None;
    }
    Some(basename.to_ascii_lowercase())
}

fn path_basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

fn looks_like_python_from_import(sample: &str) -> bool {
    sample.lines().take(512).any(|line| {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            return false;
        }
        let Some(rest) = trimmed.strip_prefix("from ") else {
            return false;
        };
        let Some((module, imported)) = rest.split_once(" import ") else {
            return false;
        };
        let module = module.trim();
        let imported = imported.trim();
        !module.is_empty()
            && !imported.is_empty()
            && module
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '.')
    })
}

fn looks_like_sql(sample: &str) -> bool {
    sample.lines().take(512).any(|line| {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("--") || trimmed.starts_with('#') {
            return false;
        }
        let lower = trimmed.to_ascii_lowercase();
        looks_like_select_sql_line(lower.as_str())
            || looks_like_insert_sql_line(lower.as_str())
            || looks_like_update_sql_line(lower.as_str())
            || looks_like_delete_sql_line(lower.as_str())
            || lower.starts_with("create table ")
            || lower.starts_with("alter table ")
            || lower.starts_with("drop table ")
    })
}

fn looks_like_select_sql_line(line: &str) -> bool {
    let Some(after_select) = line.strip_prefix("select ") else {
        return false;
    };
    let Some((projection, from_tail)) = after_select.split_once(" from ") else {
        return false;
    };
    let projection = projection.trim();
    if projection.is_empty() {
        return false;
    }
    let projection_sqlish = projection.contains(',')
        || projection.contains('*')
        || projection.contains('.')
        || projection.contains('(')
        || is_sql_identifier(projection);
    if !projection_sqlish {
        return false;
    }

    let source = from_tail
        .split_whitespace()
        .next()
        .map(|token| token.trim_matches(|ch: char| matches!(ch, ',' | ';')))
        .unwrap_or("");
    is_sql_identifier_path(source)
}

fn looks_like_insert_sql_line(line: &str) -> bool {
    line.starts_with("insert into ")
        && (line.contains(" values ")
            || line.contains(" select ")
            || line.contains(" default values"))
}

fn looks_like_update_sql_line(line: &str) -> bool {
    line.starts_with("update ") && line.contains(" set ")
}

fn looks_like_delete_sql_line(line: &str) -> bool {
    line.starts_with("delete from ") && (line.contains(" where ") || line.ends_with(';'))
}

fn is_sql_identifier_path(token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    token.split('.').all(is_sql_identifier)
}

fn is_sql_identifier(token: &str) -> bool {
    let token = token.trim_matches(|ch| matches!(ch, '"' | '\'' | '`' | '[' | ']'));
    let mut chars = token.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
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
