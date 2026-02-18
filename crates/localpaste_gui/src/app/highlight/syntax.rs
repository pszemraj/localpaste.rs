//! Syntax hint normalization and syntect grammar resolution helpers.

use syntect::parsing::{SyntaxReference, SyntaxSet};

/// Normalizes user-facing language names into syntect-compatible hints.
///
/// # Returns
/// Canonical language hint or `"text"` when input is empty/unknown.
pub(crate) fn syntect_language_hint(language: &str) -> String {
    let canonical = localpaste_core::detection::canonical::canonicalize(language);
    if canonical.is_empty() {
        "text".to_string()
    } else {
        canonical
    }
}

fn normalized_syntax_key(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .map(|ch| ch.to_ascii_lowercase())
        .collect()
}

fn try_resolve_syntax_candidate<'a>(
    ps: &'a SyntaxSet,
    candidate: &str,
) -> Option<&'a SyntaxReference> {
    let trimmed = candidate.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(syntax) = ps.find_syntax_by_name(trimmed) {
        return Some(syntax);
    }
    if let Some(syntax) = ps.find_syntax_by_extension(trimmed) {
        return Some(syntax);
    }

    for syntax in ps.syntaxes() {
        if syntax.name.eq_ignore_ascii_case(trimmed) {
            return Some(syntax);
        }
    }

    let normalized = normalized_syntax_key(trimmed);
    if !normalized.is_empty() {
        for syntax in ps.syntaxes() {
            if normalized_syntax_key(syntax.name.as_str()) == normalized {
                return Some(syntax);
            }
        }
    }

    ps.syntaxes().iter().find(|syntax| {
        syntax
            .file_extensions
            .iter()
            .any(|ext| ext.eq_ignore_ascii_case(trimmed))
    })
}

fn syntax_fallback_candidates(hint_lower: &str) -> &'static [&'static str] {
    match hint_lower {
        "cs" => &["C#", "cs"],
        "shell" => &["Bourne Again Shell (bash)", "bash", "sh"],
        "cpp" => &["C++", "cpp", "cc"],
        "objectivec" => &["Objective-C", "m"],
        "dockerfile" => &["Dockerfile", "bash", "sh"],
        "makefile" => &["Makefile", "make"],
        "latex" => &["LaTeX", "tex"],
        // Syntect defaults used by egui do not ship native grammars for these in all bundles.
        // Keep explicit fallback only for high-priority labels to avoid hiding unsupported
        // language gaps behind misleading tokenization.
        "typescript" => &["JavaScript", "js", "ts"],
        "toml" => &["Java Properties", "properties", "YAML", "yaml"],
        "swift" => &["Rust", "rs", "Go", "go", "Objective-C"],
        "powershell" => &["ps1", "Bourne Again Shell (bash)", "bash", "sh"],
        "sass" => &["sass", "Ruby Haml", "css"],
        _ => &[],
    }
}

/// Resolves a syntect grammar using canonical hint + fallback candidates.
///
/// # Arguments
/// - `ps`: Loaded syntect syntax set.
/// - `hint`: Canonicalized language hint from app state.
///
/// # Returns
/// Best matching syntax definition, falling back to plain text.
pub(crate) fn resolve_syntax<'a>(ps: &'a SyntaxSet, hint: &str) -> &'a SyntaxReference {
    let hint_trimmed = hint.trim();
    if hint_trimmed.is_empty() {
        return ps.find_syntax_plain_text();
    }

    let hint_lower = hint_trimmed.to_ascii_lowercase();
    if matches!(hint_lower.as_str(), "text" | "txt" | "plain" | "plaintext") {
        return ps.find_syntax_plain_text();
    }

    if let Some(syntax) = try_resolve_syntax_candidate(ps, hint_trimmed) {
        return syntax;
    }

    for candidate in syntax_fallback_candidates(hint_lower.as_str()) {
        if let Some(syntax) = try_resolve_syntax_candidate(ps, candidate) {
            return syntax;
        }
    }

    ps.find_syntax_plain_text()
}
