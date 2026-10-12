//! Sidebar list model: the visible rows, and the date headers, cluster gaps,
//! and geometry laid out from them.

use super::clusters::relative_time_gaps;
use crate::backend::PasteSummary;
use chrono::{DateTime, Duration as ChronoDuration, Local, NaiveDate, TimeZone, Utc};
use eframe::egui;
use std::ops::{Deref, DerefMut, Range};
use std::rc::Rc;

/// Visible sidebar rows in display order, with the list layout built from them.
///
/// Building the layout visits every row, so it is kept until its inputs
/// change. Mutable access to the rows drops it and assigning new rows replaces
/// it, so no mutation can leave a stale layout behind; reads through `Deref`
/// keep it.
#[derive(Debug, Default)]
pub(in crate::app) struct SidebarRows {
    rows: Vec<PasteSummary>,
    /// Layout of `rows`, with the other inputs it was built for.
    layout: Option<(SidebarLayoutKey, Rc<SidebarList>)>,
}

/// Inputs of the sidebar list layout besides the rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SidebarLayoutKey {
    /// Whether the rows are search results, which get no headers or gaps.
    searched: bool,
    /// Local date the date headers are measured from.
    today: NaiveDate,
}

impl SidebarRows {
    /// Returns the list layout of these rows, building it only when the rows,
    /// the search state, or the local date changed since it was last built.
    ///
    /// # Arguments
    /// - `search_query`: Active sidebar query; non-blank means the rows are search results.
    /// - `now`: Local time that dates the headers.
    ///
    /// # Returns
    /// A shared handle to the layout, which callers may keep while mutating the app.
    pub(super) fn layout(&mut self, search_query: &str, now: DateTime<Local>) -> Rc<SidebarList> {
        let key = SidebarLayoutKey {
            searched: !search_query.trim().is_empty(),
            today: now.date_naive(),
        };
        if let Some((built_for, layout)) = &self.layout {
            if *built_for == key {
                return Rc::clone(layout);
            }
        }
        let layout = Rc::new(SidebarList::build(&self.rows, search_query, now));
        self.layout = Some((key, Rc::clone(&layout)));
        layout
    }

    /// The layout kept from the last [`Self::layout`] call.
    ///
    /// # Returns
    /// The kept layout, or `None` when a change to the rows has dropped it.
    #[cfg(test)]
    pub(in crate::app) fn cached_layout(&self) -> Option<&Rc<SidebarList>> {
        self.layout.as_ref().map(|(_, layout)| layout)
    }
}

impl Deref for SidebarRows {
    type Target = Vec<PasteSummary>;

    fn deref(&self) -> &Self::Target {
        &self.rows
    }
}

impl DerefMut for SidebarRows {
    /// Hands out the rows for mutation, dropping the layout built from them.
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.layout = None;
        &mut self.rows
    }
}

impl From<Vec<PasteSummary>> for SidebarRows {
    fn from(rows: Vec<PasteSummary>) -> Self {
        Self { rows, layout: None }
    }
}

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

/// Extra space, in points, above a row whose pause before it is as unusual as
/// pauses get; smaller pauses scale down from it.
pub(in crate::app) const MAX_CLUSTER_GAP: f32 = 12.0;

/// Vertical sizes the sidebar list is laid out with, in points.
#[derive(Debug, Clone, Copy)]
pub(super) struct SidebarListMetrics {
    /// Height of one header or row.
    pub(super) item_height: f32,
    /// Space between neighbouring items.
    pub(super) item_spacing: f32,
    /// Maximum extra space above a row separated from its neighbours in time.
    pub(super) cluster_gap: f32,
}

impl SidebarListMetrics {
    /// Metrics for the sidebar's style: items as tall as an interactive widget,
    /// the theme's item spacing between them, and up to [`MAX_CLUSTER_GAP`] of
    /// extra space above rows separated from their neighbours in time.
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
            cluster_gap: MAX_CLUSTER_GAP,
        }
    }
}

