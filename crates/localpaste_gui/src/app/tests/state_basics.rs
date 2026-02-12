use super::*;

#[test]
fn paste_missing_clears_selection_and_removes_list_entry() {
    let mut harness = make_app();
    harness.app.apply_event(CoreEvent::PasteMissing {
        id: "alpha".to_string(),
    });

    assert!(harness.app.pastes.is_empty());
    assert!(harness.app.selected_id.is_none());
    assert!(harness.app.selected_paste.is_none());
    assert_eq!(harness.app.selected_content.len(), 0);
    assert!(harness.app.status.is_some());
}

#[test]
fn paste_missing_non_selected_removes_list_entry() {
    let mut harness = make_app();
    harness.app.pastes.push(PasteSummary {
        id: "beta".to_string(),
        name: "Beta".to_string(),
        language: None,
        content_len: 4,
        updated_at: Utc::now(),
        folder_id: None,
        tags: Vec::new(),
    });

    harness.app.apply_event(CoreEvent::PasteMissing {
        id: "beta".to_string(),
    });

    assert_eq!(harness.app.pastes.len(), 1);
    assert_eq!(harness.app.pastes[0].id, "alpha");
    assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));
    assert!(harness.app.selected_paste.is_some());
}

#[test]
fn set_status_pushes_toast_feedback() {
    let mut harness = make_app();
    harness.app.set_status("Saved metadata.");

    assert!(harness.app.status.is_some());
    assert_eq!(harness.app.toasts.len(), 1);
    assert_eq!(
        harness.app.toasts.back().map(|toast| toast.text.as_str()),
        Some("Saved metadata.")
    );
}

#[test]
fn toast_queue_dedupes_tail_and_caps_length() {
    let mut harness = make_app();

    harness.app.set_status("Repeated");
    harness.app.set_status("Repeated");
    assert_eq!(harness.app.toasts.len(), 1);

    for idx in 0..(TOAST_LIMIT + 2) {
        harness.app.set_status(format!("Toast {}", idx));
    }
    assert_eq!(harness.app.toasts.len(), TOAST_LIMIT);
}

#[test]
fn editor_buffer_tracks_char_len() {
    let mut buffer = EditorBuffer::new("ab".to_string());
    assert_eq!(buffer.chars_len(), 2);

    buffer.insert_text("\u{00E9}", 1);
    assert_eq!(buffer.chars_len(), 3);

    buffer.delete_char_range(1..2);
    assert_eq!(buffer.chars_len(), 2);

    buffer.replace_with("xyz");
    assert_eq!(buffer.chars_len(), 3);

    buffer.clear();
    assert_eq!(buffer.chars_len(), 0);
}
