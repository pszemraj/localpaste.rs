//! Time-sensitive sidebar collection filter tests.

use super::*;
use chrono::TimeZone;

#[test]
fn week_collection_uses_local_calendar_cutoff_day() {
    let today = chrono::NaiveDate::from_ymd_opt(2026, 6, 15).unwrap();
    let week_cutoff_day = today - chrono::Duration::days(7);
    let recent_cutoff = chrono::Utc.with_ymd_and_hms(2026, 5, 1, 0, 0, 0).unwrap();
    let cutoff_day = chrono::Local
        .with_ymd_and_hms(2026, 6, 8, 0, 1, 0)
        .unwrap()
        .with_timezone(&chrono::Utc);
    let before_cutoff_day = chrono::Local
        .with_ymd_and_hms(2026, 6, 7, 23, 59, 59)
        .unwrap()
        .with_timezone(&chrono::Utc);
    let item = |id: &str, updated_at| PasteSummary {
        id: id.to_string(),
        name: id.to_string(),
        language: None,
        content_len: 0,
        updated_at,
        folder_id: None,
        tags: Vec::new(),
        derived: Default::default(),
    };

    assert!(LocalPasteApp::matches_active_filters(
        &item("cutoff", cutoff_day),
        &SidebarCollection::Week,
        None,
        today,
        week_cutoff_day,
        recent_cutoff,
    ));
    assert!(!LocalPasteApp::matches_active_filters(
        &item("before-cutoff", before_cutoff_day),
        &SidebarCollection::Week,
        None,
        today,
        week_cutoff_day,
        recent_cutoff,
    ));
}
