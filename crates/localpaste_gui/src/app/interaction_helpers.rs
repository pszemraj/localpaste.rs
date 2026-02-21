//! Shared input/selection helpers extracted from `app::mod`.

use super::{
    VirtualCommandRoute, VirtualInputCommand, DRAG_AUTOSCROLL_EDGE_DISTANCE,
    DRAG_AUTOSCROLL_MAX_LINES_PER_FRAME, DRAG_AUTOSCROLL_MIN_LINES_PER_FRAME,
    EDITOR_DOUBLE_CLICK_DISTANCE, EDITOR_DOUBLE_CLICK_WINDOW,
};
use eframe::egui;
use std::ops::Range;
use std::time::Instant;

/// Routing bucket used for virtual-editor command deferral.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VirtualCommandBucket {
    ImmediateFocus,
    DeferredFocus,
    DeferredCopy,
}

/// Returns whether bare arrow keys should drive sidebar selection navigation.
///
/// Sidebar navigation is intentionally disabled whenever keyboard ownership is
/// ambiguous (focused editor, open modal/drawer, or active modifiers).
///
/// # Arguments
/// - `wants_keyboard_input_before`: Egui keyboard-input ownership snapshot.
/// - `modifiers`: Active keyboard modifiers for this frame.
/// - `has_pastes`: Whether sidebar list has at least one selectable paste.
/// - `focus_active_pre`: Virtual-editor focus state before routing.
/// - `command_palette_open`: Whether command palette modal is open.
/// - `properties_drawer_open`: Whether properties drawer is open.
/// - `shortcut_help_open`: Whether shortcut help modal is open.
///
/// # Returns
/// `true` only when bare arrows should select previous/next paste in sidebar.
pub(crate) fn should_route_sidebar_arrows(
    wants_keyboard_input_before: bool,
    modifiers: egui::Modifiers,
    has_pastes: bool,
    focus_active_pre: bool,
    command_palette_open: bool,
    properties_drawer_open: bool,
    shortcut_help_open: bool,
) -> bool {
    let has_nav_modifiers = modifiers.ctrl || modifiers.alt || modifiers.shift || modifiers.command;
    let overlays_open = command_palette_open || properties_drawer_open || shortcut_help_open;

    has_pastes
        && !wants_keyboard_input_before
        && !has_nav_modifiers
        && !focus_active_pre
        && !overlays_open
}

/// Returns whether modifiers represent a plain command chord (no Shift/Alt).
///
/// # Returns
/// `true` when `Ctrl`/`Cmd` is active without Shift/Alt.
pub(crate) fn is_plain_command_shortcut(modifiers: egui::Modifiers) -> bool {
    modifiers.command && !modifiers.shift && !modifiers.alt
}

/// Returns whether modifiers represent a command+shift chord (no Alt).
///
/// # Returns
/// `true` when `Ctrl`/`Cmd` and Shift are active without Alt.
pub(crate) fn is_command_shift_shortcut(modifiers: egui::Modifiers) -> bool {
    modifiers.command && modifiers.shift && !modifiers.alt
}

/// Returns whether a character should be treated as an editor word character.
///
/// # Returns
/// `true` for ASCII alphanumeric characters and underscores.
pub(crate) fn is_editor_word_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

/// Calculates click streak count for virtual-editor single/double/triple click handling.
///
/// # Arguments
/// - `last_at`: Timestamp of previous click.
/// - `last_pos`: Pointer position of previous click.
/// - `_last_line`: Previous line index (currently unused).
/// - `last_count`: Previous click streak count.
/// - `_line_idx`: Current line index (currently unused).
/// - `pointer_pos`: Current click pointer position.
/// - `now`: Current timestamp.
///
/// # Returns
/// Updated click streak count clamped to `1..=3`.
pub(crate) fn next_virtual_click_count(
    last_at: Option<Instant>,
    last_pos: Option<egui::Pos2>,
    _last_line: Option<usize>,
    last_count: u8,
    _line_idx: usize,
    pointer_pos: egui::Pos2,
    now: Instant,
) -> u8 {
    let is_continuation = if let (Some(last_at), Some(last_pos)) = (last_at, last_pos) {
        now.duration_since(last_at) <= EDITOR_DOUBLE_CLICK_WINDOW
            && last_pos.distance(pointer_pos) <= EDITOR_DOUBLE_CLICK_DISTANCE
    } else {
        false
    };
    if is_continuation {
        last_count.saturating_add(1).min(3)
    } else {
        1
    }
}

