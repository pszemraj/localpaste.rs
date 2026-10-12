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
/// The handle is the definition keyword and the item's name. Qualifiers before
/// the keyword are skipped: Rust visibility (`pub`, `pub(crate)`) and `const`,
/// `async`, `unsafe`, and `extern "C"`; JavaScript and TypeScript `default`,
/// `async`, `declare`, and `abstract`; and Python's `async`. JavaScript and
/// TypeScript keep `export`, which marks the module's public items, and a Go
/// method's receiver is skipped, so `func (s *Server) Handle` gets `func Handle`.
///
/// # Returns
/// A code-facing handle such as `fn run` or `export function render`, when the
/// line starts with a supported definition for `language`.
pub(super) fn extract_definition_handle_from_line(
    line: &str,
    language: Option<&str>,
) -> Option<String> {
    let lang = canonicalize(language.unwrap_or_default().trim());
    let (qualifiers, keywords): (&[&str], &[&str]) = match lang.as_str() {
        "rust" => (
            &["pub", "const", "async", "unsafe", "extern"],
            &["fn", "struct", "enum", "trait", "impl"],
        ),
        "python" => (&["async"], &["def", "class"]),
        "javascript" | "typescript" => (
            &["export", "default", "async", "declare", "abstract"],
            &["function", "class", "const"],
        ),
        "go" => (&[], &["func", "type", "package"]),
        _ => return None,
    };

    let mut rest = line.trim();
    if rest.is_empty() || rest.starts_with("//") || rest.starts_with('#') {
        return None;
    }
    let mut exported = false;
    while let Some((qualifier, after)) = qualifiers.iter().find_map(|qualifier| {
        let after = rest.strip_prefix(qualifier)?;
        (after.starts_with(char::is_whitespace) || after.starts_with('('))
            .then_some((*qualifier, after))
    }) {
        let after = match after.strip_prefix('(') {
            // `pub(crate)`, `pub(in crate::db)`; only Rust visibility takes parentheses.
            Some(scope) if qualifier == "pub" => scope.split_once(')')?.1,
            Some(_) => return None,
            None => after,
        }
        .trim_start();
        // `extern "C" fn`: the ABI string belongs to the qualifier.
        rest = match after.strip_prefix('"') {
            Some(abi) if qualifier == "extern" => abi.split_once('"')?.1.trim_start(),
            _ => after,
        };
        exported |= qualifier == "export";
    }

    let (keyword, after) = keywords.iter().find_map(|keyword| {
        rest.strip_prefix(keyword)
            .filter(|after| after.starts_with(char::is_whitespace))
            .map(|after| (*keyword, after.trim_start()))
    })?;
    // A Go method names its receiver first: `func (s *Server) Handle`.
    let after = match after.strip_prefix('(') {
        Some(receiver) if keyword == "func" => receiver.split_once(')')?.1.trim_start(),
        _ => after,
    };
    let ident: String = after
        .chars()
        .take_while(|ch| ch.is_alphanumeric() || *ch == '_')
        .collect();
    let export = if exported { "export " } else { "" };
    (!ident.is_empty()).then(|| format!("{export}{keyword} {ident}"))
}
