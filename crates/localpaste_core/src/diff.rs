//! Line-based diff helpers and request/response payloads.

use serde::{Deserialize, Serialize};
use similar::{ChangeTag, TextDiff};

/// Reference to a concrete paste snapshot (head or historical version).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiffRef {
    pub paste_id: String,
    pub version_id_ms: Option<u64>,
}

/// Request payload for comparing two paste snapshots.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiffRequest {
    pub left: DiffRef,
    pub right: DiffRef,
}

/// Diff response payload returned by API/CLI surfaces.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiffResponse {
    pub equal: bool,
    pub unified: Vec<String>,
}

/// Equality response payload for boolean-only compare flows.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EqualResponse {
    pub equal: bool,
}

/// Build a compact unified line diff.
///
/// # Arguments
/// - `left`: Left-hand content.
/// - `right`: Right-hand content.
///
/// # Returns
/// A vector of rendered diff lines prefixed with `+`, `-`, or ` `.
pub fn unified_diff_lines(left: &str, right: &str) -> Vec<String> {
    let diff = TextDiff::from_lines(left, right);
    diff.iter_all_changes()
        .map(|change| {
            let prefix = match change.tag() {
                ChangeTag::Delete => '-',
                ChangeTag::Insert => '+',
                ChangeTag::Equal => ' ',
            };
            format!("{prefix}{change}")
        })
        .collect()
}
