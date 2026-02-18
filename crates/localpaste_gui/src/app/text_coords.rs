//! Shared text-coordinate helpers for editor buffers.

use ropey::Rope;

/// Clamp a global char index and return its containing line index.
///
/// # Arguments
/// - `rope`: Rope buffer to query.
/// - `char_index`: Global character index to clamp and map.
///
/// # Returns
/// Zero-based line index containing the clamped char position.
pub(crate) fn line_for_char(rope: &Rope, char_index: usize) -> usize {
    rope.char_to_line(char_index.min(rope.len_chars()))
}

/// Returns a UTF-8 prefix containing at most `max_chars` unicode scalar values.
///
/// # Arguments
/// - `text`: Source text to slice.
/// - `max_chars`: Maximum number of Unicode scalar values to keep.
///
/// # Returns
/// Borrowed prefix ending on a valid UTF-8 boundary.
///
/// # Panics
/// Panics only if internal char-boundary assumptions are violated.
pub(crate) fn prefix_by_chars(text: &str, max_chars: usize) -> &str {
    match text.char_indices().nth(max_chars) {
        Some((byte_idx, _)) => &text[..byte_idx],
        None => text,
    }
}