/// Computes vertical drag autoscroll delta for pointer positions outside viewport.
///
/// # Arguments
/// - `pointer_y`: Current pointer y-coordinate.
/// - `top`: Viewport top boundary.
/// - `bottom`: Viewport bottom boundary.
/// - `line_height`: Active row height in pixels.
///
/// # Returns
/// Positive delta to scroll up, negative delta to scroll down, or `0.0`.
///
/// # Panics
/// Panics only if floating-point math invariants are violated unexpectedly.
pub(crate) fn drag_autoscroll_delta(
    pointer_y: f32,
    top: f32,
    bottom: f32,
    line_height: f32,
) -> f32 {
    if !pointer_y.is_finite()
        || !top.is_finite()
        || !bottom.is_finite()
        || !line_height.is_finite()
        || line_height <= 0.0
        || bottom <= top
    {
        return 0.0;
    }

    let outside_distance = if pointer_y < top {
        top - pointer_y
    } else if pointer_y > bottom {
        pointer_y - bottom
    } else {
        return 0.0;
    };

    // Scale autoscroll speed with distance beyond the viewport edge.
    let edge_distance = (line_height * 2.0).max(DRAG_AUTOSCROLL_EDGE_DISTANCE);
    let lines_per_frame = (outside_distance / edge_distance).clamp(
        DRAG_AUTOSCROLL_MIN_LINES_PER_FRAME,
        DRAG_AUTOSCROLL_MAX_LINES_PER_FRAME,
    );
    let delta = line_height * lines_per_frame;

    if pointer_y < top {
        delta
    } else {
        -delta
    }
}

/// Classifies a virtual-editor input command into immediate or deferred buckets.
///
/// # Arguments
/// - `command`: Virtual input command to classify.
/// - `focus_active_pre`: Whether virtual editor focus was active before routing.
///
/// # Returns
/// Command bucket used by the main update loop.
pub(crate) fn classify_virtual_command(
    command: &VirtualInputCommand,
    focus_active_pre: bool,
) -> VirtualCommandBucket {
    match command.route() {
        VirtualCommandRoute::CopyOnly => VirtualCommandBucket::DeferredCopy,
        VirtualCommandRoute::FocusRequired => {
            if command.requires_post_focus() || !focus_active_pre {
                VirtualCommandBucket::DeferredFocus
            } else {
                VirtualCommandBucket::ImmediateFocus
            }
        }
    }
}

