//! Paste-intent helpers for explicit "paste as new" routing.

use super::*;

/// Keyboard ownership state used to route plain paste shortcuts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PlainPasteFocusState {
    EditorFocused,
    OtherInputFocused,
    Unfocused,
}

/// Focus context used to decide whether the global delete shortcut is safe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DeleteShortcutFocusState {
    OtherInputFocused,
    EditorFocused,
    EditorFocusPromotionPending,
    Unfocused,
}

/// Clipboard acceptance policy for creating new paste entries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ClipboardCreatePolicy {
    // Explicit Ctrl/Cmd+Shift+V intent should preserve whitespace-only payloads.
    ExplicitPasteAsNew,
    // Implicit global Ctrl/Cmd+V-to-new-paste keeps existing non-whitespace gate.
    ImplicitGlobalShortcut,
}

impl LocalPasteApp {
    /// Merges a newly observed paste payload into the current frame snapshot.
    ///
    /// Keeps the most complete payload deterministically so shorter/partial
    /// duplicates cannot replace fuller clipboard text.
    ///
    /// # Arguments
    /// - `observed`: In/out frame-local paste payload accumulator.
    /// - `candidate`: Newly observed clipboard text candidate.
    pub(super) fn merge_pasted_text(observed: &mut Option<String>, candidate: &str) {
        let Some(current) = observed.as_mut() else {
            *observed = Some(candidate.to_string());
            return;
        };
        if current == candidate {
            return;
        }

        // Prefer cheap length checks before expensive substring scans on large payloads.
        match candidate.len().cmp(&current.len()) {
            std::cmp::Ordering::Greater => *current = candidate.to_string(),
            std::cmp::Ordering::Less => {}
            std::cmp::Ordering::Equal => {
                // Equal byte lengths but different strings: prefer more scalar values
                // (rare UTF-8 tie-breaker), otherwise keep the existing payload.
                if candidate.chars().count() > current.chars().count() {
                    *current = candidate.to_string();
                }
            }
        }
    }

    /// Returns whether clipboard text should create a new paste.
    ///
    /// # Arguments
    /// - `text`: Clipboard payload text to evaluate.
    /// - `policy`: Routing intent that determines whitespace handling.
    ///
    /// # Returns
    /// `true` when the payload qualifies under the selected policy.
    pub(super) fn should_create_paste_from_clipboard(
        text: &str,
        policy: ClipboardCreatePolicy,
    ) -> bool {
        match policy {
            ClipboardCreatePolicy::ExplicitPasteAsNew => !text.is_empty(),
            ClipboardCreatePolicy::ImplicitGlobalShortcut => !text.trim().is_empty(),
        }
    }

    /// Clears any pending explicit "paste as new" intent state.
    pub(super) fn cancel_paste_as_new_intent(&mut self) {
        self.paste_as_new_pending_frames = 0;
        self.paste_as_new_clipboard_requested_at = None;
    }

    /// Arms the short-lived "paste as new" intent window.
    pub(super) fn arm_paste_as_new_intent(&mut self) {
        self.paste_as_new_pending_frames = PASTE_AS_NEW_PENDING_TTL_FRAMES;
        self.paste_as_new_clipboard_requested_at = None;
    }

    /// Requests system paste and marks the result to be routed as new paste content.
    ///
    /// # Arguments
    /// - `ctx`: Egui context used to dispatch viewport paste requests.
    pub(super) fn request_paste_as_new(&mut self, ctx: &egui::Context) {
        self.arm_paste_as_new_intent();
        self.paste_as_new_clipboard_requested_at = Some(Instant::now());
        ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
    }

    /// Arms explicit "paste as new" intent when command+shift+V is observed this frame.
    ///
    /// # Arguments
    /// - `ctx`: Egui context used to inspect current-frame input events.
    ///
    /// # Returns
    /// `true` when the explicit shortcut was observed and intent was armed.
    pub(super) fn maybe_arm_paste_as_new_shortcut_intent(&mut self, ctx: &egui::Context) -> bool {
        let explicit_shortcut = ctx.input(|input| {
            input.events.iter().any(|event| {
                matches!(
                    event,
                    egui::Event::Key {
                        key: egui::Key::V,
                        pressed: true,
                        modifiers,
                        ..
                    } if is_command_shift_shortcut(*modifiers)
                )
            })
        });
        if explicit_shortcut {
            self.arm_paste_as_new_intent();
        }
        explicit_shortcut
    }

    /// Returns whether a virtual paste command should be skipped due to explicit paste-as-new intent.
    ///
    /// # Arguments
    /// - `command`: Candidate virtual editor command for this frame.
    ///
    /// # Returns
    /// `true` when a pending explicit paste-as-new intent should consume the paste event instead.
    pub(super) fn should_skip_virtual_command_for_paste_as_new(
        &self,
        command: &VirtualInputCommand,
    ) -> bool {
        self.paste_as_new_pending_frames > 0 && matches!(command, VirtualInputCommand::Paste(_))
    }

