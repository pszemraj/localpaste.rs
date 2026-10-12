//! Relative temporal spacing within a sidebar date group.
//!
//! The median gap between neighbouring modification times describes the usual
//! pace of that group's edits. Gaps no larger than that stay compact. Larger
//! gaps receive progressively more whitespace, proportional to their excess
//! over the median: `(gap - median) / (gap + median)`. This fraction approaches
//! one for widely separated sessions, without an absolute time cutoff or any
//! dependence on how old the pastes are.

use chrono::{DateTime, Utc};

/// Relative extra space above each row, between zero and one.
///
/// Each date group is measured separately. The first row has no extra space;
/// equal timestamps and out-of-order neighbours contribute zero elapsed time.
/// Uniformly spaced rows stay uniform, and translating or scaling all times
/// leaves the relative spacing unchanged.
///
/// # Arguments
/// - `timestamps`: Modification times in newest-first display order.
///
/// # Returns
/// One spacing weight per timestamp, with zero above the first row.
///
/// # Panics
/// Median indexing requires a nonempty gap slice, ensured before it is indexed.
pub(super) fn relative_time_gaps(mut timestamps: impl Iterator<Item = DateTime<Utc>>) -> Vec<f32> {
    let Some(mut previous) = timestamps.next() else {
        return Vec::new();
    };
    let gaps: Vec<f64> = timestamps
        .map(|timestamp| {
            let gap = previous
                .signed_duration_since(timestamp)
                .num_milliseconds()
                .max(0) as f64;
            previous = timestamp;
            gap
        })
        .collect();
    if gaps.is_empty() {
        return vec![0.0];
    }
    let mut ordered = gaps.clone();
    ordered.sort_unstable_by(f64::total_cmp);
    let middle = ordered.len() / 2;
    let median = if ordered.len().is_multiple_of(2) {
        (ordered[middle - 1] + ordered[middle]) / 2.0
    } else {
        ordered[middle]
    };
    std::iter::once(0.0)
        .chain(gaps.into_iter().map(|gap| {
            if gap > median {
                ((gap - median) / (gap + median)) as f32
            } else {
                0.0
            }
        }))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::relative_time_gaps;
    use chrono::{Duration, TimeZone, Utc};

    fn weights(ages: &[i64], scale: i64, offset: i64) -> Vec<f32> {
        let now = Utc.with_ymd_and_hms(2026, 6, 15, 12, 0, 0).unwrap();
        relative_time_gaps(
            ages.iter()
                .map(|age| now - Duration::milliseconds(age * scale + offset)),
        )
    }

    #[test]
    fn a_pause_between_bursts_gets_space_at_any_time_scale_or_age() {
        let ages = [0, 1, 2, 100, 101, 102];
        let expected = weights(&ages, 1, 0);
        assert_eq!(expected[..3], [0.0; 3]);
        assert!(expected[3] > 0.9);
        assert_eq!(expected[4..], [0.0; 2]);
        for scale in [1, 60_000, 86_400_000] {
            for offset in [0, 90 * 86_400_000] {
                assert_eq!(weights(&ages, scale, offset), expected);
            }
        }
    }

    #[test]
    fn increasingly_unusual_pauses_get_increasing_space() {
        // Consecutive gaps: 1, 2, 4, 8, 16; the observed median is 4.
        let gaps = weights(&[0, 1, 3, 7, 15, 31], 1, 0);
        assert_eq!(gaps[..4], [0.0; 4]);
        assert!(gaps[4] > 0.0 && gaps[5] > gaps[4] && gaps[5] < 1.0);
    }

    #[test]
    fn a_very_old_outlier_does_not_hide_other_separate_sessions() {
        let gaps = weights(&[0, 1, 2, 2880, 2881, 2882, 1_000_000], 60_000, 0);
        assert!(gaps[3] > 0.9 && gaps[6] > gaps[3]);
        assert_eq!(gaps[4..6], [0.0; 2]);
    }

    #[test]
    fn repeated_timestamps_keep_bursts_compact_without_hiding_the_pause() {
        assert_eq!(
            weights(&[0, 0, 0, 100, 100, 100], 1, 0),
            [0.0, 0.0, 0.0, 1.0, 0.0, 0.0]
        );
    }

    #[test]
    fn uniform_rows_and_lists_without_comparative_gaps_stay_compact() {
        for ages in [&[][..], &[0], &[0, 100], &[0, 10, 20, 30], &[0, 0, 0]] {
            assert_eq!(weights(ages, 1, 0), vec![0.0; ages.len()]);
        }
    }

    #[test]
    fn clock_skew_does_not_create_negative_or_nonfinite_spacing() {
        let gaps = weights(&[0, -10, -10, 10, 11], 1, 0);
        assert_eq!(gaps[..3], [0.0; 3]);
        assert!(gaps[3] > gaps[4]);
        assert!(gaps
            .iter()
            .all(|gap| gap.is_finite() && (0.0..=1.0).contains(gap)));
    }
}
