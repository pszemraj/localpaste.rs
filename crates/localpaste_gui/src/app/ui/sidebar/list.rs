//! Sidebar list model: date headers, paste rows, cluster gaps, and their geometry.

use super::clusters::is_cluster_break;
use crate::backend::PasteSummary;
use chrono::{DateTime, Duration as ChronoDuration, Local, TimeZone, Utc};
use eframe::egui;
use std::ops::Range;

/// Date group of the unsearched list, by the local calendar day of a paste's last change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SidebarTimeBucket {
    Today,
    Yesterday,
    ThisWeek,
    Earlier,
}

impl SidebarTimeBucket {
    /// Header text shown above the group.
    ///
    /// # Returns
    /// The group's display name.
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Today => "Today",
            Self::Yesterday => "Yesterday",
            Self::ThisWeek => "This Week",
            Self::Earlier => "Earlier",
        }
    }
}

/// One line of the sidebar list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SidebarListItem {
    /// Date header above the rows of one group.
    Header(SidebarTimeBucket),
    /// Paste row, by its index in the visible rows.
    Row(usize),
}

fn sidebar_time_bucket<Tz: TimeZone>(
    updated_at: DateTime<Utc>,
    now: DateTime<Tz>,
) -> SidebarTimeBucket {
    let timezone = now.timezone();
    let updated_day = updated_at.with_timezone(&timezone).date_naive();
    let today = now.date_naive();
    if updated_day == today {
        return SidebarTimeBucket::Today;
    }
    if updated_day == today - ChronoDuration::days(1) {
        return SidebarTimeBucket::Yesterday;
    }
    if updated_day >= today - ChronoDuration::days(7) {
        SidebarTimeBucket::ThisWeek
    } else {
        SidebarTimeBucket::Earlier
    }
}

/// Vertical sizes the sidebar list is laid out with, in points.
#[derive(Debug, Clone, Copy)]
pub(super) struct SidebarListMetrics {
    /// Height of one header or row.
    pub(super) item_height: f32,
    /// Space between neighbouring items.
    pub(super) item_spacing: f32,
    /// Extra space above a row that starts a new cluster.
    pub(super) cluster_gap: f32,
}

impl SidebarListMetrics {
    /// Metrics for the sidebar's style: items as tall as an interactive widget,
    /// the theme's item spacing between them, and that spacing once more above
    /// a row that starts a new cluster.
    ///
    /// # Arguments
    /// - `spacing`: Spacing of the UI that renders the list.
    ///
    /// # Returns
    /// The list metrics for that spacing.
    pub(super) fn from_spacing(spacing: &egui::style::Spacing) -> Self {
        Self {
            item_height: spacing.interact_size.y,
            item_spacing: spacing.item_spacing.y,
            cluster_gap: spacing.item_spacing.y,
        }
    }
}

/// Items the sidebar list shows, top to bottom, with the cluster gaps between them.
///
/// Items share one height and spacing; the only irregularity is a cluster gap
/// above some rows. Keeping the running gap count per item makes any item's
/// position a closed-form expression, so the list finds the items in view
/// with a binary search instead of measuring every item above them.
#[derive(Debug)]
pub(super) struct SidebarList {
    items: Vec<SidebarListItem>,
    /// Cluster gaps at or above each item; a row that starts a cluster counts its own gap.
    gaps_above: Vec<u32>,
}

impl SidebarList {
    /// Lays out `pastes` as the sidebar shows them.
    ///
    /// The unsearched list is newest first, so it gets date headers and
    /// cluster gaps (see [`super::clusters`]). A gap never sits next to a date
    /// header, which already separates its neighbours. Search results are in
    /// relevance order and get neither.
    ///
    /// # Arguments
    /// - `pastes`: Visible rows in display order.
    /// - `search_query`: Active sidebar query; non-blank means `pastes` are search results.
    /// - `now`: Local time that dates the headers.
    /// - `rows_built_at`: When `pastes` were built, against which cluster gaps measure ages.
    ///
    /// # Returns
    /// The list items with their cluster gaps.
    pub(super) fn build(
        pastes: &[PasteSummary],
        search_query: &str,
        now: DateTime<Local>,
        rows_built_at: DateTime<Utc>,
    ) -> Self {
        if !search_query.trim().is_empty() {
            return Self {
                items: (0..pastes.len()).map(SidebarListItem::Row).collect(),
                gaps_above: vec![0; pastes.len()],
            };
        }

        let mut list = Self {
            items: Vec::with_capacity(pastes.len().saturating_add(4)),
            gaps_above: Vec::with_capacity(pastes.len().saturating_add(4)),
        };
        let mut gaps = 0;
        let mut current_bucket = None;
        let mut previous: Option<&PasteSummary> = None;
        for (idx, paste) in pastes.iter().enumerate() {
            let bucket = sidebar_time_bucket(paste.updated_at, now);
            if current_bucket != Some(bucket) {
                list.items.push(SidebarListItem::Header(bucket));
                list.gaps_above.push(gaps);
                current_bucket = Some(bucket);
            } else if previous.is_some_and(|previous| {
                is_cluster_break(previous.updated_at, paste.updated_at, rows_built_at)
            }) {
                gaps += 1;
            }
            list.items.push(SidebarListItem::Row(idx));
            list.gaps_above.push(gaps);
            previous = Some(paste);
        }
        list
    }

