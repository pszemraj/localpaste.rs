//! Line-based diff helpers and request/response payloads.

use crate::{constants::MAX_DIFF_INPUT_BYTES, AppError};
use serde::{Deserialize, Serialize};
use similar::{ChangeTag, TextDiff};
use std::time::Duration;

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

/// Wall-clock budget for computing one line diff.
///
/// Myers' algorithm costs `O((N + M) * D)`, so two large inputs with almost no
/// lines in common (`D` close to `N + M`) can take minutes even under the
/// input byte cap. The diff runs on the GUI backend worker thread, which also
/// handles saves, and inside an API request that CLI clients abandon after 30
/// seconds, so it must finish quickly. Ordinary edits (small `D`) diff in
/// milliseconds and never approach this budget; one second leaves wide margin
/// for them while keeping both callers responsive. When the budget runs out the
/// remaining unmatched region is emitted as a plain delete plus insert, which is
/// a correct but possibly non-minimal diff.
const DIFF_TIME_BUDGET: Duration = Duration::from_secs(1);

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
/// Computing the diff is bounded by a wall-clock budget of about one second.
/// Inputs that are very large and very different may get a correct but
/// non-minimal diff.
///
/// # Arguments
/// - `left`: Left-hand content.
/// - `right`: Right-hand content.
///
/// # Returns
/// A vector of rendered diff lines prefixed with `+`, `-`, ` `, or `@@`.
pub fn unified_diff_lines(left: &str, right: &str) -> Vec<String> {
    let diff = TextDiff::configure()
        .timeout(DIFF_TIME_BUDGET)
        .diff_lines(left, right);
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
    fn unified_diff_lines_stays_correct_for_large_fully_dissimilar_inputs() {
        // An unbounded Myers diff of these inputs takes minutes in a debug build; the
        // time budget makes it return a non-minimal-but-correct diff instead.
        let left: Vec<String> = (1..=30_000).map(|n| format!("alpha line {n}\n")).collect();
        let right: Vec<String> = (1..=30_000).map(|n| format!("beta row {n}\n")).collect();

        let lines = unified_diff_lines(left.concat().as_str(), right.concat().as_str());

        let removed: Vec<&str> = lines
            .iter()
            .filter_map(|line| line.strip_prefix('-'))
            .collect();
        let added: Vec<&str> = lines
            .iter()
            .filter_map(|line| line.strip_prefix('+'))
            .collect();
        assert_eq!(removed, left.iter().map(String::as_str).collect::<Vec<_>>());
        assert_eq!(added, right.iter().map(String::as_str).collect::<Vec<_>>());
        assert_eq!(
            lines.len(),
            left.len() + right.len(),
            "fully dissimilar inputs have no context or hunk-marker lines"
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
