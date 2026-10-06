//! Language detection abstraction with Magika and heuristic fallback.

/// Language canonicalization and manual UI option tables.
pub mod canonical;
mod extensions;
pub use extensions::preferred_extension;
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
    if looks_like_rust_panic(content) {
        return Some("log".to_string());
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

    detect_heuristically(content)
}

/// Detect structural language hints without statistical inference or a model lock.
///
/// # Returns
/// A canonical language hint suitable for synchronous metadata derivation.
pub(crate) fn detect_heuristically(content: &str) -> Option<String> {
    heuristic::detect(content)
        .map(|label| canonical::canonicalize(&label))
        .filter(|label| !label.is_empty() && label != "text")
}

/// Recognize Rust's runtime panic header, rather than prose mentioning a panic.
///
/// # Returns
/// Whether the first nonempty line has the runtime's thread and source-location prefix.
pub(crate) fn looks_like_rust_panic(content: &str) -> bool {
    content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .is_some_and(|line| line.starts_with("thread '") && line.contains("' panicked at "))
}

#[derive(Clone, Copy)]
struct MarkdownFence {
    marker: char,
    len: usize,
}

fn markdown_fence_override_applies(content: &str) -> bool {
    standalone_fenced_block(content).is_some()
}

/// Extract the info word and body when the entire paste is one fenced block.
///
/// The borrowed body excludes fence delimiters; surrounding prose prevents a
/// match so real Markdown documents retain their document classification.
///
/// # Returns
/// The optional info word and body when matching fences surround the entire paste.
pub(crate) fn standalone_fenced_block(content: &str) -> Option<(&str, &str)> {
    let trimmed = content.trim_end();
    let mut lines = trimmed.split_inclusive('\n');
    let mut offset = 0;
    let first = loop {
        let line = lines.next()?;
        if !line.trim().is_empty() {
            break line;
        }
        offset += line.len();
    };
    let fence = parse_markdown_fence_opener(first)?;
    let opener = first.trim();
    let info = opener
        .get(fence.len..)?
        .split_whitespace()
        .next()
        .unwrap_or("");
    let body_start = offset + first.len();
    offset = body_start;
    for line in lines {
        if line_closes_markdown_fence(line, fence) {
            return trimmed
                .get(offset + line.len()..)?
                .trim()
                .is_empty()
                .then_some((info, trimmed.get(body_start..offset)?));
        }
        offset += line.len();
    }
    None
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

    if label == "yaml" && !looks_like_yaml(content) && !looks_like_flat_config_yaml(content) {
        return None;
    }

    if label == "scss" && looks_like_plain_css(content) {
        return Some("css".to_string());
    }

    if label == "gitattributes" && !looks_like_gitattributes(content) {
        return None;
    }

    // Magika uses either JSON label for both single values and record streams.
    // Establish the record format from multiple valid lines in the sample.
    if matches!(label, "json" | "jsonl") {
        return Some(if heuristic::looks_like_json_lines(content) {
            "jsonl".to_string()
        } else {
            "json".to_string()
        });
    }

    Some(label.to_string())
}

