//! Shared helpers for virtual editor focus tests.

use super::*;
use crate::app::virtual_editor::PlatformFlavor;

/// Builds the standard app viewport rectangle used by full-frame focus tests.
///
/// # Returns
/// A fixed logical viewport rectangle large enough to include sidebar and editor hit targets.
pub(super) fn screen_rect() -> egui::Rect {
    egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1200.0, 900.0))
}

/// Builds a pointer move plus primary-button press at a target position.
///
/// # Returns
/// Pointer events that simulate a primary click without release.
pub(super) fn primary_click_events(pos: egui::Pos2) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::default(),
        },
    ]
}

/// Builds the platform-specific modifier state for focused word selection.
///
/// # Returns
/// Option-shift on macOS and control-command-shift elsewhere, matching the virtual editor input map.
pub(super) fn focused_word_select_modifiers() -> egui::Modifiers {
    #[cfg(target_os = "macos")]
    {
        egui::Modifiers {
            alt: true,
            shift: true,
            ..Default::default()
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        egui::Modifiers {
            ctrl: true,
            command: true,
            shift: true,
            ..Default::default()
        }
    }
}

/// Asserts that egui focus is currently owned by the virtual editor widget.
///
/// # Panics
/// Panics when the virtual editor does not own egui keyboard focus.
pub(super) fn assert_editor_focus(ctx: &egui::Context) {
    assert!(ctx.memory(|m| m.has_focus(egui::Id::new(VIRTUAL_EDITOR_ID))));
}

/// Asserts that common app chrome widgets did not steal keyboard focus.
///
/// # Arguments
/// - `ctx`: egui context containing focus state.
/// - `name`: Scenario label included in assertion messages.
///
/// # Panics
/// Panics when any tracked chrome widget owns keyboard focus.
pub(super) fn assert_no_chrome_focus(ctx: &egui::Context, name: &str) {
    for (label, id) in [
        ("sidebar search", SEARCH_INPUT_ID),
        ("editor title", TITLE_INPUT_ID),
        ("command palette query", COMMAND_PALETTE_INPUT_ID),
        ("properties name", PROPERTIES_NAME_INPUT_ID),
        ("properties tags", PROPERTIES_TAGS_INPUT_ID),
        ("diff query", DIFF_QUERY_INPUT_ID),
    ] {
        assert!(
            !ctx.memory(|m| m.has_focus(egui::Id::new(id))),
            "{name} should not move focus to {label}"
        );
    }
}

/// Returns a stable point inside the editor body for click-to-focus tests.
///
/// # Returns
/// Logical viewport coordinates within the editor body.
pub(super) fn editor_body_click_pos() -> egui::Pos2 {
    egui::pos2(520.0, 700.0)
}

/// Builds document-boundary navigation modifiers for the requested platform flavor.
///
/// # Returns
/// Command on macOS and control-command elsewhere.
pub(super) fn platform_doc_modifiers(platform: PlatformFlavor) -> egui::Modifiers {
    match platform {
        PlatformFlavor::Mac => egui::Modifiers {
            command: true,
            ..Default::default()
        },
        PlatformFlavor::Other => egui::Modifiers {
            ctrl: true,
            command: true,
            ..Default::default()
        },
    }
}

/// Builds raw input with explicit native and viewport focus state.
///
/// # Arguments
/// - `focused`: Native window focus flag for the frame.
/// - `viewport_focused`: Root viewport focus override.
/// - `events`: Input events to attach to the frame.
/// - `modifiers`: Keyboard modifiers to attach to the frame.
///
/// # Returns
/// An egui raw input frame carrying the supplied focus flags, events, and modifiers.
///
/// # Panics
/// Panics if the root viewport is missing from egui raw input defaults.
pub(super) fn raw_input_with_viewport_focus(
    focused: bool,
    viewport_focused: Option<bool>,
    events: Vec<egui::Event>,
    modifiers: egui::Modifiers,
) -> egui::RawInput {
    let mut input = egui::RawInput {
        focused,
        screen_rect: Some(screen_rect()),
        modifiers,
        events,
        ..Default::default()
    };
    input
        .viewports
        .get_mut(&egui::ViewportId::ROOT)
        .expect("root viewport")
        .focused = viewport_focused;
    input
}
