//! Definition handles such as `fn handle_request` for source in recognized languages.

use crate::detection::canonical::canonicalize;

/// Find the first sampled line that opens a definition in `language`.
///
/// # Arguments
/// - `sample`: Bounded semantic sample whose lines are inspected in order.
/// - `language`: Optional language label used to select supported definition patterns.
///
/// # Returns
/// That line's handle (see [`extract_definition_handle_from_line`]), or `None`.
pub(super) fn extract_definition_handle(sample: &str, language: Option<&str>) -> Option<String> {
    sample
        .lines()
        .find_map(|line| extract_definition_handle_from_line(line, language))
}

/// Extract a compact definition handle from one source line.
///
/// # Arguments
/// - `line`: Source line to inspect.
/// - `language`: Optional language label used to select supported definition patterns.
///
/// # Returns
/// A code-facing handle such as `fn run` or `export function render`, when the
/// line starts with a supported definition pattern for `language`.
pub(super) fn extract_definition_handle_from_line(
    line: &str,
    language: Option<&str>,
) -> Option<String> {
    let lang = canonicalize(language.unwrap_or_default().trim());
    let patterns: &[&str] = match lang.as_str() {
        "rust" => &["fn ", "struct ", "enum ", "trait ", "impl "],
        "python" => &["def ", "class ", "async def "],
        "javascript" | "typescript" => &[
            "function ",
            "class ",
            "const ",
            "export function ",
            "export class ",
            "export const ",
        ],
        "go" => &["func ", "type ", "package "],
        _ => return None,
    };

    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') {
        return None;
    }
    for pattern in patterns {
        if let Some(rest) = trimmed.strip_prefix(pattern) {
            let ident: String = rest
                .chars()
                .take_while(|ch| ch.is_alphanumeric() || *ch == '_')
                .collect();
            if !ident.is_empty() {
                return Some(format!("{} {}", pattern.trim_end(), ident));
            }
        }
    }

    None
}
