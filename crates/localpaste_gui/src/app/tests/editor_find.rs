//! Current-paste find behavior and sidebar search handoff regressions.

use super::*;

#[test]
fn editor_find_selects_first_match_and_wraps_navigation() {
    let mut harness = make_app();
    harness
        .app
        .reset_virtual_editor("alpha needle beta needle gamma");

    harness.app.open_editor_find();
    harness.app.set_editor_find_query("needle".to_string());

    assert_eq!(harness.app.editor_find.matches, vec![6..12, 18..24]);
    assert_eq!(harness.app.editor_find.active_match, Some(0));
    assert_eq!(
        harness.app.virtual_editor_state.selection_range(),
        Some(6..12)
    );
    assert!(harness.app.virtual_follow_cursor_next_frame);

    harness.app.editor_find_next();
    assert_eq!(harness.app.editor_find.active_match, Some(1));
    assert_eq!(
        harness.app.virtual_editor_state.selection_range(),
        Some(18..24)
    );

    harness.app.editor_find_next();
    assert_eq!(harness.app.editor_find.active_match, Some(0));
    assert_eq!(
        harness.app.virtual_editor_state.selection_range(),
        Some(6..12)
    );

    harness.app.editor_find_previous();
    assert_eq!(harness.app.editor_find.active_match, Some(1));
}

#[test]
fn editor_find_rebuilds_after_buffer_revision_changes() {
    let mut harness = make_app();
    harness.app.reset_virtual_editor("needle alpha");
    harness.app.open_editor_find();
    harness.app.set_editor_find_query("needle".to_string());
    assert_eq!(harness.app.editor_find.matches, vec![0..6]);

    let len = harness.app.virtual_editor_buffer.len_chars();
    harness
        .app
        .virtual_editor_buffer
        .replace_char_range(len..len, " needle")
        .expect("append text");

    harness.app.editor_find_next();
    assert_eq!(harness.app.editor_find.matches, vec![0..6, 13..19]);
    assert_eq!(harness.app.editor_find.active_match, Some(1));
    assert_eq!(
        harness.app.virtual_editor_state.selection_range(),
        Some(13..19)
    );
}

#[test]
fn editor_find_reopen_with_saved_query_selects_active_match() {
    let mut harness = make_app();
    harness.app.editor_find.query = "needle".to_string();
    harness.app.reset_virtual_editor("alpha needle beta");

    harness.app.open_editor_find();

    assert_eq!(harness.app.editor_find.active_match, Some(0));
    assert_eq!(
        harness.app.virtual_editor_state.selection_range(),
        Some(6..12)
    );
}

#[test]
fn sidebar_full_text_query_primes_current_paste_find() {
    let mut harness = make_app();
    harness.app.search_query = "target".to_string();
    let paste = Paste::new(
        "first line\nsecond target line\n".to_string(),
        "Has body match".to_string(),
    );

    harness.app.select_loaded_paste(paste);

    assert!(harness.app.editor_find.open);
    assert_eq!(harness.app.editor_find.query, "target");
    assert_eq!(harness.app.editor_find.matches, vec![18..24]);
    assert_eq!(
        harness.app.virtual_editor_state.selection_range(),
        Some(18..24)
    );
}

#[test]
fn sidebar_metadata_only_query_does_not_open_editor_find() {
    let mut harness = make_app();
    harness.app.search_query = "metadata-only".to_string();
    let paste = Paste::new("body content".to_string(), "Metadata only".to_string());

    harness.app.select_loaded_paste(paste);

    assert!(!harness.app.editor_find.open);
    assert!(harness.app.editor_find.matches.is_empty());
}
