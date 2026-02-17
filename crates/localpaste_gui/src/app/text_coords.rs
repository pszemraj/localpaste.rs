//! Shared text-coordinate helpers for editor buffers.

use ropey::Rope;

/// Clamp a global char index and return its containing line index.
pub(crate) fn line_for_char(rope: &Rope, char_index: usize) -> usize {
    rope.char_to_line(char_index.min(rope.len_chars()))
}

/// Returns a UTF-8 prefix containing at most `max_chars` unicode scalar values.
pub(crate) fn prefix_by_chars(text: &str, max_chars: usize) -> &str {
    match text.char_indices().nth(max_chars) {
        Some((byte_idx, _)) => &text[..byte_idx],
        None => text,
    }
}
