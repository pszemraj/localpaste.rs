//! Language detection abstraction with Magika and heuristic fallback.

/// Language canonicalization and manual UI option tables.
pub mod canonical;
mod heuristic;
#[cfg(feature = "magika")]
mod magika;
#[cfg(test)]
mod tests;

/// Detect language/type of text content.
///
/// # Returns
/// Canonicalized language label when detection succeeds, otherwise `None`.
pub fn detect_language(content: &str) -> Option<String> {
    #[cfg(feature = "magika")]
    {
        if let Some(label) = magika::detect(content) {
            let canonical = canonical::canonicalize(&label);
            if let Some(refined) = refine_magika_label(&canonical, content) {
                return Some(refined);
            }
        }
    }

    heuristic::detect(content)
        .map(|label| canonical::canonicalize(&label))
        .filter(|label| !label.is_empty() && label != "text")
}

#[cfg(feature = "magika")]
fn refine_magika_label(label: &str, content: &str) -> Option<String> {
    if label.is_empty() || label == "text" {
        return None;
    }

    if label == "yaml" && !looks_like_yaml(content) {
        return None;
    }

    if label == "scss" && looks_like_plain_css(content) {
        return Some("css".to_string());
    }

    Some(label.to_string())
}

#[cfg(feature = "magika")]
fn looks_like_yaml(content: &str) -> bool {
    let trimmed = content.trim_start();
    if trimmed.starts_with("---") {
        return true;
    }

    let yaml_pairs = content
        .lines()
        .take(512)
        .filter(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return false;
            }
            (trimmed.starts_with("- ")
                || trimmed.contains(": ")
                || (trimmed.ends_with(':') && trimmed.len() > 1))
                && !trimmed.contains('{')
        })
        .count();

    yaml_pairs >= 2
}

#[cfg(feature = "magika")]
fn looks_like_plain_css(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    let has_css_block = lower.contains('{')
        && lower.contains('}')
        && lower.contains(':')
        && (lower.contains(';') || lower.contains('\n'));
    let has_scss_specific_tokens = lower.contains('$')
        || lower.contains("@mixin")
        || lower.contains("@include")
        || lower.contains("@extend")
        || lower.contains("#{");

    has_css_block && !has_scss_specific_tokens
}

/// Initialize the Magika model session early when available.
pub fn prewarm() {
    #[cfg(feature = "magika")]
    {
        magika::prewarm();
    }
}
