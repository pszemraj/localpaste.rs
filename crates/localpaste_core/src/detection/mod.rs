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
    let mut yaml_pairs = 0usize;
    let mut content_lines = 0usize;
    let mut first_content_line: Option<&str> = None;
    let mut has_doc_start = false;
    let mut first_meaningful_seen = false;

    for line in content.lines().take(512) {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if !first_meaningful_seen {
            first_meaningful_seen = true;
            if trimmed == "---" {
                has_doc_start = true;
                continue;
            }
        }
        content_lines = content_lines.saturating_add(1);
        if first_content_line.is_none() {
            first_content_line = Some(trimmed);
        }
        if trimmed.starts_with("- ")
            || trimmed.contains(": ")
            || (trimmed.ends_with(':') && trimmed.len() > 1)
        {
            let yaml_like = if trimmed.ends_with(':') && trimmed.len() > 1 {
                true
            } else {
                looks_like_single_line_yaml_mapping(trimmed, true)
            };
            if yaml_like {
                yaml_pairs = yaml_pairs.saturating_add(1);
            }
        }
    }

    if yaml_pairs >= 2 {
        return true;
    }

    if yaml_pairs == 1 && content_lines == 1 {
        return first_content_line
            .map(|line| !line.starts_with("- ") && looks_like_single_line_yaml_mapping(line, false))
            .unwrap_or(false);
    }

    if has_doc_start {
        return yaml_pairs >= 1;
    }

    false
}

fn looks_like_single_line_yaml_mapping(line: &str, allow_unquoted_space_keys: bool) -> bool {
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
    let quoted_key = (key.starts_with('"') && key.ends_with('"'))
        || (key.starts_with('\'') && key.ends_with('\''));
    if key.contains(char::is_whitespace) && !allow_unquoted_space_keys && !quoted_key {
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
        return looks_like_yaml_flow_sequence(value);
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

fn looks_like_yaml_flow_sequence(value: &str) -> bool {
    let trimmed = value.trim();
    if !trimmed.starts_with('[') || !trimmed.ends_with(']') {
        return false;
    }
    if trimmed.len() < 2 {
        return false;
    }
    if trimmed.contains(';') {
        return false;
    }

    true
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
        || lower.contains("#{")
        || content_has_scss_placeholder_selector(content);
    let has_nested_scss_selector = content_has_nested_scss_selector(&lower);

    // SCSS blocks can look like CSS but still include nested rules (for example
    // `.parent { .child { ... } }` or `.button { &:hover { ... } }`).
    has_css_block && !(has_scss_specific_tokens || has_nested_scss_selector)
}

#[cfg(feature = "magika")]
fn content_has_scss_placeholder_selector(content: &str) -> bool {
    for line in content.lines() {
        let Some((selectors, _rest)) = line.split_once('{') else {
            continue;
        };
        for selector in selectors.split(',') {
            if selector.trim_start().starts_with('%') {
                return true;
            }
        }
    }
    false
}

#[cfg(feature = "magika")]
fn content_has_nested_scss_selector(content: &str) -> bool {
    let mut block_depth = 0usize;
    for line in content.lines() {
        let mut idx = 0usize;
        while idx < line.len() {
            let ch = line.as_bytes()[idx];
            if ch == b'{' {
                let selector = line[..idx].trim();
                if block_depth > 0 && appears_nested_scss_selector(selector) {
                    return true;
                }
                block_depth = block_depth.saturating_add(1);
            } else if ch == b'}' {
                block_depth = block_depth.saturating_sub(1);
            }
            idx += 1;
        }
    }

    false
}

#[cfg(feature = "magika")]
fn appears_nested_scss_selector(selector: &str) -> bool {
    let selector = selector.trim().trim_end_matches(',');
    if selector.is_empty() || selector.ends_with(';') {
        return false;
    }
    if selector.starts_with('@') {
        return false;
    }
    if selector == "from" || selector == "to" {
        return false;
    }

    selector.starts_with('&')
        || selector.starts_with('.')
        || selector.starts_with('#')
        || selector.starts_with(':')
        || selector.starts_with('>')
        || selector.starts_with('+')
        || selector.starts_with('~')
        || selector.starts_with('[')
        || selector.starts_with('*')
        || selector.contains('&')
        || selector.chars().any(|c| c.is_ascii_alphabetic())
}

/// Initialize the Magika model session early when available.
pub fn prewarm() {
    #[cfg(feature = "magika")]
    {
        magika::prewarm();
    }
}
