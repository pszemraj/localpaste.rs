//! Language normalization and export filename helpers.

pub(in crate::app) use crate::backend::collections::matches_active_filters;

/// Normalizes an optional language filter into canonical storage form.
///
/// # Returns
/// Canonical language filter string, or `None` when unset/blank.
pub(super) fn normalize_language_filter_value(value: Option<&str>) -> Option<String> {
    localpaste_core::models::paste::normalize_language_filter(value)
}

/// Maps canonical language labels to preferred export file extensions.
///
/// # Returns
/// Extension without leading dot, defaulting to `"txt"`.
pub(super) fn language_extension(language: Option<&str>) -> &'static str {
    localpaste_core::detection::preferred_extension(language)
}

/// Sanitizes a filename candidate for cross-platform export compatibility.
///
/// # Returns
/// Safe filename with reserved characters replaced by `_`.
pub(super) fn sanitize_filename(value: &str) -> String {
    let mut out: String = value
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ => ch,
        })
        .collect();
    out = out.trim().to_string();
    if out.is_empty() {
        "localpaste-export".to_string()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_extension_maps_known_and_unknown_languages() {
        assert_eq!(language_extension(Some("rust")), "rs");
        assert_eq!(language_extension(Some(" Python ")), "py");
        assert_eq!(language_extension(Some("csharp")), "cs");
        assert_eq!(language_extension(Some("bash")), "sh");
        assert_eq!(language_extension(Some("scss")), "scss");
        assert_eq!(language_extension(Some("unknown")), "txt");
        assert_eq!(language_extension(None), "txt");
    }

    #[test]
    fn sanitize_filename_replaces_reserved_chars_and_falls_back() {
        assert_eq!(sanitize_filename("bad<>:\"/\\|?*name"), "bad_________name");
        assert_eq!(sanitize_filename("   "), "localpaste-export");
    }
}
