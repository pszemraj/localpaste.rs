//! Shared line-reuse helpers for highlight caches and worker passes.

use syntect::highlighting::HighlightState;
use syntect::parsing::ParseState;

const FNV_OFFSET: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x00000100000001B3;

fn hash_bytes_step(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Checks whether parser/highlighter state at line start matches cached state.
///
/// # Arguments
/// - `idx`: Line index being evaluated.
/// - `prev_line_reused`: Whether the previous line was reused without recompute.
/// - `old_lines`: Optional cached line values aligned to current line indices.
/// - `parse_state`: Current parse state before processing this line.
/// - `highlight_state`: Current highlight state before processing this line.
/// - `default_state`: `(parse, highlight)` state expected at the first line.
/// - `end_state_for`: Extractor for a line's ending `(parse, highlight)` state.
///
/// # Returns
/// `true` when line-level reuse is safe for the current state boundary.
pub(crate) fn line_start_state_matches<T, F>(
    idx: usize,
    prev_line_reused: bool,
    old_lines: &[Option<T>],
    parse_state: &ParseState,
    highlight_state: &HighlightState,
    default_state: (&ParseState, &HighlightState),
    end_state_for: F,
) -> bool
where
    F: Fn(&T) -> (&ParseState, &HighlightState),
{
    if idx == 0 {
        return *default_state.0 == *parse_state && *default_state.1 == *highlight_state;
    }
    if prev_line_reused {
        return true;
    }
    old_lines
        .get(idx - 1)
        .and_then(|line| line.as_ref())
        .map(|line| {
            let (end_parse, end_highlight) = end_state_for(line);
            *end_parse == *parse_state && *end_highlight == *highlight_state
        })
        .unwrap_or(false)
}

/// Checks whether an aligned cached line hash matches expected hash.
///
/// # Arguments
/// - `old_lines`: Optional cached line values aligned to current line indices.
/// - `idx`: Line index being evaluated.
/// - `expected_hash`: Hash for the current source line.
/// - `hash_for`: Hash extractor for cached line values.
///
/// # Returns
/// `true` when a cached line exists at `idx` and hash values match.
pub(crate) fn line_hash_matches<T, F>(
    old_lines: &[Option<T>],
    idx: usize,
    expected_hash: u64,
    hash_for: F,
) -> bool
where
    F: Fn(&T) -> u64,
{
    old_lines
        .get(idx)
        .and_then(|line| line.as_ref())
        .map(|line| hash_for(line) == expected_hash)
        .unwrap_or(false)
}

/// Computes a stable FNV-1a hash for a byte slice.
///
/// # Returns
/// 64-bit hash value used for highlight-line reuse checks.
pub(crate) fn hash_bytes(bytes: &[u8]) -> u64 {
    hash_bytes_step(FNV_OFFSET, bytes)
}

/// Aligns previous cached lines to a new sequence by matching prefix/suffix hashes.
///
/// # Arguments
/// - `old_lines`: Previously cached lines in original order.
/// - `new_hashes`: Hashes for the latest line sequence.
/// - `hash_for`: Hash extractor for `T`.
///
/// # Returns
/// Vec aligned to `new_hashes`, with reusable entries in prefix/suffix slots.
///
/// # Panics
/// Panics if internal prefix/suffix alignment invariants are violated.
pub(crate) fn align_old_lines_by_hash<T, F>(
    old_lines: Vec<T>,
    new_hashes: &[u64],
    hash_for: F,
) -> Vec<Option<T>>
where
    F: Fn(&T) -> u64,
{
    let old_len = old_lines.len();
    let new_len = new_hashes.len();
    if new_len == 0 {
        return Vec::new();
    }
    if old_len == 0 {
        let mut out = Vec::with_capacity(new_len);
        out.resize_with(new_len, || None);
        return out;
    }

    let mut old: Vec<Option<T>> = old_lines.into_iter().map(Some).collect();

    let mut prefix = 0usize;
    while prefix < old_len && prefix < new_len {
        let Some(ref line) = old[prefix] else {
            break;
        };
        if hash_for(line) == new_hashes[prefix] {
            prefix += 1;
        } else {
            break;
        }
    }

    let mut suffix = 0usize;
    while suffix < (old_len - prefix) && suffix < (new_len - prefix) {
        let old_idx = old_len - 1 - suffix;
        let new_idx = new_len - 1 - suffix;
        let Some(ref line) = old[old_idx] else {
            break;
        };
        if hash_for(line) == new_hashes[new_idx] {
            suffix += 1;
        } else {
            break;
        }
    }

    let mut aligned = Vec::with_capacity(new_len);
    aligned.resize_with(new_len, || None);
    for i in 0..prefix {
        aligned[i] = old[i].take();
    }
    for j in 0..suffix {
        let new_idx = new_len - suffix + j;
        let old_idx = old_len - suffix + j;
        aligned[new_idx] = old[old_idx].take();
    }
    aligned
}
