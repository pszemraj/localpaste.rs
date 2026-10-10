//! Paste-intent helpers for new-paste and open-paste clipboard routing.

use super::*;

/// Keyboard ownership state used to route global shortcuts around text inputs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum KeyboardFocusState {
    EditorFocused,
    OtherInputFocused,
    Unfocused,
}

/// Clipboard acceptance policy for creating new paste entries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ClipboardCreatePolicy {
    // The explicit palette action should preserve whitespace-only payloads.
    ExplicitPasteAsNew,
    // Implicit global Ctrl/Cmd+V-to-new-paste keeps existing non-whitespace gate.
    ImplicitGlobalShortcut,
}

impl LocalPasteApp {
    /// Captures native paste modifiers before discovery splits input across frames.
    ///
    /// Egui-winit replaces Command+V with Paste, whose text carries no modifiers.
    /// An ordered key marker preserves the observed chord when a deferred slice
    /// is replayed after Command or Shift has been released.
    ///
    /// # Arguments
    /// - `input`: Native events to stage, with their current modifier snapshot.
    pub(super) fn stage_native_paste_shortcuts(input: &mut egui::RawInput) {
        if input.events.iter().any(|event| {
            matches!(
                shortcuts::runtime_shortcut_action(event),
                Some(RuntimeShortcutAction::PlainPaste | RuntimeShortcutAction::PasteIntoEditor)
            )
        }) {
            return;
        }
        let mut events = Vec::with_capacity(input.events.len());
        for event in std::mem::take(&mut input.events) {
            if shortcuts::native_paste_shortcut_action(&event, input.modifiers).is_some() {
                events.push(egui::Event::Key {
                    key: egui::Key::V,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: input.modifiers,
                });
            }
            events.push(event);
        }
        input.events = events;
    }

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
        if let Some(requested_at) = self.paste_as_new_clipboard_requested_at.take() {
            self.canceled_paste_request_at = Some(requested_at);
        }
    }

    /// Discards a canceled native clipboard reply before any text widget sees it.
    ///
    /// Egui paste events carry text without request identity. A newer observed
    /// paste shortcut therefore supersedes the canceled request; otherwise its
    /// next payload without a recognized shortcut is discarded within the wait window.
    /// A reply arriving with Command held is indistinguishable from a fresh
    /// native shortcut, so the observed shortcut takes precedence.
    ///
    /// # Arguments
    /// - `ctx`: Context whose native paste events are filtered before rendering.
    pub(super) fn discard_canceled_clipboard_reply(&mut self, ctx: &egui::Context) {
        if self.keyboard_overlay_open() || self.mutation_shortcut_block_reason().is_some() {
            self.cancel_paste_as_new_intent();
        }
        let Some(requested_at) = self.canceled_paste_request_at else {
            return;
        };
        ctx.input_mut(|input| {
            if requested_at.elapsed() >= PASTE_AS_NEW_CLIPBOARD_WAIT_TIMEOUT {
                self.canceled_paste_request_at = None;
                return;
            }
            let has_paste_key = input.events.iter().any(|event| {
                matches!(event, egui::Event::Key {
                    key: egui::Key::V, pressed: true, modifiers, ..
                } if modifiers.command)
            });
            let native_modifiers = input.modifiers;
            let mut discarded = false;
            input.events.retain(|event| {
                if matches!(event, egui::Event::Key {
                    key: egui::Key::V, pressed: true, modifiers, ..
                } if modifiers.command)
                    || (!has_paste_key
                        && shortcuts::native_paste_shortcut_action(event, native_modifiers)
                            .is_some())
                {
                    self.canceled_paste_request_at = None;
                }
                if self.canceled_paste_request_at.is_some()
                    && matches!(event, egui::Event::Paste(_))
                {
                    discarded = true;
                    return false;
                }
                true
            });
            if discarded {
                self.canceled_paste_request_at = None;
            }
        });
    }

    /// Arms the short-lived "paste as new" intent window.
    pub(super) fn arm_paste_as_new_intent(&mut self) {
        self.canceled_paste_request_at = None;
        self.paste_as_new_pending_frames = PASTE_AS_NEW_PENDING_TTL_FRAMES;
        self.paste_as_new_clipboard_requested_at = None;
    }

    /// Requests system paste and marks the result to be routed as new paste content.
    ///
    /// # Arguments
    /// - `ctx`: Egui context used to dispatch viewport paste requests.
    pub(super) fn request_paste_as_new(&mut self, ctx: &egui::Context) {
        if self.mutation_shortcut_block_reason().is_some() {
            self.set_mutation_shortcut_blocked_status();
            return;
        }
        self.arm_paste_as_new_intent();
        self.paste_as_new_clipboard_requested_at = Some(Instant::now());
        ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
    }

    /// Inserts clipboard text into the open paste regardless of keyboard focus.
    ///
    /// With no loaded paste, the text is appended to the loading selection or,
    /// failing that, to the top sidebar paste once it loads. An empty list
    /// leaves nothing to insert into, so the text becomes a new paste.
    ///
    /// # Arguments
    /// - `ctx`: Context used to apply the editor edit and request focus.
    /// - `text`: Clipboard payload observed with `Ctrl/Cmd+Shift+V`.
    pub(super) fn paste_into_editor(&mut self, ctx: &egui::Context, text: String) {
        if text.is_empty() {
            return;
        }
        // Selection switches clear the loaded paste until its replacement arrives.
        if self.selected_paste.is_some() {
            self.pending_editor_paste = None;
            self.apply_editor_paste(ctx, &[VirtualInputCommand::Paste(text)]);
            return;
        }
        let target = self
            .selected_id
            .clone()
            .or_else(|| self.pastes.first().map(|paste| paste.id.clone()));
        let Some(id) = target else {
            self.create_new_paste_with_content(text);
            return;
        };
        if self.selected_id.as_deref() != Some(id.as_str()) && !self.select_paste(id.clone()) {
            return;
        }
        self.pending_editor_paste = Some(PendingEditorPaste { id, text });
    }

    /// Appends a deferred `Ctrl/Cmd+Shift+V` payload once its target paste loads.
    ///
    /// The payload is dropped when selection moves to a different paste first.
    ///
    /// # Arguments
    /// - `ctx`: Context used to apply the editor edit and request focus.
    pub(super) fn maybe_apply_pending_editor_paste(&mut self, ctx: &egui::Context) {
        let Some(pending) = self.pending_editor_paste.as_ref() else {
            return;
        };
        let target = Some(pending.id.as_str());
        if self.selected_id.as_deref() != target {
            if self.pending_selection_id.as_deref() != target {
                self.pending_editor_paste = None;
            }
            return;
        }
        if self.selected_paste.as_ref().map(|paste| paste.id.as_str()) != target
            || self.editor_shortcuts_blocked()
        {
            return;
        }
        let Some(pending) = self.pending_editor_paste.take() else {
            return;
        };
        let mut text = pending.text;
        let len = self.virtual_editor_buffer.len_chars();
        if len > 0
            && !matches!(
                self.virtual_editor_buffer
                    .slice_chars(len - 1..len)
                    .as_str(),
                "\n" | "\r"
            )
        {
            // Appended text starts after the last line rather than extending it.
            text.insert(0, '\n');
        }
        self.apply_editor_paste(
            ctx,
            &[
                VirtualInputCommand::MoveDocEnd { select: false },
                VirtualInputCommand::Paste(text),
            ],
        );
    }

    fn apply_editor_paste(&mut self, ctx: &egui::Context, commands: &[VirtualInputCommand]) {
        if self.apply_virtual_commands(ctx, commands).changed {
            self.mark_dirty();
        }
        self.focus_editor_next = true;
        ctx.request_repaint();
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
        if self.mutation_shortcut_block_reason().is_some() || self.keyboard_overlay_open() {
            // Overlay ownership and reset fences also cancel older clipboard
            // intents whose payload arrives after the overlay opens.
            self.cancel_paste_as_new_intent();
            return false;
        }
        if let Some(text) = pasted_text.take() {
            // The reply has arrived; completing it must not leave a canceled
            // request marker that would discard the next independent paste.
            self.paste_as_new_clipboard_requested_at = None;
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

    /// Routes an unfocused paste event into a new paste when the global contract allows it.
    ///
    /// # Arguments
    /// - `pasted_text`: Clipboard payload observed for this frame, if any.
    /// - `editor_focus_active`: Whether the virtual editor currently owns focus.
    /// - `wants_keyboard_input`: Whether egui reports focused text input elsewhere.
    /// - `virtual_paste_consumed`: Whether the editor already consumed the paste event.
    ///
    /// # Returns
    /// `true` when the clipboard payload was turned into `CreatePaste`.
    pub(super) fn maybe_route_implicit_global_clipboard_create(
        &mut self,
        pasted_text: Option<String>,
        editor_focus_active: bool,
        wants_keyboard_input: bool,
        virtual_paste_consumed: bool,
    ) -> bool {
        if self.mutation_shortcut_block_reason().is_some()
            || self.keyboard_overlay_open()
            || editor_focus_active
            || wants_keyboard_input
            || virtual_paste_consumed
        {
            return false;
        }
        let Some(text) = pasted_text else {
            return false;
        };
        if Self::should_create_paste_from_clipboard(
            text.as_str(),
            ClipboardCreatePolicy::ImplicitGlobalShortcut,
        ) {
            self.create_new_paste_with_content(text);
            return true;
        }
        false
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
        focus_state: KeyboardFocusState,
        saw_virtual_paste: bool,
    ) -> (bool, bool) {
        if self.keyboard_overlay_open() {
            return (false, false);
        }
        match focus_state {
            KeyboardFocusState::EditorFocused => (!saw_virtual_paste, false),
            // Respect focused non-editor text inputs (search, palette query, metadata fields).
            KeyboardFocusState::OtherInputFocused => (false, false),
            KeyboardFocusState::Unfocused => (false, true),
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
        focus_state: KeyboardFocusState,
        saw_virtual_paste: bool,
    ) -> (bool, bool) {
        if !shortcut_pressed {
            return (false, false);
        }
        self.route_plain_paste_shortcut(focus_state, saw_virtual_paste)
    }

    /// Derives keyboard ownership state from editor and egui focus snapshots.
    ///
    /// # Arguments
    /// - `editor_focus_active`: Whether the virtual editor currently owns focus.
    /// - `wants_keyboard_input`: Whether egui reports focused keyboard input elsewhere.
    ///
    /// # Returns
    /// A [`KeyboardFocusState`] used by global shortcut routing.
    pub(super) fn keyboard_focus_state(
        editor_focus_active: bool,
        wants_keyboard_input: bool,
    ) -> KeyboardFocusState {
        if editor_focus_active {
            KeyboardFocusState::EditorFocused
        } else if wants_keyboard_input {
            KeyboardFocusState::OtherInputFocused
        } else {
            KeyboardFocusState::Unfocused
        }
    }

    /// Returns whether the global delete-selected shortcut should run this frame.
    ///
    /// When the virtual editor owns keyboard focus, `Ctrl/Cmd+Delete` belongs to
    /// native text-editing behavior instead of deleting the selected paste.
    ///
    /// # Arguments
    /// - `focus_state`: Focus/ownership context for the current shortcut frame.
    ///
    /// # Returns
    /// `true` only when no text-input context owns keyboard input.
    pub(super) fn should_route_delete_selected_shortcut(
        &self,
        focus_state: KeyboardFocusState,
    ) -> bool {
        matches!(focus_state, KeyboardFocusState::Unfocused)
    }
}
