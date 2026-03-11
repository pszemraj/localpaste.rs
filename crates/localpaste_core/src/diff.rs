//! Line-based diff helpers and request/response payloads.

use crate::{constants::MAX_DIFF_INPUT_BYTES, AppError};
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

const DIFF_CONTEXT_LINES: usize = 3;

/// Reject oversized diff requests before diff generation work begins.
///
/// # Arguments
/// - `left_bytes`: UTF-8 byte length of the left-hand source.
/// - `right_bytes`: UTF-8 byte length of the right-hand source.
///
/// # Returns
/// `Ok(())` when the combined input stays within the diff generation cap.
///
/// # Errors
/// Returns [`AppError::PayloadTooLarge`] when the combined input exceeds
/// [`MAX_DIFF_INPUT_BYTES`].
pub fn ensure_diff_input_within_limit(
    left_bytes: usize,
    right_bytes: usize,
) -> Result<(), AppError> {
    let total_bytes = left_bytes.saturating_add(right_bytes);
    if total_bytes > MAX_DIFF_INPUT_BYTES {
        return Err(AppError::PayloadTooLarge(format!(
            "Combined diff input exceeds maximum of {} bytes (left={}, right={})",
            MAX_DIFF_INPUT_BYTES, left_bytes, right_bytes
        )));
    }
    Ok(())
}

/// Build a compact unified line diff.
///
/// # Arguments
/// - `left`: Left-hand content.
/// - `right`: Right-hand content.
///
/// # Returns
/// A vector of rendered diff lines prefixed with `+`, `-`, ` `, or `@@`.
pub fn unified_diff_lines(left: &str, right: &str) -> Vec<String> {
    let diff = TextDiff::from_lines(left, right);
    let mut lines = Vec::new();

    for (group_index, group) in diff.grouped_ops(DIFF_CONTEXT_LINES).iter().enumerate() {
        if group_index > 0 {
            lines.push("@@".to_string());
        }

        for op in group {
            for change in diff.iter_changes(op) {
                let prefix = match change.tag() {
                    ChangeTag::Delete => '-',
                    ChangeTag::Insert => '+',
                    ChangeTag::Equal => ' ',
                };
                lines.push(format!("{prefix}{change}"));
            }
        }
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::{ensure_diff_input_within_limit, unified_diff_lines};
    use crate::{AppError, MAX_DIFF_INPUT_BYTES};

    #[test]
    fn unified_diff_lines_collapses_far_unchanged_regions_into_hunks() {
        let left = [
            "line-1", "line-2", "line-3", "line-4", "line-5", "line-6", "line-7", "line-8",
            "line-9", "line-10",
        ]
        .join("\n");
        let right = [
            "line-1",
            "line-2",
            "line-3",
            "line-4",
            "line-5 changed",
            "line-6",
            "line-7",
            "line-8",
            "line-9",
            "line-10",
        ]
        .join("\n");

        let lines = unified_diff_lines(left.as_str(), right.as_str());

        assert!(
            !lines.iter().any(|line| line == " line-1\n"),
            "far-away unchanged context should not be included"
        );
        assert!(
            !lines.iter().any(|line| line == " line-10"),
            "far-away unchanged context should not be included"
        );
        assert!(lines.iter().any(|line| line.contains("-line-5")));
        assert!(lines.iter().any(|line| line.contains("+line-5 changed")));
    }

    #[test]
    fn unified_diff_lines_marks_separate_hunks() {
        let left = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl\nm\n";
        let right = "a\nb changed\nc\nd\ne\nf\ng\nh\ni\nj\nk changed\nl\nm\n";

        let lines = unified_diff_lines(left, right);

        assert!(
            lines.iter().any(|line| line == "@@"),
            "distant edits should be separated by a hunk marker"
        );
    }

    #[test]
    fn ensure_diff_input_within_limit_rejects_oversized_requests() {
        let err = ensure_diff_input_within_limit(MAX_DIFF_INPUT_BYTES, 1)
            .expect_err("combined diff input should be rejected");

        assert!(
            matches!(err, AppError::PayloadTooLarge(ref message) if message.contains("Combined diff input exceeds")),
            "expected payload-too-large diff error, got {err:?}"
        );
    }
}
