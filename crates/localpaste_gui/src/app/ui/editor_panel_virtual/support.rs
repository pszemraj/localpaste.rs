//! Support geometry, focus, and scroll helpers for the virtual editor.

use super::super::super::*;
use eframe::egui;

/// Left/right inset between the line-number gutter and editor text.
pub(super) const VIRTUAL_EDITOR_TEXT_INSET: f32 = 6.0;
/// Padding around painted line numbers inside the gutter.
pub(super) const VIRTUAL_EDITOR_LINE_NUMBER_PADDING: f32 = 8.0;

/// Returns the compact monospace font used for virtual-editor line numbers.
///
/// # Arguments
/// - `row_height`: Current virtual editor row height.
///
/// # Returns
/// Monospace font id scaled and clamped from the row height.
pub(super) fn line_number_font_for_row_height(row_height: f32) -> egui::FontId {
    egui::FontId::monospace((row_height * 0.72).clamp(10.0, 14.0))
}

/// Returns the gutter width needed for the visible line-number digit count.
///
/// # Arguments
/// - `line_count`: Number of physical lines in the buffer.
/// - `line_number_char_width`: Measured monospace digit width.
///
/// # Returns
/// Width in UI points for the line-number gutter.
pub(super) fn line_number_gutter_width(line_count: usize, line_number_char_width: f32) -> f32 {
    let line_number_digits = line_count.max(1).to_string().len();
    (line_number_digits as f32 * line_number_char_width.max(1.0))
        + VIRTUAL_EDITOR_LINE_NUMBER_PADDING * 2.0
}

/// Returns a click/drag sense that does not enter egui's focus ring.
///
/// # Returns
/// Non-focusable click-and-drag sense for row hit testing.
pub(super) fn virtual_row_hit_test_sense() -> egui::Sense {
    let mut sense = egui::Sense::click_and_drag();
    sense.remove(egui::Sense::focusable_noninteractive());
    sense
}

/// Returns the event-filter keys owned while the virtual editor has focus.
///
/// # Arguments
/// - `editor_shortcuts_available`: Whether focused editor shortcuts may own navigation keys.
///
/// # Returns
/// egui focus-lock event filter for editor-owned navigation keys.
pub(super) fn virtual_editor_focus_lock_filter(
    editor_shortcuts_available: bool,
) -> egui::EventFilter {
    egui::EventFilter {
        tab: editor_shortcuts_available,
        horizontal_arrows: editor_shortcuts_available,
        vertical_arrows: editor_shortcuts_available,
        escape: false,
    }
}

/// Extends the editor interaction rect to include the vertical scrollbar gutter.
///
/// # Arguments
/// - `inner_rect`: Rect reported by the inner scroll area content.
/// - `wrap_width`: Full editor wrap width including any scrollbar gutter.
///
/// # Returns
/// Rect used for inside/outside pointer classification.
pub(super) fn editor_interaction_rect(inner_rect: egui::Rect, wrap_width: f32) -> egui::Rect {
    let scrollbar_gutter = (wrap_width - inner_rect.width()).max(0.0);
    if scrollbar_gutter <= 0.0 {
        return inner_rect;
    }
    egui::Rect::from_min_max(
        inner_rect.min,
        egui::pos2(inner_rect.max.x + scrollbar_gutter, inner_rect.max.y),
    )
}

/// Returns whether a pointer/window event should explicitly blur the editor.
///
/// # Arguments
/// - `clicked_outside_editor`: Whether the primary pointer press landed outside the editor.
/// - `window_blurred`: Whether the app viewport lost focus.
/// - `preserve_editor_focus`: Whether editor chrome claimed focus preservation for this frame.
///
/// # Returns
/// `true` when the virtual editor should surrender egui focus.
pub(super) fn should_explicitly_blur_virtual_editor(
    clicked_outside_editor: bool,
    window_blurred: bool,
    preserve_editor_focus: bool,
) -> bool {
    window_blurred || (clicked_outside_editor && !preserve_editor_focus)
}

/// Marks key events consumed after virtual-editor routing applies them.
///
/// # Arguments
/// - `ctx`: egui context containing the current input event stream.
/// - `applied_commands`: Commands accepted by virtual-editor input routing.
pub(super) fn consume_virtual_editor_owned_key_events(
    ctx: &egui::Context,
    applied_commands: &[VirtualInputCommand],
) {
    // Non-key events are routed by focused ownership; `consume_key` only
    // models keys whose commands survived the editor's route filters.
    let keys_to_consume = ctx.input(|input| {
        input
            .events
            .iter()
            .filter_map(|event| {
                let egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } = event
                else {
                    return None;
                };
                let event_commands = commands_from_events(std::slice::from_ref(event), true);
                if !event_commands
                    .iter()
                    .any(|command| applied_commands.contains(command))
                {
                    None
                } else {
                    Some((*modifiers, *key))
                }
            })
            .collect::<Vec<_>>()
    });
    if keys_to_consume.is_empty() {
        return;
    }
    ctx.input_mut(|input| {
        for (modifiers, key) in keys_to_consume {
            input.consume_key(modifiers, key);
        }
    });
}

/// Returns global character bounds for a double-click word selection.
///
/// # Arguments
/// - `line_start`: Global character offset for the start of the line.
/// - `column_in_line`: Clicked character column within the line.
/// - `line`: Full line text.
/// - `clamp_global`: Callback that clamps global bounds to the renderable range.
///
/// # Returns
/// Global character selection bounds when the click lands on a word token.
pub(super) fn virtual_editor_double_click_selection_bounds<F>(
    line_start: usize,
    column_in_line: usize,
    line: &str,
    clamp_global: F,
) -> Option<(usize, usize)>
where
    F: Fn(usize) -> usize,
{
    let (start, end) = word_range_at(line, column_in_line)?;
    Some((
        clamp_global(line_start.saturating_add(start)),
        clamp_global(line_start.saturating_add(end)),
    ))
}

