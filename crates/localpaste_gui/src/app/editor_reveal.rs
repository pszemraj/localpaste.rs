//! Explicit caret reveal requests and observed viewport geometry.

use super::{LocalPasteApp, VirtualInputCommand, VIRTUAL_EDITOR_ID};
use eframe::egui;

/// How the next rendered frame should reveal the caret, independent of focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CursorReveal {
    /// Move only enough to keep the caret and a small margin visible.
    Minimal,
    /// Put a document jump or Find result near the viewport center.
    Center,
}

/// Geometry from the actual scroll viewport, also used by the navigation probe.
#[derive(Default)]
pub(super) struct EditorViewport {
    pub(super) rect: Option<egui::Rect>,
    pub(super) caret: Option<egui::Rect>,
    pub(super) offset_y: f32,
}

impl EditorViewport {
    /// Whether the rendered caret is fully inside the real viewport.
    ///
    /// # Returns
    /// True only when both bounds are available and the caret fits.
    pub(super) fn caret_visible(&self) -> bool {
        self.rect
            .zip(self.caret)
            .is_some_and(|(view, caret)| view.expand(0.5).contains_rect(caret))
    }
}

impl CursorReveal {
    /// Calculate a scroll target in content coordinates using actual viewport height.
    ///
    /// # Arguments
    /// - `row`: Caret visual row.
    /// - `offset`: Current vertical scroll offset.
    /// - `height`: Actual viewport height.
    /// - `stride`: Positive rendered row height.
    ///
    /// # Returns
    /// Nonnegative vertical scroll offset.
    ///
    /// # Panics
    /// Does not intentionally panic.
    pub(super) fn offset(self, row: usize, offset: f32, height: f32, stride: f32) -> f32 {
        let top = row as f32 * stride;
        let margin = (2.0 * stride).min((height - stride).max(0.0) / 2.0);
        match self {
            Self::Center => (top + stride / 2.0 - height / 2.0).max(0.0),
            Self::Minimal if top < offset + margin => (top - margin).max(0.0),
            Self::Minimal if top + stride > offset + height - margin => {
                (top + stride + margin - height).max(0.0)
            }
            Self::Minimal => offset,
        }
    }
}

impl LocalPasteApp {
    /// Drop editor ownership on native deactivation without disturbing the selection.
    pub(super) fn blur_deactivated_editor(&mut self, ctx: &egui::Context) {
        let last_focus = ctx.input(|input| {
            input.events.iter().rev().find_map(|event| match event {
                egui::Event::WindowFocused(focused) => Some(*focused),
                _ => None,
            })
        });
        if last_focus == Some(false) {
            self.virtual_editor_state.has_focus = false;
            self.focus_editor_next = false;
            ctx.memory_mut(|memory| memory.surrender_focus(egui::Id::new(VIRTUAL_EDITOR_ID)));
        }
    }

    /// Navigation remains a reveal request even when the caret is already at a boundary.
    pub(super) fn request_navigation_reveal(&mut self, command: &VirtualInputCommand) {
        use VirtualInputCommand::*;
        let reveal = match command {
            MoveDocHome { .. } | MoveDocEnd { .. } => CursorReveal::Center,
            MoveLeft { .. }
            | MoveRight { .. }
            | MoveUp { .. }
            | MoveDown { .. }
            | MoveLineHome { .. }
            | MoveLineEnd { .. }
            | PageUp { .. }
            | PageDown { .. }
            | SelectAll => CursorReveal::Minimal,
            _ => return,
        };
        self.virtual_cursor_reveal = Some(reveal);
    }
}
