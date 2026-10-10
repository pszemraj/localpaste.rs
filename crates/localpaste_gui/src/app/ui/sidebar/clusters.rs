//! Temporal clustering of sidebar rows: where a small gap separates neighbours.
//!
//! The unsearched sidebar lists pastes newest first by last change time.
//! Pastes changed in one sitting form a cluster, and a small gap separates
//! clusters. Two neighbouring rows, the newer changed at `a` and the older at
//! `b`, fall into different clusters when
//!
//! ```text
//! a - b >= max(MIN_CLUSTER_BREAK, CLUSTER_BREAK_RATIO * (now - a))
//! ```
//!
//! The time needed for a break grows with how long ago the newer row changed.
//! Pastes edited minutes apart this morning stay together, while yesterday's
//! paste sits apart from this morning's. Three weeks back, two pastes changed
//! a day apart still read as one cluster, and a paste from three months ago
//! sits apart from one changed two weeks ago. A fixed threshold cannot do
//! both: one small enough to split this morning from yesterday would put a gap
//! between almost every older paste, where changes are usually days apart.

use chrono::{DateTime, TimeDelta, Utc};

/// Smallest time between neighbouring rows that separates two clusters.
///
/// Rows changed less than this far apart always share a cluster, so a burst of
/// edits stays together however recent it is.
pub(super) const MIN_CLUSTER_BREAK: TimeDelta = TimeDelta::minutes(20);

/// Fraction of the newer row's age that the time to its older neighbour must reach to break.
pub(super) const CLUSTER_BREAK_RATIO: f64 = 0.5;

/// Whether a cluster break separates a row from the older row listed after it.
///
/// A newer row whose change time lies after `now`, as with clock skew, counts
/// as changed just now. Rows out of time order never break.
///
/// # Arguments
/// - `newer`: Change time of the row listed first.
/// - `older`: Change time of the row listed directly after it.
/// - `now`: Time the list was built, against which ages are measured.
///
/// # Returns
/// `true` when a gap belongs between the two rows.
pub(super) fn is_cluster_break(
    newer: DateTime<Utc>,
    older: DateTime<Utc>,
    now: DateTime<Utc>,
) -> bool {
    let between = newer.signed_duration_since(older);
    let age = now.signed_duration_since(newer).max(TimeDelta::zero());
    between >= MIN_CLUSTER_BREAK
        && between.num_milliseconds() as f64 >= CLUSTER_BREAK_RATIO * age.num_milliseconds() as f64
}

#[cfg(test)]
mod tests {
    use super::is_cluster_break;
    use chrono::{DateTime, Duration, TimeZone, Utc};

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 6, 15, 12, 0, 0).unwrap()
    }

    /// Whether rows changed `newer_age` and `older_age` before `now()` break.
    fn breaks(newer_age: Duration, older_age: Duration) -> bool {
        is_cluster_break(now() - newer_age, now() - older_age, now())
    }

    #[test]
    fn pastes_changed_minutes_apart_this_morning_share_a_cluster() {
        // Back-to-back edits a couple of hours ago.
        assert!(!breaks(
            Duration::hours(2),
            Duration::hours(2) + Duration::minutes(3)
        ));
        assert!(!breaks(
            Duration::hours(2) + Duration::minutes(3),
            Duration::hours(2) + Duration::minutes(9)
        ));
        // Just under the floor, even for a paste changed a moment ago.
        assert!(!breaks(Duration::minutes(1), Duration::minutes(20)));
    }

    #[test]
    fn yesterday_sits_apart_from_this_morning() {
        // 09:00 today against 17:00 yesterday, seen at noon.
        assert!(breaks(Duration::hours(3), Duration::hours(19)));
    }

    #[test]
    fn pastes_from_three_weeks_ago_changed_a_day_apart_share_a_cluster() {
        assert!(!breaks(Duration::days(21), Duration::days(22)));
    }

    #[test]
    fn three_months_ago_sits_apart_from_two_weeks_ago() {
        assert!(breaks(Duration::days(14), Duration::days(90)));
    }

    #[test]
    fn the_floor_and_the_age_rule_are_both_inclusive() {
        // Recent rows: the floor decides.
        assert!(breaks(Duration::minutes(1), Duration::minutes(21)));
        assert!(!breaks(
            Duration::minutes(1),
            Duration::minutes(21) - Duration::milliseconds(1)
        ));
        // Older rows: half the newer row's age decides.
        assert!(breaks(Duration::days(10), Duration::days(15)));
        assert!(!breaks(
            Duration::days(10),
            Duration::days(15) - Duration::milliseconds(1)
        ));
    }

    #[test]
    fn equal_change_times_never_break() {
        for age in [Duration::zero(), Duration::hours(5), Duration::days(400)] {
            assert!(!breaks(age, age), "{age}");
        }
    }

    #[test]
    fn a_newer_row_from_the_future_counts_as_changed_now() {
        // Clock skew: the newer row claims to have changed after the list was built.
        let ahead = -Duration::minutes(30);
        assert!(breaks(ahead, ahead + Duration::minutes(20)));
        assert!(!breaks(ahead, ahead + Duration::minutes(19)));
        // Both rows ahead of the clock still follow the floor.
        assert!(!breaks(
            -Duration::days(2),
            -Duration::days(2) + Duration::minutes(5)
        ));
    }

    #[test]
    fn rows_out_of_time_order_never_break() {
        assert!(!breaks(Duration::days(90), Duration::days(14)));
        assert!(!breaks(Duration::hours(19), Duration::hours(3)));
    }
}
