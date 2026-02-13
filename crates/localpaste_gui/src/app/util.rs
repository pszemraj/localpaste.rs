//! Small UI helpers for labels and word selection.

pub(super) fn env_flag_enabled(name: &str) -> bool {
    localpaste_core::config::env_flag_enabled(name)
}

/// Formats the language label shown in the UI, falling back to auto/plain.
pub(super) fn display_language_label(language: Option<&str>, is_large: bool) -> String {
    if is_large {
        return "plain".to_string();
    }
    let Some(raw) = language else {
        return "auto".to_string();
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "auto".to_string();
    }
    let lowered = trimmed.to_ascii_lowercase();
    match lowered.as_str() {
        "plaintext" | "plain" | "text" | "txt" => "plain".to_string(),
        _ => trimmed.to_string(),
    }
}

/// Chooses the status-bar language filter label.
///
/// Prefers the explicit filter value, then falls back to the selected paste
/// language so the footer reflects known language context.
pub(super) fn status_language_filter_label(
    active_filter: Option<&str>,
    selected_language: Option<&str>,
) -> String {
    active_filter
        .and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        })
        .or_else(|| {
            selected_language.and_then(|value| {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed)
                }
            })
        })
        .unwrap_or("Any")
        .to_string()
}

/// Formats clipboard/export content as a fenced code block.
pub(super) fn format_fenced_code_block(content: &str, language: Option<&str>) -> String {
    let lang = language.unwrap_or("text");
    format!("```{}\n{}\n```", lang, content)
}

fn is_word_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

/// Returns a word-selection range (in char indices) around the given char index.
pub(super) fn word_range_at(text: &str, char_index: usize) -> Option<(usize, usize)> {
    if text.is_empty() {
        return None;
    }
    let total_chars = text.chars().count();
    let target_char_index = if char_index >= total_chars {
        total_chars.saturating_sub(1)
    } else {
        char_index
    };
    let byte_index = text
        .char_indices()
        .nth(target_char_index)
        .map(|(idx, _)| idx)?;
    let mut iter = text[byte_index..].chars();
    let current = iter.next()?;
    let current_is_word = is_word_char(current);
    let mut start_byte = byte_index;
    let mut head = text[..byte_index].chars().rev();
    for ch in &mut head {
        if is_word_char(ch) == current_is_word {
            start_byte = start_byte.saturating_sub(ch.len_utf8());
        } else {
            break;
        }
    }
    let mut end_byte = byte_index + current.len_utf8();
    let mut tail = text[end_byte..].chars();
    for ch in &mut tail {
        if is_word_char(ch) == current_is_word {
            end_byte = end_byte.saturating_add(ch.len_utf8());
        } else {
            break;
        }
    }

    let start_char = text[..start_byte].chars().count();
    let selected_chars = text[start_byte..end_byte].chars().count();
    Some((start_char, start_char + selected_chars))
}

#[cfg(test)]
mod tests {
    use super::{format_fenced_code_block, status_language_filter_label};

    #[test]
    fn status_language_filter_label_prefers_active_filter() {
        assert_eq!(
            status_language_filter_label(Some("rust"), Some("python")),
            "rust"
        );
    }

    #[test]
    fn status_language_filter_label_falls_back_to_selected_language() {
        assert_eq!(status_language_filter_label(None, Some("python")), "python");
    }

    #[test]
    fn status_language_filter_label_uses_any_when_unknown() {
        assert_eq!(status_language_filter_label(None, None), "Any");
        assert_eq!(
            status_language_filter_label(Some("   "), Some("   ")),
            "Any"
        );
    }

    #[test]
    fn format_fenced_code_block_uses_language_or_text_default() {
        assert_eq!(
            format_fenced_code_block("let x = 1;", Some("rust")),
            "```rust\nlet x = 1;\n```"
        );
        assert_eq!(
            format_fenced_code_block("print('hi')", None),
            "```text\nprint('hi')\n```"
        );
    }
}
