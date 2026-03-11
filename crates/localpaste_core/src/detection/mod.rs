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
    if markdown_fence_override_applies(content) {
        return Some("markdown".to_string());
    }

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

#[derive(Clone, Copy)]
struct MarkdownFence {
    marker: char,
    len: usize,
}

fn markdown_fence_override_applies(content: &str) -> bool {
    if !crate::models::paste::is_markdown_content(content) {
        return false;
    }
    is_standalone_fenced_markdown_block(content)
}

fn is_standalone_fenced_markdown_block(content: &str) -> bool {
    let lines: Vec<&str> = content.lines().collect();
    let Some((start_idx, fence)) = lines.iter().enumerate().find_map(|(idx, line)| {
        (!line.trim().is_empty())
            .then(|| parse_markdown_fence_opener(line).map(|fence| (idx, fence)))
            .flatten()
    }) else {
        return false;
    };

    let Some(end_idx) = lines
        .iter()
        .enumerate()
        .skip(start_idx.saturating_add(1))
        .find_map(|(idx, line)| line_closes_markdown_fence(line, fence).then_some(idx))
    else {
        return false;
    };

    lines
        .iter()
        .skip(end_idx.saturating_add(1))
        .all(|line| line.trim().is_empty())
}

fn parse_markdown_fence_opener(line: &str) -> Option<MarkdownFence> {
    let trimmed_trailing = line.trim_end();
    let indent = trimmed_trailing.chars().take_while(|ch| *ch == ' ').count();
    if indent > 3 {
        return None;
    }
    let remainder = &trimmed_trailing[indent..];
    let marker = remainder.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let len = remainder.chars().take_while(|ch| *ch == marker).count();
    (len >= 3).then_some(MarkdownFence { marker, len })
}

fn line_closes_markdown_fence(line: &str, fence: MarkdownFence) -> bool {
    let trimmed_trailing = line.trim_end();
    let indent = trimmed_trailing.chars().take_while(|ch| *ch == ' ').count();
    if indent > 3 {
        return false;
    }
    let remainder = &trimmed_trailing[indent..];
    let marker_run = remainder
        .chars()
        .take_while(|ch| *ch == fence.marker)
        .count();
    marker_run >= fence.len
        && marker_run == remainder.chars().count()
        && remainder.chars().all(|ch| ch == fence.marker)
}

#[cfg(any(feature = "magika", test))]
fn refine_magika_label(label: &str, content: &str) -> Option<String> {
    if label.is_empty() || label == "text" {
        return None;
    }

    if markdown_fence_override_applies(content) {
        return Some("markdown".to_string());
    }

    if label == "yaml" && !looks_like_yaml(content) {
        return None;
    }

    if label == "scss" && looks_like_plain_css(content) {
        return Some("css".to_string());
    }

    Some(label.to_string())
}

/// Heuristically checks whether content resembles YAML mapping/sequence syntax.
///
/// # Returns
/// `true` when line-level patterns strongly indicate YAML.
pub(crate) fn looks_like_yaml(content: &str) -> bool {
    let mut yaml_pairs = 0usize;
    let mut bare_sequence_items = 0usize;
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
        if let Some(sequence_item) = trimmed.strip_prefix("- ") {
            if looks_like_yaml_sequence_item(sequence_item) {
                yaml_pairs = yaml_pairs.saturating_add(1);
            } else {
                bare_sequence_items = bare_sequence_items.saturating_add(1);
            }
            continue;
        }
        if trimmed.contains(':') && looks_like_single_line_yaml_mapping(trimmed, true) {
            // Block-style mapping heads like `jobs:` and `build:` are still
            // YAML pairs even when the nested value appears on later lines.
            yaml_pairs = yaml_pairs.saturating_add(1);
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
        return yaml_pairs >= 1 || bare_sequence_items >= 2;
    }

    false
}

fn looks_like_yaml_sequence_item(item: &str) -> bool {
    let trimmed = item.trim();
    if trimmed.is_empty() {
        return false;
    }
    if looks_like_single_line_yaml_mapping(trimmed, true) {
        return true;
    }
    if trimmed.starts_with('{') || trimmed.ends_with('}') {
        return looks_like_yaml_flow_mapping(trimmed);
    }
    if trimmed.starts_with('[') || trimmed.ends_with(']') {
        return looks_like_yaml_flow_sequence(trimmed);
    }
    false
}

fn looks_like_single_line_yaml_mapping(line: &str, allow_unquoted_space_keys: bool) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
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
    if !quoted_key && key.split_whitespace().count() > 3 {
        return false;
    }
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

#[cfg(any(feature = "magika", test))]
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

#[cfg(any(feature = "magika", test))]
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

#[cfg(any(feature = "magika", test))]
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

#[cfg(any(feature = "magika", test))]
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
