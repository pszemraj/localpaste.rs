//! Similarity scoring and report rendering for AST duplicate checks.

use super::{jaccard, normalize_path, Args, FunctionInfo};
use std::cmp::Ordering;

const LENGTH_RATIO_FLOOR: f64 = 0.65;

#[derive(Debug)]
pub(super) struct SimilarityFinding {
    pub(super) left_id: usize,
    pub(super) right_id: usize,
    pub(super) score: f64,
    pub(super) overlap: usize,
    pub(super) union: usize,
}

pub(super) fn find_similarity_pairs(
    functions: &[FunctionInfo],
    args: &Args,
) -> (Vec<SimilarityFinding>, Vec<SimilarityFinding>) {
    let mut duplicates = Vec::new();
    let mut near_misses = Vec::new();
    let mut candidates: Vec<&FunctionInfo> = functions
        .iter()
        .filter(|info| {
            info.normalized_nodes.len() >= args.min_nodes
                && !info.shingles.is_empty()
                && (args.include_tests || (!info.is_test && !info.has_cfg))
        })
        .collect();
    candidates.sort_by_key(|info| info.normalized_nodes.len());

    for (i, left) in candidates.iter().enumerate() {
        let left = *left;
        let left_len = left.normalized_nodes.len() as f64;
        for right in candidates.iter().skip(i + 1) {
            let right = *right;
            let right_len = right.normalized_nodes.len() as f64;
            let len_ratio = left_len / right_len;
            if len_ratio < LENGTH_RATIO_FLOOR {
                // Candidates are sorted by length; later entries are only longer.
                break;
            }

            let (score, overlap, union) = jaccard(&left.shingles, &right.shingles);
            let finding = SimilarityFinding {
                left_id: left.id,
                right_id: right.id,
                score,
                overlap,
                union,
            };
            if score >= args.threshold {
                duplicates.push(finding);
            } else if score >= args.near_miss_threshold {
                near_misses.push(finding);
            }
        }
    }

    sort_similarity_findings(&mut duplicates);
    sort_similarity_findings(&mut near_misses);
    duplicates.truncate(args.top);
    near_misses.truncate(args.near_miss_top);
    (duplicates, near_misses)
}

pub(super) fn print_duplicate_findings(
    functions: &[FunctionInfo],
    findings: &[SimilarityFinding],
    args: &Args,
) {
    if findings.is_empty() {
        println!(
            "duplicate findings: none (threshold {:.2}, min_nodes {}, k {})",
            args.threshold, args.min_nodes, args.k
        );
        return;
    }

    println!(
        "duplicate findings (top {}, threshold {:.2}, min_nodes {}, k {}):",
        findings.len(),
        args.threshold,
        args.min_nodes,
        args.k
    );
    for finding in findings {
        let left = &functions[finding.left_id];
        let right = &functions[finding.right_id];
        println!(
            "  score {:.3} [{} / {}] {}:{} `{}` <-> {}:{} `{}`",
            finding.score,
            finding.overlap,
            finding.union,
            normalize_path(left.file.as_path()),
            left.line,
            left.symbol,
            normalize_path(right.file.as_path()),
            right.line,
            right.symbol
        );
    }
}

pub(super) fn print_near_miss_findings(
    functions: &[FunctionInfo],
    findings: &[SimilarityFinding],
    args: &Args,
) {
    if args.near_miss_threshold >= args.threshold {
        println!("near-miss findings: disabled");
        return;
    }
    if findings.is_empty() {
        println!(
            "near-miss findings: none (range {:.2}..{:.2})",
            args.near_miss_threshold, args.threshold
        );
        return;
    }

    println!(
        "near-miss findings (top {}, range {:.2}..{:.2}, possible false negatives):",
        findings.len(),
        args.near_miss_threshold,
        args.threshold
    );
    for finding in findings {
        let left = &functions[finding.left_id];
        let right = &functions[finding.right_id];
        println!(
            "  score {:.3} [{} / {}] {}:{} `{}` <-> {}:{} `{}`",
            finding.score,
            finding.overlap,
            finding.union,
            normalize_path(left.file.as_path()),
            left.line,
            left.symbol,
            normalize_path(right.file.as_path()),
            right.line,
            right.symbol
        );
    }
}

fn sort_similarity_findings(findings: &mut [SimilarityFinding]) {
    findings.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(Ordering::Equal)
            .then(right.overlap.cmp(&left.overlap))
            .then(left.left_id.cmp(&right.left_id))
            .then(left.right_id.cmp(&right.right_id))
    });
}