    /// Items in display order.
    ///
    /// # Returns
    /// Headers and rows, top to bottom.
    pub(super) fn items(&self) -> &[SidebarListItem] {
        &self.items
    }

    /// Top of an item, measured from the top of the list.
    ///
    /// # Arguments
    /// - `index`: Item index in [`Self::items`].
    /// - `metrics`: Sizes the list is laid out with.
    ///
    /// # Returns
    /// The item's top edge in list coordinates.
    ///
    /// # Panics
    /// Panics if `index` is out of range.
    pub(super) fn item_top(&self, index: usize, metrics: SidebarListMetrics) -> f32 {
        index as f32 * (metrics.item_height + metrics.item_spacing)
            + self.gaps_above[index] as f32 * metrics.cluster_gap
    }

    /// Height of the whole list, from the top of its first item to the bottom of its last.
    ///
    /// # Returns
    /// The list height, or zero for an empty list.
    pub(super) fn content_height(&self, metrics: SidebarListMetrics) -> f32 {
        self.items.len().checked_sub(1).map_or(0.0, |last| {
            self.item_top(last, metrics) + metrics.item_height
        })
    }

    /// Indices of the items that overlap `viewport`, in list coordinates.
    ///
    /// Items are ordered by position, so both ends are binary searches and the
    /// cost does not grow with the number of items above the viewport.
    ///
    /// # Arguments
    /// - `viewport`: Vertical extent in view, in list coordinates.
    /// - `metrics`: Sizes the list is laid out with.
    ///
    /// # Returns
    /// The range of item indices to lay out; empty when none overlaps.
    pub(super) fn visible_range(
        &self,
        viewport: egui::Rangef,
        metrics: SidebarListMetrics,
    ) -> Range<usize> {
        let start = self.leading_items(|index| {
            self.item_top(index, metrics) + metrics.item_height <= viewport.min
        });
        let end = self.leading_items(|index| self.item_top(index, metrics) < viewport.max);
        start..end.max(start)
    }

