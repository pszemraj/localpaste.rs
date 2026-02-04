//! Small UI helpers for labels and word selection.

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
    while let Some(ch) = head.next() {
        if is_word_char(ch) == current_is_word {
            start_byte = start_byte.saturating_sub(ch.len_utf8());
        } else {
            break;
        }
    }
    let mut end_byte = byte_index + current.len_utf8();
    let mut tail = text[end_byte..].chars();
    while let Some(ch) = tail.next() {
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