    /// Consumes a pending explicit paste-as-new intent and dispatches create when clipboard text exists.
    ///
    /// # Arguments
    /// - `pasted_text`: Optional clipboard text captured from current-frame egui events.
    /// # Returns
    /// `true` when clipboard text was consumed and routed into `CreatePaste`.
    pub(super) fn maybe_consume_explicit_paste_as_new(
        &mut self,
        pasted_text: &mut Option<String>,
    ) -> bool {
        if self.paste_as_new_pending_frames == 0 {
            self.paste_as_new_clipboard_requested_at = None;
            return false;
        }
        if let Some(text) = pasted_text.take() {
            self.cancel_paste_as_new_intent();
            if Self::should_create_paste_from_clipboard(
                text.as_str(),
                ClipboardCreatePolicy::ExplicitPasteAsNew,
            ) {
                self.create_new_paste_with_content(text);
                return true;
            }
            self.set_status("Clipboard was empty.");
            return false;
        }
        if let Some(request_started_at) = self.paste_as_new_clipboard_requested_at {
            // Keep explicit intent armed while RequestPaste is in flight; otherwise a slow
            // clipboard backend can expire intent before the payload arrives.
            if request_started_at.elapsed() < PASTE_AS_NEW_CLIPBOARD_WAIT_TIMEOUT {
                return false;
            }
            self.cancel_paste_as_new_intent();
            self.set_status("Paste-as-new clipboard request timed out; try again.");
            return false;
        }
        self.paste_as_new_pending_frames = self.paste_as_new_pending_frames.saturating_sub(1);
        false
    }

    /// Decides whether paste-as-new should request clipboard text from the viewport.
    ///
    /// # Arguments
    /// - `request_paste_as_new`: Whether paste-as-new routing is requested this frame.
    /// - `pasted_text`: Clipboard payload already observed in this frame, if any.
    ///
    /// # Returns
    /// `true` when a viewport paste request is still needed to fetch clipboard text.
    pub(super) fn should_request_viewport_paste_for_new(
        &self,
        request_paste_as_new: bool,
        pasted_text: Option<&str>,
    ) -> bool {
        request_paste_as_new && pasted_text.is_none()
    }

    /// Routes plain paste shortcut behavior based on editor-focus state.
    ///
    /// # Arguments
    /// - `focus_state`: Keyboard ownership state for this frame.
    /// - `saw_virtual_paste`: Whether virtual command extraction already observed paste.
    ///
    /// # Returns
    /// Tuple of `(request_virtual_paste, request_new_paste)`.
    pub(super) fn route_plain_paste_shortcut(
        &self,
        focus_state: PlainPasteFocusState,
        saw_virtual_paste: bool,
    ) -> (bool, bool) {
        match focus_state {
            PlainPasteFocusState::EditorFocused => (!saw_virtual_paste, false),
            // Respect focused non-editor text inputs (search, palette query, metadata fields).
            PlainPasteFocusState::OtherInputFocused => (false, false),
            PlainPasteFocusState::Unfocused => (false, true),
        }
    }

    /// Resolves plain paste shortcut requests from post-layout focus state.
    ///
    /// # Arguments
    /// - `shortcut_pressed`: Whether plain command+V was pressed this frame.
    /// - `focus_state`: Keyboard ownership state after layout.
    /// - `saw_virtual_paste`: Whether virtual command extraction already observed paste.
    ///
    /// # Returns
    /// Tuple of `(request_virtual_paste, request_new_paste)`.
    pub(super) fn resolve_plain_paste_shortcut_request(
        &self,
        shortcut_pressed: bool,
        focus_state: PlainPasteFocusState,
        saw_virtual_paste: bool,
    ) -> (bool, bool) {
        if !shortcut_pressed {
            return (false, false);
        }
        self.route_plain_paste_shortcut(focus_state, saw_virtual_paste)
    }

    /// Derives plain-paste keyboard ownership state from editor and egui focus snapshots.
    ///
    /// # Arguments
    /// - `editor_focus_active`: Whether the virtual editor currently owns focus.
    /// - `wants_keyboard_input`: Whether egui reports focused keyboard input elsewhere.
    ///
    /// # Returns
    /// A [`PlainPasteFocusState`] used by plain shortcut routing.
    pub(super) fn plain_paste_focus_state(
        editor_focus_active: bool,
        wants_keyboard_input: bool,
    ) -> PlainPasteFocusState {
        if editor_focus_active {
            PlainPasteFocusState::EditorFocused
        } else if wants_keyboard_input {
            PlainPasteFocusState::OtherInputFocused
        } else {
            PlainPasteFocusState::Unfocused
        }
    }

    /// Returns whether the global delete-selected shortcut should run this frame.
    ///
    /// # Arguments
    /// - `focus_state`: Focus/ownership context for the current shortcut frame.
    ///
    /// # Returns
    /// `true` only when no text-input context owns keyboard input.
    pub(super) fn should_route_delete_selected_shortcut(
        &self,
        focus_state: DeleteShortcutFocusState,
    ) -> bool {
        matches!(focus_state, DeleteShortcutFocusState::Unfocused)
    }

    /// Derives delete-shortcut focus context from input/focus state snapshots.
    ///
    /// # Arguments
    /// - `wants_keyboard_input`: Whether any text-input widget currently owns keyboard capture.
    /// - `virtual_editor_focus_active`: Whether virtual editor text focus is active.
    /// - `focus_promotion_requested`: Whether editor focus is scheduled to promote this frame.
    ///
    /// # Returns
    /// A [`DeleteShortcutFocusState`] used to guard global delete behavior.
    pub(super) fn delete_shortcut_focus_state(
        wants_keyboard_input: bool,
        virtual_editor_focus_active: bool,
        focus_promotion_requested: bool,
    ) -> DeleteShortcutFocusState {
        if wants_keyboard_input {
            DeleteShortcutFocusState::OtherInputFocused
        } else if virtual_editor_focus_active {
            DeleteShortcutFocusState::EditorFocused
        } else if focus_promotion_requested {
            DeleteShortcutFocusState::EditorFocusPromotionPending
        } else {
            DeleteShortcutFocusState::Unfocused
        }
    }
}