/// Paints a line-scoped selection overlay onto a rendered galley row.
///
/// # Arguments
/// - `painter`: Painter used for overlay rendering.
/// - `row_rect`: Row rectangle in UI coordinates.
/// - `galley`: Shaped galley for the row.
/// - `selection`: Selection range in row-relative character coordinates.
/// - `selection_fill`: Fill color for selected regions.
pub(crate) fn paint_virtual_selection_overlay(
    painter: &egui::Painter,
    row_rect: egui::Rect,
    galley: &egui::Galley,
    selection: Range<usize>,
    selection_fill: egui::Color32,
) {
    if selection.start >= selection.end {
        return;
    }
    let mut consumed = 0usize;
    for placed_row in &galley.rows {
        let row_chars = placed_row.char_count_excluding_newline();
        let local_start = selection.start.saturating_sub(consumed).min(row_chars);
        let local_end = selection.end.saturating_sub(consumed).min(row_chars);
        if local_end > local_start {
            let left = row_rect.min.x + placed_row.pos.x + placed_row.x_offset(local_start);
            let mut right = row_rect.min.x + placed_row.pos.x + placed_row.x_offset(local_end);
            if right <= left {
                right = left + 1.0;
            }
            let top = row_rect.min.y + placed_row.pos.y;
            let bottom = top + placed_row.height();
            let rect = egui::Rect::from_min_max(egui::pos2(left, top), egui::pos2(right, bottom));
            painter.rect_filled(rect, 2.0, selection_fill);
        }
        consumed = consumed.saturating_add(placed_row.char_count_including_newline());
        if consumed >= selection.end {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        is_command_shift_shortcut, is_plain_command_shortcut, should_route_sidebar_arrows,
    };
    use eframe::egui;

    #[test]
    fn sidebar_arrow_routing_guard_matrix() {
        struct Case {
            wants_keyboard_input_before: bool,
            modifiers: egui::Modifiers,
            has_pastes: bool,
            focus_active_pre: bool,
            command_palette_open: bool,
            properties_drawer_open: bool,
            shortcut_help_open: bool,
            expected: bool,
        }

        let cases = [
            Case {
                wants_keyboard_input_before: false,
                modifiers: egui::Modifiers::NONE,
                has_pastes: true,
                focus_active_pre: false,
                command_palette_open: false,
                properties_drawer_open: false,
                shortcut_help_open: false,
                expected: true,
            },
            Case {
                wants_keyboard_input_before: true,
                modifiers: egui::Modifiers::NONE,
                has_pastes: true,
                focus_active_pre: false,
                command_palette_open: false,
                properties_drawer_open: false,
                shortcut_help_open: false,
                expected: false,
            },
            Case {
                wants_keyboard_input_before: false,
                modifiers: egui::Modifiers {
                    shift: true,
                    ..egui::Modifiers::NONE
                },
                has_pastes: true,
                focus_active_pre: false,
                command_palette_open: false,
                properties_drawer_open: false,
                shortcut_help_open: false,
                expected: false,
            },
            Case {
                wants_keyboard_input_before: false,
                modifiers: egui::Modifiers::NONE,
                has_pastes: true,
                focus_active_pre: true,
                command_palette_open: false,
                properties_drawer_open: false,
                shortcut_help_open: false,
                expected: false,
            },
            Case {
                wants_keyboard_input_before: false,
                modifiers: egui::Modifiers::NONE,
                has_pastes: true,
                focus_active_pre: false,
                command_palette_open: true,
                properties_drawer_open: false,
                shortcut_help_open: false,
                expected: false,
            },
            Case {
                wants_keyboard_input_before: false,
                modifiers: egui::Modifiers::NONE,
                has_pastes: false,
                focus_active_pre: false,
                command_palette_open: false,
                properties_drawer_open: false,
                shortcut_help_open: false,
                expected: false,
            },
        ];

        for case in cases {
            let actual = should_route_sidebar_arrows(
                case.wants_keyboard_input_before,
                case.modifiers,
                case.has_pastes,
                case.focus_active_pre,
                case.command_palette_open,
                case.properties_drawer_open,
                case.shortcut_help_open,
            );
            assert_eq!(actual, case.expected);
        }
    }

    #[test]
    fn command_shortcut_modifier_matrix() {
        let plain = egui::Modifiers {
            command: true,
            ..egui::Modifiers::NONE
        };
        assert!(is_plain_command_shortcut(plain));
        assert!(!is_command_shift_shortcut(plain));

        let command_shift = egui::Modifiers {
            command: true,
            shift: true,
            ..egui::Modifiers::NONE
        };
        assert!(!is_plain_command_shortcut(command_shift));
        assert!(is_command_shift_shortcut(command_shift));

        let command_alt = egui::Modifiers {
            command: true,
            alt: true,
            ..egui::Modifiers::NONE
        };
        assert!(!is_plain_command_shortcut(command_alt));
        assert!(!is_command_shift_shortcut(command_alt));

        let command_shift_alt = egui::Modifiers {
            command: true,
            shift: true,
            alt: true,
            ..egui::Modifiers::NONE
        };
        assert!(!is_plain_command_shortcut(command_shift_alt));
        assert!(!is_command_shift_shortcut(command_shift_alt));
    }
}
