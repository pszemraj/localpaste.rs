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

pub(crate) fn looks_like_yaml(content: &str) -> bool {
    let trimmed = content.trim_start();
    if trimmed.starts_with("---") {
        return true;
    }

    let mut yaml_pairs = 0usize;
    let mut meaningful_lines = 0usize;
    let mut first_meaningful_line: Option<&str> = None;

    for line in content.lines().take(512) {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        meaningful_lines = meaningful_lines.saturating_add(1);
        if first_meaningful_line.is_none() {
            first_meaningful_line = Some(trimmed);
        }
        if trimmed.starts_with("- ")
            || trimmed.contains(": ")
            || (trimmed.ends_with(':') && trimmed.len() > 1)
        {
            let yaml_like = if trimmed.ends_with(':') && trimmed.len() > 1 {
                true
            } else {
                looks_like_single_line_yaml_mapping(trimmed)
            };
            if yaml_like {
                yaml_pairs = yaml_pairs.saturating_add(1);
            }
        }
    }

    if yaml_pairs >= 2 {
        return true;
    }

    if yaml_pairs == 1 && meaningful_lines == 1 {
        return first_meaningful_line
            .map(looks_like_single_line_yaml_mapping)
            .unwrap_or(false);
    }

    false
}

fn looks_like_single_line_yaml_mapping(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.starts_with("- ") {
        return true;
    }

    let Some((raw_key, raw_value)) = trimmed.split_once(':') else {
        return false;
    };
    let key = raw_key.trim();
    if key.is_empty() {
        return false;
    }
    if key.contains(char::is_whitespace)
        && !(key.starts_with('"') && key.ends_with('"'))
        && !(key.starts_with('\'') && key.ends_with('\''))
    {
        return false;
    }

    let value = raw_value.trim();
    if value.contains(';') {
        return false;
    }
    if value.contains('{') || value.contains('}') {
        return looks_like_yaml_flow_mapping(value);
    }
    if value.contains('[') || value.contains(']') {
        return false;
    }
    if value.contains(char::is_control) {
        return false;
    }
    if !value.starts_with('"') && !value.starts_with('\'') && value.split_whitespace().count() > 3 {
        return false;
    }

    true
}

fn looks_like_yaml_flow_mapping(value: &str) -> bool {
    let trimmed = value.trim();
    if !trimmed.starts_with('{') || !trimmed.ends_with('}') {
        return false;
    }

    let inner = &trimmed[1..trimmed.len().saturating_sub(1)];
    let inner = inner.trim();
    if inner.is_empty() {
        return true;
    }

    // Flow-style YAML mappings (`key: {child: value}`) are valid and should
    // not be dropped by refinement; reject obvious CSS/JS-like shapes.
    inner.contains(':') && !inner.contains(';')
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