/// Items the sidebar list shows, top to bottom, with the cluster gaps between them.
///
/// Items share one height and spacing; the only irregularity is a cluster gap
/// above some rows. Keeping the cumulative gap weight per item makes any item's
/// position a closed-form expression, so the list finds the items in view
/// with a binary search instead of measuring every item above them.
#[derive(Debug)]
pub(in crate::app) struct SidebarList {
    items: Vec<SidebarListItem>,
    /// Cumulative extra spacing weight at or above each item.
    gaps_above: Vec<f32>,
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
    ///
    /// # Returns
    /// The list items with their cluster gaps.
    ///
    /// # Panics
    /// Date-group indexing requires nonempty groups, as guaranteed by `slice::chunk_by`.
    pub(super) fn build(pastes: &[PasteSummary], search_query: &str, now: DateTime<Local>) -> Self {
        if !search_query.trim().is_empty() {
            return Self {
                items: (0..pastes.len()).map(SidebarListItem::Row).collect(),
                gaps_above: vec![0.0; pastes.len()],
            };
        }

        let mut list = Self {
            items: Vec::with_capacity(pastes.len().saturating_add(4)),
            gaps_above: Vec::with_capacity(pastes.len().saturating_add(4)),
        };
        let mut gaps = 0.0;
        let mut row = 0;
        for group in pastes.chunk_by(|left, right| {
            sidebar_time_bucket(left.updated_at, now) == sidebar_time_bucket(right.updated_at, now)
        }) {
            list.items.push(SidebarListItem::Header(sidebar_time_bucket(
                group[0].updated_at,
                now,
            )));
            list.gaps_above.push(gaps);
            for weight in relative_time_gaps(group.iter().map(|paste| paste.updated_at)) {
                gaps += weight;
                list.items.push(SidebarListItem::Row(row));
                list.gaps_above.push(gaps);
                row += 1;
            }
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
            + self.gaps_above[index] * metrics.cluster_gap
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
        sidebar_time_bucket, SidebarList, SidebarListItem, SidebarListMetrics, SidebarRows,
        SidebarTimeBucket,
    };
    use chrono::{Duration, FixedOffset, Local, TimeZone, Utc};
    use eframe::egui;
    use std::rc::Rc;

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
        SidebarList::build(&pastes, "", now)
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
        let pastes = vec![
            summary("today", (now - Duration::hours(1)).with_timezone(&Utc)),
            summary("yesterday", (now - Duration::days(1)).with_timezone(&Utc)),
            summary("week", (now - Duration::days(3)).with_timezone(&Utc)),
            summary("earlier", (now - Duration::days(9)).with_timezone(&Utc)),
        ];

        assert_eq!(
            SidebarList::build(&pastes, "", now).items(),
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
            SidebarList::build(&pastes, "needle", now).items(),
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
            summary(
                "nearby",
                (now - Duration::days(14) - Duration::minutes(1)).with_timezone(&Utc),
            ),
            summary("old", (now - Duration::days(90)).with_timezone(&Utc)),
        ];
        let list = SidebarList::build(&pastes, "needle", now);
        assert!(cluster_starts(&list).is_empty());
        assert_eq!(list.content_height(METRICS), 3.0 * 34.0 + 2.0 * 8.0);
        // The same rows unsearched do break.
        let list = SidebarList::build(&pastes, " ", now);
        assert_eq!(cluster_starts(&list), [2]);
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
        assert_eq!(tops[..4], [0.0, 42.0, 84.0, 126.0]);
        assert!((tops[4] - 176.0).abs() < 0.01);
        assert!((list.content_height(METRICS) - (176.0 + 34.0)).abs() < 0.01);
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

    #[test]
    fn rows_keep_their_layout_until_the_rows_search_state_or_date_change() {
        let noon = Local.with_ymd_and_hms(2026, 6, 15, 12, 0, 0).unwrap();
        let tomorrow = noon + Duration::days(1);
        let mut rows = SidebarRows::from(vec![
            summary("new", (noon - Duration::hours(1)).with_timezone(&Utc)),
            summary("old", (noon - Duration::days(9)).with_timezone(&Utc)),
        ]);
        let built = rows.layout("", noon);

        // Reads, a later hour of the same day, and a blank query reuse it.
        assert_eq!((rows.len(), rows[0].id.as_str()), (2, "new"));
        assert!(Rc::ptr_eq(
            &built,
            &rows.layout(" ", noon + Duration::hours(11))
        ));

        // Searching drops the headers once; editing the query alone does not rebuild.
        let searched = rows.layout("needle", noon);
        assert!(!Rc::ptr_eq(&built, &searched));
        assert_eq!(
            searched.items(),
            [SidebarListItem::Row(0), SidebarListItem::Row(1)]
        );
        assert!(Rc::ptr_eq(&searched, &rows.layout("needles", noon)));

        // A new local day re-dates the headers.
        let next_day = rows.layout("", tomorrow);
        assert!(!Rc::ptr_eq(&searched, &next_day));
        assert_eq!(
            next_day.items()[0],
            SidebarListItem::Header(SidebarTimeBucket::Yesterday)
        );

        // Any mutable access drops the layout, and the next one shows the change.
        rows[0].updated_at = rows[1].updated_at;
        assert!(rows.cached_layout().is_none());
        assert_eq!(
            rows.layout("", tomorrow).items(),
            [
                SidebarListItem::Header(SidebarTimeBucket::Earlier),
                SidebarListItem::Row(0),
                SidebarListItem::Row(1),
            ]
        );
        rows.retain(|row| row.id == "old");
        assert!(rows.cached_layout().is_none());
        assert_eq!(rows.layout("", tomorrow).items().len(), 2);
    }
}