/// Require an attribute assignment or a path pattern before trusting Magika's
/// gitattributes label, which otherwise also matches short whitespace prose.
#[cfg(any(feature = "magika", test))]
fn looks_like_gitattributes(content: &str) -> bool {
    content.lines().take(512).any(|line| {
        let mut parts = line.split_whitespace();
        let Some(pattern) = parts.next() else {
            return false;
        };
        if pattern.starts_with('#') {
            return false;
        }
        let path_pattern = pattern.contains(['*', '?', '/', '.']);
        let attributes: Vec<_> = parts.collect();
        !attributes.is_empty()
            && attributes.iter().all(|attribute| {
                attribute.contains('=')
                    || attribute.starts_with(['-', '!'])
                    || matches!(
                        *attribute,
                        "text" | "binary" | "diff" | "merge" | "export-ignore"
                    )
                    || (path_pattern
                        && attribute
                            .chars()
                            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.')))
            })
    })
}

/// Heuristically checks whether content resembles YAML mapping/sequence syntax.
///
/// # Returns
/// `true` when line-level patterns strongly indicate YAML.
pub(crate) fn looks_like_yaml(content: &str) -> bool {
    let mut mapping_pairs = 0usize;
    let mut sequence_items = 0usize;
    let mut sequence_mapping_items = 0usize;
    let mut bare_sequence_items = 0usize;
    let mut has_doc_start = false;
    let mut first_meaningful_seen = false;
    let mut strong_structure = false;
    let mut open_mapping_head_indent: Option<usize> = None;
    let mut block_scalar_head_indent: Option<usize> = None;

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
        let indent = line.len().saturating_sub(line.trim_start().len());
        if block_scalar_head_indent.is_some_and(|head_indent| indent > head_indent) {
            strong_structure = true;
            block_scalar_head_indent = None;
            continue;
        }
        block_scalar_head_indent = None;
        if let Some(sequence_item) = trimmed.strip_prefix("- ") {
            sequence_items = sequence_items.saturating_add(1);
            if open_mapping_head_indent.is_some_and(|head_indent| indent > head_indent) {
                strong_structure = true;
            }
            if looks_like_yaml_sequence_item(sequence_item) {
                sequence_mapping_items = sequence_mapping_items.saturating_add(1);
                if yaml_sequence_item_has_distinctive_structure(sequence_item) {
                    strong_structure = true;
                }
            } else {
                bare_sequence_items = bare_sequence_items.saturating_add(1);
            }
            open_mapping_head_indent = None;
            continue;
        }
        if let Some(value) = yaml_mapping_value(trimmed, true) {
            // Block-style mapping heads like `jobs:` and `build:` are still
            // YAML pairs even when the nested value appears on later lines.
            mapping_pairs = mapping_pairs.saturating_add(1);
            if open_mapping_head_indent.is_some_and(|head_indent| indent > head_indent) {
                strong_structure = true;
            }
            let block_scalar_header = yaml_value_is_block_scalar_header(value);
            if !block_scalar_header && yaml_value_has_distinctive_structure(value) {
                strong_structure = true;
            }
            block_scalar_head_indent = block_scalar_header.then_some(indent);
            open_mapping_head_indent =
                (value.is_empty() || yaml_value_is_anchor_header(value)).then_some(indent);
        } else {
            open_mapping_head_indent = None;
        }
    }

    if has_doc_start {
        return mapping_pairs >= 1 || sequence_items >= 2;
    }

    strong_structure && (mapping_pairs >= 1 || sequence_items >= 1) || sequence_mapping_items >= 2
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

/// Heuristically checks whether content is a flat, config-shaped YAML mapping.
///
/// # Returns
/// `true` when at least two meaningful mapping lines use compact config keys.
pub(crate) fn looks_like_flat_config_yaml(content: &str) -> bool {
    let mut mapping_pairs = 0usize;
    let mut meaningful_lines = 0usize;

    for line in content.lines().take(512) {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed == "---" || trimmed == "..." {
            continue;
        }
        meaningful_lines = meaningful_lines.saturating_add(1);
        let Some((key, _value)) = trimmed.split_once(':') else {
            return false;
        };
        let key = key.trim();
        if !looks_like_single_line_yaml_mapping(trimmed, false)
            || !yaml_mapping_key_has_config_shape(key)
        {
            return false;
        }
        mapping_pairs = mapping_pairs.saturating_add(1);
    }

    meaningful_lines >= 2 && mapping_pairs >= 2
}

fn yaml_mapping_key_has_config_shape(key: &str) -> bool {
    let unquoted = key
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            key.strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(key);
    unquoted
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_lowercase() || ch == '_' || ch == '-')
}

fn yaml_sequence_item_has_distinctive_structure(item: &str) -> bool {
    let trimmed = item.trim();
    if let Some(value) = yaml_mapping_value(trimmed, true) {
        return yaml_value_has_distinctive_structure(value);
    }
    yaml_value_has_distinctive_structure(trimmed)
}

fn yaml_mapping_value(line: &str, allow_unquoted_space_keys: bool) -> Option<&str> {
    if !looks_like_single_line_yaml_mapping(line, allow_unquoted_space_keys) {
        return None;
    }
    line.split_once(':').map(|(_, value)| value.trim())
}

fn yaml_value_has_distinctive_structure(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.contains('{') || trimmed.contains('}') {
        return looks_like_yaml_flow_mapping(trimmed);
    }
    if trimmed.contains('[') || trimmed.contains(']') {
        return looks_like_yaml_flow_sequence(trimmed);
    }
    false
}

fn yaml_value_is_block_scalar_header(value: &str) -> bool {
    let trimmed = value.trim();
    let Some(indicator) = trimmed.chars().next() else {
        return false;
    };
    if indicator != '|' && indicator != '>' {
        return false;
    }
    let mut chomping_seen = false;
    let mut indent_seen = false;
    for ch in trimmed.chars().skip(1) {
        match ch {
            '+' | '-' if !chomping_seen => chomping_seen = true,
            '1'..='9' if !indent_seen => indent_seen = true,
            _ => return false,
        }
    }
    true
}

fn yaml_value_is_anchor_header(value: &str) -> bool {
    let trimmed = value.trim();
    let Some(rest) = trimmed.strip_prefix('&') else {
        return false;
    };
    !rest.is_empty()
        && rest
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
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