/// Returns the vertical scroll offset needed to keep the cursor in view.
///
/// # Arguments
/// - `follow_requested`: Whether this frame requested cursor-follow scrolling.
/// - `cursor_row`: Visual row index containing the cursor.
/// - `visible_row_range`: Currently visible visual row range.
/// - `viewport_rows`: Number of rows that fit in the viewport.
/// - `line_height`: Height of a virtual editor row.
///
/// # Returns
/// New vertical scroll offset when the cursor needs to be revealed.
pub(super) fn follow_cursor_scroll_offset_y(
    follow_requested: bool,
    cursor_row: usize,
    visible_row_range: std::ops::Range<usize>,
    viewport_rows: usize,
    line_height: f32,
) -> Option<f32> {
    if !follow_requested || viewport_rows == 0 {
        return None;
    }
    let scrolloff_rows = 2usize.min(viewport_rows.saturating_sub(1));
    if cursor_row.saturating_add(scrolloff_rows) >= visible_row_range.end {
        let desired_top = cursor_row
            .saturating_add(1)
            .saturating_add(scrolloff_rows)
            .saturating_sub(viewport_rows);
        return Some(desired_top as f32 * line_height);
    }
    if cursor_row < visible_row_range.start.saturating_add(scrolloff_rows) {
        let desired_top = cursor_row.saturating_sub(scrolloff_rows);
        return Some(desired_top as f32 * line_height);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interaction_rect_handles_scrollbar_gutter_matrix() {
        struct Case {
            total_width: f32,
            expected_extra_right: f32,
        }

        let inner = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(180.0, 60.0));
        let cases = [
            Case {
                total_width: 180.0,
                expected_extra_right: 0.0,
            },
            Case {
                total_width: 194.0,
                expected_extra_right: 14.0,
            },
        ];

        for case in cases {
            let rect = editor_interaction_rect(inner, case.total_width);
            assert_eq!(rect.min, inner.min);
            assert_eq!(rect.max.y, inner.max.y);
            assert_eq!(rect.max.x, inner.max.x + case.expected_extra_right);
        }
    }

    #[test]
    fn follow_cursor_scroll_offset_only_applies_when_requested() {
        let hidden_cursor_offset = follow_cursor_scroll_offset_y(false, 100, 0..20, 20, 12.0);
        assert_eq!(hidden_cursor_offset, None);

        let requested_offset = follow_cursor_scroll_offset_y(true, 100, 0..20, 20, 12.0);
        assert!(
            requested_offset.is_some(),
            "requested follow should produce a scroll offset when caret is out of view"
        );
    }

    #[test]
    fn virtual_editor_double_click_selection_respects_clamp_callback() {
        let line_start = 17usize;
        let clamp_end = line_start.saturating_add(32);
        let line = "a".repeat(96);

        let bounds =
            virtual_editor_double_click_selection_bounds(line_start, 31, line.as_str(), |global| {
                global.min(clamp_end)
            })
            .expect("expected word bounds");

        assert_eq!(bounds, (line_start, clamp_end));
    }

    #[test]
    fn line_number_font_size_is_clamped() {
        assert_eq!(line_number_font_for_row_height(1.0).size, 10.0);
        assert_eq!(line_number_font_for_row_height(100.0).size, 14.0);
        let mid = line_number_font_for_row_height(16.0).size;
        assert!(mid > 10.0 && mid < 14.0);
    }

    #[test]
    fn line_number_gutter_width_scales_with_digits_and_char_width() {
        let single_digit = line_number_gutter_width(9, 5.0);
        let two_digits = line_number_gutter_width(10, 5.0);
        let wider_chars = line_number_gutter_width(10, 7.0);
        assert!(two_digits > single_digit);
        assert!(wider_chars > two_digits);

        let clamped = line_number_gutter_width(999, 0.0);
        let expected = (3.0 * 1.0) + VIRTUAL_EDITOR_LINE_NUMBER_PADDING * 2.0;
        assert!((clamped - expected).abs() < f32::EPSILON);
    }

    #[test]
    fn virtual_row_hit_test_sense_is_non_focusable_click_and_drag() {
        let sense = virtual_row_hit_test_sense();
        assert!(sense.senses_click());
        assert!(sense.senses_drag());
        assert!(!sense.is_focusable());
    }

    #[test]
    fn focus_lock_filter_releases_navigation_when_shortcuts_are_blocked() {
        let available = virtual_editor_focus_lock_filter(true);
        assert!(available.tab);
        assert!(available.horizontal_arrows);
        assert!(available.vertical_arrows);
        assert!(!available.escape);

        let blocked = virtual_editor_focus_lock_filter(false);
        assert!(!blocked.tab);
        assert!(!blocked.horizontal_arrows);
        assert!(!blocked.vertical_arrows);
        assert!(!blocked.escape);
    }

    #[test]
    fn explicit_blur_policy_preserves_editor_focus_for_editor_chrome_actions() {
        assert!(should_explicitly_blur_virtual_editor(true, false, false));
        assert!(!should_explicitly_blur_virtual_editor(true, false, true));
        assert!(should_explicitly_blur_virtual_editor(false, true, true));
        assert!(!should_explicitly_blur_virtual_editor(false, false, false));
    }
}
