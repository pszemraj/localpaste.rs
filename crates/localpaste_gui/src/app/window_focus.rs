//! Editor keyboard ownership across native window deactivation.
//!
//! Like a native text view, the editor keeps the keyboard across an app
//! switch. Deactivation releases egui focus, so the inactive window neither
//! blinks a caret nor keeps an IME composition or a drag alive, and
//! reactivation returns it. The first clipboard paste after switching back
//! still becomes a new paste, as it does with nothing focused, until other
//! input shows the user is editing again.

use super::shortcuts::{runtime_shortcut_action, RuntimeShortcutAction};
use super::{last_window_focus, LocalPasteApp, VIRTUAL_EDITOR_ID};
use eframe::egui;
use std::time::Instant;

/// Editor keyboard ownership carried across a native window switch.
#[derive(Debug, Default)]
pub(super) struct WindowSwitchFocus {
    /// Paste whose editor owned the keyboard when the window deactivated.
    owner: Option<String>,
    /// Paste whose editor reactivation refocused; its next clipboard paste
    /// is routed as if the editor were unfocused.
    paste_capture: Option<String>,
}

impl LocalPasteApp {
    /// Releases editor ownership on native deactivation without disturbing the selection.
    ///
    /// Runs after the frame's input is routed, so edits that arrived before the
    /// deactivation finish in the editor. Records whether the editor owned the
    /// keyboard so reactivation can return it.
    pub(super) fn blur_deactivated_editor(&mut self, ctx: &egui::Context) {
        let deactivated = ctx.input(|input| {
            input
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::WindowFocused(false)))
        });
        if deactivated {
            // Deactivation can swallow the button release (no button-up after
            // Alt+Tab mid-drag), leaving egui's primary button reported down.
            // End the gesture here; this frame's render already applied any
            // release it did receive.
            self.virtual_drag_active = false;
            self.virtual_pointer_press_modifiers = None;
        }
        if last_window_focus(ctx) == Some(false) {
            let owned = self.focus_editor_next
                || self.virtual_editor_state.has_focus
                || ctx.memory(|memory| memory.has_focus(egui::Id::new(VIRTUAL_EDITOR_ID)));
            self.window_switch_focus = WindowSwitchFocus {
                owner: owned.then(|| self.selected_id.clone()).flatten(),
                paste_capture: None,
            };
            if self.cancel_virtual_ime_preedit_if_active(Instant::now()) {
                self.mark_dirty();
            }
            self.release_editor_keyboard(ctx);
        }
    }

    /// Returns the keyboard to the editor that owned it when the window deactivated.
    ///
    /// Runs before input routing on the frame whose last focus event activates
    /// the window, so a click in that frame then moves focus as any click does.
    /// Focus stays put when anything changed meanwhile: another or an unloaded
    /// paste, an overlay or reset fence, or another focused widget. A key,
    /// text, or press other than a clipboard paste ends the paste capture.
    pub(super) fn restore_reactivated_editor(&mut self, ctx: &egui::Context) {
        let editing = ctx.input(|input| input.events.iter().any(ends_paste_capture));
        if editing {
            self.window_switch_focus.paste_capture = None;
        }
        if last_window_focus(ctx) != Some(true) {
            return;
        }
        let Some(owner) = self.window_switch_focus.owner.take() else {
            return;
        };
        let still_open = self.selected_id.as_deref() == Some(owner.as_str())
            && self
                .selected_paste
                .as_ref()
                .is_some_and(|paste| paste.id == owner);
        if !still_open
            || self.editor_shortcuts_blocked()
            || ctx.memory(|memory| memory.focused().is_some())
        {
            return;
        }
        self.virtual_editor_state.has_focus = true;
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(VIRTUAL_EDITOR_ID)));
        self.reset_virtual_caret_blink();
        if !editing {
            self.window_switch_focus.paste_capture = Some(owner);
        }
    }

    /// Whether a clipboard paste now creates a new paste instead of entering the refocused editor.
    ///
    /// # Returns
    /// `true` from reactivation until the selected paste's editor sees other input.
    pub(super) fn paste_capture_pending(&self) -> bool {
        self.window_switch_focus
            .paste_capture
            .as_deref()
            .is_some_and(|id| self.selected_id.as_deref() == Some(id))
    }

    /// Ends the post-switch capture once a clipboard paste has used it.
    ///
    /// # Arguments
    /// - `ctx`: Context whose editor focus is released.
    /// - `release_editor`: Leave the editor unfocused, as a paste routed to a
    ///   new paste expects; an append to the open paste keeps it focused.
    pub(super) fn finish_paste_capture(&mut self, ctx: &egui::Context, release_editor: bool) {
        self.window_switch_focus.paste_capture = None;
        if release_editor {
            self.release_editor_keyboard(ctx);
        }
    }

    fn release_editor_keyboard(&mut self, ctx: &egui::Context) {
        self.virtual_editor_state.has_focus = false;
        self.focus_editor_next = false;
        ctx.memory_mut(|memory| memory.surrender_focus(egui::Id::new(VIRTUAL_EDITOR_ID)));
    }
}

/// Whether an input event shows the user editing rather than pasting the clipboard.
fn ends_paste_capture(event: &egui::Event) -> bool {
    match event {
        egui::Event::Key { pressed: true, .. } => !matches!(
            runtime_shortcut_action(event),
            Some(RuntimeShortcutAction::PlainPaste | RuntimeShortcutAction::PasteIntoEditor)
        ),
        egui::Event::PointerButton { pressed: true, .. }
        | egui::Event::Text(_)
        | egui::Event::Cut
        | egui::Event::Ime(egui::ImeEvent::Preedit(_) | egui::ImeEvent::Commit(_)) => true,
        _ => false,
    }
}