    /// Number of leading items for which `pred` holds; it must hold for a prefix only.
    fn leading_items(&self, pred: impl Fn(usize) -> bool) -> usize {
        let (mut low, mut high) = (0, self.items.len());
        while low < high {
            let mid = low + (high - low) / 2;
            if pred(mid) {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        low
    }
}

#[cfg(test)]
mod tests {
    use super::{
        sidebar_time_bucket, SidebarList, SidebarListItem, SidebarListMetrics, SidebarTimeBucket,
    };
    use chrono::{Duration, FixedOffset, Local, TimeZone, Utc};
    use eframe::egui;

    fn summary(id: &str, updated_at: chrono::DateTime<Utc>) -> crate::backend::PasteSummary {
        crate::backend::PasteSummary {
            id: id.to_string(),
            name: id.to_string(),
            language: None,
            content_len: 0,
            updated_at,
            folder_id: None,
            tags: Vec::new(),
            derived: localpaste_core::semantic::DerivedMeta::default(),
            match_excerpt: None,
        }
    }

    /// Unsearched list of rows changed `ages` before a local noon, built at that noon.
    fn unsearched(ages: &[Duration]) -> SidebarList {
        let now = Local.with_ymd_and_hms(2026, 6, 15, 12, 0, 0).unwrap();
        let pastes: Vec<_> = ages
            .iter()
            .enumerate()
            .map(|(idx, age)| summary(&format!("row-{idx}"), (now - *age).with_timezone(&Utc)))
            .collect();
        SidebarList::build(&pastes, "", now, now.with_timezone(&Utc))
    }

    /// Row indices that start a new cluster, in display order.
    fn cluster_starts(list: &SidebarList) -> Vec<usize> {
        list.items()
            .iter()
            .enumerate()
            .filter_map(|(item_idx, item)| match item {
                SidebarListItem::Row(row)
                    if item_idx > 0
                        && list.gaps_above[item_idx] > list.gaps_above[item_idx - 1] =>
                {
                    Some(*row)
                }
                _ => None,
            })
            .collect()
    }

    const METRICS: SidebarListMetrics = SidebarListMetrics {
        item_height: 34.0,
        item_spacing: 8.0,
        cluster_gap: 8.0,
    };

    #[test]
    fn sidebar_time_bucket_classifies_recency_boundaries() {
        let now = Local.with_ymd_and_hms(2026, 6, 15, 12, 0, 0).unwrap();
        assert_eq!(
            sidebar_time_bucket((now - Duration::hours(2)).with_timezone(&Utc), now),
            SidebarTimeBucket::Today
        );
        assert_eq!(
            sidebar_time_bucket((now - Duration::days(1)).with_timezone(&Utc), now),
            SidebarTimeBucket::Yesterday
        );
        assert_eq!(
            sidebar_time_bucket((now - Duration::days(3)).with_timezone(&Utc), now),
            SidebarTimeBucket::ThisWeek
        );
        assert_eq!(
            sidebar_time_bucket((now - Duration::days(9)).with_timezone(&Utc), now),
            SidebarTimeBucket::Earlier
        );
    }

    #[test]
    fn sidebar_time_bucket_uses_local_calendar_boundaries() {
        let east = FixedOffset::east_opt(14 * 60 * 60).unwrap();
        let west = FixedOffset::west_opt(10 * 60 * 60).unwrap();
        let this_week_now = east.with_ymd_and_hms(2026, 6, 15, 12, 0, 0).unwrap();
        let yesterday_now = west.with_ymd_and_hms(2026, 6, 15, 0, 30, 0).unwrap();
        let cases = [
            (
                east.with_ymd_and_hms(2026, 6, 8, 0, 1, 0).unwrap(),
                this_week_now,
                SidebarTimeBucket::ThisWeek,
            ),
            (
                east.with_ymd_and_hms(2026, 6, 7, 23, 59, 59).unwrap(),
                this_week_now,
                SidebarTimeBucket::Earlier,
            ),
            (
                west.with_ymd_and_hms(2026, 6, 14, 23, 59, 0).unwrap(),
                yesterday_now,
                SidebarTimeBucket::Yesterday,
            ),
        ];

        for (updated_at, now, expected) in cases {
            assert_eq!(
                sidebar_time_bucket(updated_at.with_timezone(&Utc), now),
                expected
            );
        }
    }

    #[test]
    fn sidebar_list_items_matrix_covers_headers_and_search_density() {
        let now = Local.with_ymd_and_hms(2026, 6, 15, 12, 0, 0).unwrap();
        let built_at = now.with_timezone(&Utc);
        let pastes = vec![
            summary("today", (now - Duration::hours(1)).with_timezone(&Utc)),
            summary("yesterday", (now - Duration::days(1)).with_timezone(&Utc)),
            summary("week", (now - Duration::days(3)).with_timezone(&Utc)),
            summary("earlier", (now - Duration::days(9)).with_timezone(&Utc)),
        ];

        assert_eq!(
            SidebarList::build(&pastes, "", now, built_at).items(),
            [
                SidebarListItem::Header(SidebarTimeBucket::Today),
                SidebarListItem::Row(0),
                SidebarListItem::Header(SidebarTimeBucket::Yesterday),
                SidebarListItem::Row(1),
                SidebarListItem::Header(SidebarTimeBucket::ThisWeek),
                SidebarListItem::Row(2),
                SidebarListItem::Header(SidebarTimeBucket::Earlier),
                SidebarListItem::Row(3),
            ]
        );
        assert_eq!(
            SidebarList::build(&pastes, "needle", now, built_at).items(),
            [
                SidebarListItem::Row(0),
                SidebarListItem::Row(1),
                SidebarListItem::Row(2),
                SidebarListItem::Row(3),
            ]
        );
    }

    #[test]
    fn cluster_gaps_split_rows_within_a_date_group_but_never_beside_a_header() {
        let list = unsearched(&[
            // Today: three back-to-back edits, then one from early this morning.
            Duration::minutes(1),
            Duration::minutes(2),
            Duration::minutes(3),
            Duration::hours(4),
            // Yesterday: the header separates it from today, so no gap as well.
            Duration::hours(19),
            Duration::hours(19) + Duration::minutes(5),
            // Earlier: a day apart a month ago stays together; months older breaks.
            Duration::days(30),
            Duration::days(31),
            Duration::days(200),
        ]);
        assert_eq!(
            list.items(),
            [
                SidebarListItem::Header(SidebarTimeBucket::Today),
                SidebarListItem::Row(0),
                SidebarListItem::Row(1),
                SidebarListItem::Row(2),
                SidebarListItem::Row(3),
                SidebarListItem::Header(SidebarTimeBucket::Yesterday),
                SidebarListItem::Row(4),
                SidebarListItem::Row(5),
                SidebarListItem::Header(SidebarTimeBucket::Earlier),
                SidebarListItem::Row(6),
                SidebarListItem::Row(7),
                SidebarListItem::Row(8),
            ]
        );
        assert_eq!(cluster_starts(&list), [3, 8]);
    }

    #[test]
    fn search_results_get_no_cluster_gaps() {
        let now = Local.with_ymd_and_hms(2026, 6, 15, 12, 0, 0).unwrap();
        let pastes = vec![
            summary("recent", (now - Duration::days(14)).with_timezone(&Utc)),
            summary("old", (now - Duration::days(90)).with_timezone(&Utc)),
        ];
        let list = SidebarList::build(&pastes, "needle", now, now.with_timezone(&Utc));
        assert!(cluster_starts(&list).is_empty());
        assert_eq!(list.content_height(METRICS), 2.0 * 34.0 + 8.0);
        // The same rows unsearched do break.
        let list = SidebarList::build(&pastes, " ", now, now.with_timezone(&Utc));
        assert_eq!(cluster_starts(&list), [1]);
    }

    #[test]
    fn empty_and_single_row_lists_have_no_gaps() {
        let empty = unsearched(&[]);
        assert!(empty.items().is_empty());
        assert_eq!(empty.content_height(METRICS), 0.0);
        assert_eq!(
            empty.visible_range(egui::Rangef::new(0.0, 500.0), METRICS),
            0..0
        );

        let single = unsearched(&[Duration::days(400)]);
        assert_eq!(
            single.items(),
            [
                SidebarListItem::Header(SidebarTimeBucket::Earlier),
                SidebarListItem::Row(0),
            ]
        );
        assert!(cluster_starts(&single).is_empty());
        assert_eq!(single.content_height(METRICS), 2.0 * 34.0 + 8.0);
    }

    #[test]
    fn gaps_shift_every_item_below_them() {
        // Header, three rows, then a row that starts a new cluster.
        let list = unsearched(&[
            Duration::days(30),
            Duration::days(30) + Duration::minutes(1),
            Duration::days(30) + Duration::minutes(2),
            Duration::days(90),
        ]);
        assert_eq!(cluster_starts(&list), [3]);
        let tops: Vec<f32> = (0..list.items().len())
            .map(|index| list.item_top(index, METRICS))
            .collect();
        assert_eq!(tops, [0.0, 42.0, 84.0, 126.0, 176.0]);
        assert_eq!(list.content_height(METRICS), 176.0 + 34.0);
    }

    #[test]
    fn visible_range_covers_exactly_the_items_overlapping_the_viewport() {
        // Tops: 0, 42, 84, 126, then 176 after the gap.
        let list = unsearched(&[
            Duration::days(30),
            Duration::days(30) + Duration::minutes(1),
            Duration::days(30) + Duration::minutes(2),
            Duration::days(90),
        ]);
        let range = |min: f32, max: f32| list.visible_range(egui::Rangef::new(min, max), METRICS);
        assert_eq!(range(0.0, 100.0), 0..3);
        assert_eq!(range(0.0, 1000.0), 0..5);
        // A viewport that starts inside an item still shows it.
        assert_eq!(range(150.0, 200.0), 3..5);
        // A viewport inside the spacing and gap between two rows shows nothing.
        assert_eq!(range(161.0, 175.0), 4..4);
        assert_eq!(range(500.0, 600.0), 5..5);
    }
}
