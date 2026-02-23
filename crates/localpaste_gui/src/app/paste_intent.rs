//! Paste-intent helpers for explicit "paste as new" routing.

use super::*;

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

        let candidate_contains_current = candidate.contains(current.as_str());
        let current_contains_candidate = current.contains(candidate);
        if candidate_contains_current && !current_contains_candidate {
            *current = candidate.to_string();
            return;
        }
        if current_contains_candidate && !candidate_contains_current {
            return;
        }

        let candidate_chars = candidate.chars().count();
        let current_chars = current.chars().count();
        if candidate_chars > current_chars
            || (candidate_chars == current_chars && candidate.len() > current.len())
        {
            *current = candidate.to_string();
        }
    }

    /// Returns whether clipboard text should create a new paste.
    ///
    /// # Arguments
    /// - `text`: Clipboard payload text to evaluate.
    ///
    /// # Returns
    /// `true` when the payload contains at least one non-whitespace character.
    pub(super) fn should_create_paste_from_clipboard(text: &str) -> bool {
        !text.trim().is_empty()
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
            if Self::should_create_paste_from_clipboard(text.as_str()) {
                self.create_new_paste_with_content(text);
                return true;
            }
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

    /// Routes plain paste shortcut behavior based on current editor mode/focus state.
    ///
    /// # Arguments
    /// - `editor_focus_pre`: Whether the active editor owned focus before routing.
    /// - `saw_virtual_paste`: Whether virtual command extraction already observed paste.
    /// - `wants_keyboard_input_before`: Whether egui already assigned keyboard input to a focused widget.
    ///
    /// # Returns
    /// Tuple of `(request_virtual_paste, request_new_paste)`.
    pub(super) fn route_plain_paste_shortcut(
        &self,
        editor_focus_pre: bool,
        saw_virtual_paste: bool,
        wants_keyboard_input_before: bool,
    ) -> (bool, bool) {
        if self.editor_mode == EditorMode::VirtualEditor && editor_focus_pre {
            (!saw_virtual_paste, false)
        } else if wants_keyboard_input_before {
            // Respect focused non-editor text inputs (search, palette query, metadata fields).
            (false, false)
        } else if !editor_focus_pre {
            (false, true)
        } else {
            (false, false)
        }
    }

    /// Resolves plain paste shortcut requests from post-layout focus state.
    ///
    /// # Arguments
    /// - `shortcut_pressed`: Whether plain command+V was pressed this frame.
    /// - `editor_focus_post`: Whether the active editor owns focus after layout.
    /// - `saw_virtual_paste`: Whether virtual command extraction already observed paste.
    /// - `wants_keyboard_input_post`: Whether egui assigned keyboard input after layout.
    ///
    /// # Returns
    /// Tuple of `(request_virtual_paste, request_new_paste)`.
    pub(super) fn resolve_plain_paste_shortcut_request(
        &self,
        shortcut_pressed: bool,
        editor_focus_post: bool,
        saw_virtual_paste: bool,
        wants_keyboard_input_post: bool,
    ) -> (bool, bool) {
        if !shortcut_pressed {
            return (false, false);
        }
        self.route_plain_paste_shortcut(
            editor_focus_post,
            saw_virtual_paste,
            wants_keyboard_input_post,
        )
    }

    /// Returns whether the global delete-selected shortcut should run this frame.
    ///
    /// # Arguments
    /// - `wants_keyboard_input`: Whether an input widget currently owns keyboard capture.
    /// - `virtual_editor_focus_active`: Whether virtual editor focus is active for text input.
    ///
    /// # Returns
    /// `true` only when no text-input context owns keyboard input.
    pub(super) fn should_route_delete_selected_shortcut(
        &self,
        wants_keyboard_input: bool,
        virtual_editor_focus_active: bool,
    ) -> bool {
        if wants_keyboard_input {
            return false;
        }
        !(self.editor_mode == EditorMode::VirtualEditor && virtual_editor_focus_active)
    }
}
