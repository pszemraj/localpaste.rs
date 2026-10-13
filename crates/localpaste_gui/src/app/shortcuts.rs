//! Runtime shortcut registry shared by input dispatch and discoverability UI.

use super::interaction_helpers::{is_command_shift_shortcut, is_plain_command_shortcut};
use super::virtual_editor::uses_mac_shortcuts;
use eframe::egui;

/// Runtime shortcut actions handled by the app frame loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum RuntimeShortcutAction {
    NewPaste,
    Save,
    DeleteSelected,
    FocusSearch,
    /// The palette's primary chord, `Ctrl/Cmd+K`, which hints and help show.
    ToggleCommandPalette,
    /// The palette's `Ctrl/Cmd+Shift+P` alias.
    ToggleCommandPaletteLegacy,
    TogglePastePicker,
    ToggleProperties,
    PlainPaste,
    PasteIntoEditor,
    ToggleShortcutHelp,
}

/// User-facing shortcut row content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ShortcutHelpEntry {
    pub(crate) keys: &'static str,
    pub(crate) description: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShortcutChord {
    PlainCommand(egui::Key),
    /// A plain command chord that macOS also accepts on a second, native key.
    PlainCommandOrMac {
        key: egui::Key,
        mac_key: egui::Key,
    },
    CommandShift(egui::Key),
    AnyModifier(egui::Key),
}

/// A shortcut with both executable matching data and display metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RuntimeShortcut {
    pub(crate) action: RuntimeShortcutAction,
    chord: ShortcutChord,
    help: ShortcutHelpEntry,
}

impl RuntimeShortcut {
    /// Returns the user-facing help entry for this executable shortcut.
    ///
    /// # Returns
    /// Display metadata for rendering shortcut help rows.
    pub(crate) const fn help(self) -> ShortcutHelpEntry {
        self.help
    }

    fn matches_event(self, event: &egui::Event, mac: bool) -> bool {
        let egui::Event::Key {
            key,
            pressed: true,
            modifiers,
            ..
        } = event
        else {
            return false;
        };
        match self.chord {
            ShortcutChord::PlainCommand(expected) => {
                *key == expected && is_plain_command_shortcut(*modifiers)
            }
            ShortcutChord::PlainCommandOrMac {
                key: expected,
                mac_key,
            } => {
                (*key == expected || (mac && *key == mac_key))
                    && is_plain_command_shortcut(*modifiers)
            }
            ShortcutChord::CommandShift(expected) => {
                *key == expected && is_command_shift_shortcut(*modifiers)
            }
            ShortcutChord::AnyModifier(expected) => *key == expected,
        }
    }
}

/// App-level shortcuts that are both dispatched by the frame loop and shown in help UI.
pub(crate) const RUNTIME_SHORTCUTS: &[RuntimeShortcut] = &[
    RuntimeShortcut {
        action: RuntimeShortcutAction::NewPaste,
        chord: ShortcutChord::PlainCommand(egui::Key::N),
        help: ShortcutHelpEntry {
            keys: "Ctrl/Cmd+N",
            description: "Create new paste",
        },
    },
    RuntimeShortcut {
        action: RuntimeShortcutAction::Save,
        chord: ShortcutChord::PlainCommand(egui::Key::S),
        help: ShortcutHelpEntry {
            keys: "Ctrl/Cmd+S",
            description: "Save content and metadata",
        },
    },
    RuntimeShortcut {
        action: RuntimeShortcutAction::DeleteSelected,
        // MacBook keyboards have no forward Delete key, and Cmd+Backspace is the
        // native chord for deleting a selected item there. Elsewhere
        // Ctrl+Backspace deletes a word, so the alias is macOS-only.
        chord: ShortcutChord::PlainCommandOrMac {
            key: egui::Key::Delete,
            mac_key: egui::Key::Backspace,
        },
        help: ShortcutHelpEntry {
            keys: "Ctrl+Delete (Win/Linux) or Cmd+Backspace/Delete (macOS)",
            description: "Delete selected paste when text inputs are unfocused",
        },
    },
    RuntimeShortcut {
        action: RuntimeShortcutAction::FocusSearch,
        chord: ShortcutChord::PlainCommand(egui::Key::F),
        help: ShortcutHelpEntry {
            keys: "Ctrl/Cmd+F",
            description: "Focus sidebar search",
        },
    },
    RuntimeShortcut {
        action: RuntimeShortcutAction::ToggleCommandPalette,
        chord: ShortcutChord::PlainCommand(egui::Key::K),
        help: ShortcutHelpEntry {
            keys: "Ctrl/Cmd+K",
            description: "Toggle command palette",
        },
    },
    RuntimeShortcut {
        action: RuntimeShortcutAction::ToggleCommandPaletteLegacy,
        chord: ShortcutChord::CommandShift(egui::Key::P),
        help: ShortcutHelpEntry {
            keys: "Ctrl/Cmd+Shift+P",
            description: "Toggle command palette",
        },
    },
    RuntimeShortcut {
        action: RuntimeShortcutAction::TogglePastePicker,
        chord: ShortcutChord::CommandShift(egui::Key::K),
        help: ShortcutHelpEntry {
            keys: "Ctrl/Cmd+Shift+K",
            description: "Toggle paste picker",
        },
    },
    RuntimeShortcut {
        action: RuntimeShortcutAction::ToggleProperties,
        chord: ShortcutChord::PlainCommand(egui::Key::I),
        help: ShortcutHelpEntry {
            keys: "Ctrl/Cmd+I",
            description: "Toggle properties drawer",
        },
    },
    RuntimeShortcut {
        action: RuntimeShortcutAction::PlainPaste,
        chord: ShortcutChord::PlainCommand(egui::Key::V),
        help: ShortcutHelpEntry {
            keys: "Ctrl/Cmd+V",
            description: "Paste into focused text; otherwise create new paste",
        },
    },
    RuntimeShortcut {
        action: RuntimeShortcutAction::PasteIntoEditor,
        chord: ShortcutChord::CommandShift(egui::Key::V),
        help: ShortcutHelpEntry {
            keys: "Ctrl/Cmd+Shift+V",
            description: "Paste into the open paste",
        },
    },
    RuntimeShortcut {
        action: RuntimeShortcutAction::ToggleShortcutHelp,
        chord: ShortcutChord::AnyModifier(egui::Key::F1),
        help: ShortcutHelpEntry {
            keys: "F1",
            description: "Toggle keyboard shortcut help",
        },
    },
];

/// Returns all registered runtime shortcut actions pressed in the current input frame.
///
/// # Returns
/// Iterator of shortcut actions whose registered chords were pressed.
pub(crate) fn pressed_runtime_shortcuts(
    input: &egui::InputState,
) -> impl Iterator<Item = RuntimeShortcutAction> + '_ {
    // The native hook places a key marker immediately before each Paste, using
    // its ordered modifier state before discovery can defer either event.
    input.events.iter().filter_map(runtime_shortcut_action)
}

/// Matches egui-winit's paste-only shortcut representation.
///
/// # Arguments
/// - `event`: Native event, which has no embedded modifiers for paste.
/// - `modifiers`: Modifier state at this event's position in the native stream.
///
/// # Returns
/// Paste action inferred from Command and Shift, or `None` for other input.
pub(crate) fn native_paste_shortcut_action(
    event: &egui::Event,
    modifiers: egui::Modifiers,
) -> Option<RuntimeShortcutAction> {
    if !matches!(event, egui::Event::Paste(_)) {
        return None;
    }
    if is_command_shift_shortcut(modifiers) {
        Some(RuntimeShortcutAction::PasteIntoEditor)
    } else if is_plain_command_shortcut(modifiers) {
        Some(RuntimeShortcutAction::PlainPaste)
    } else {
        None
    }
}

/// Matches one key event without changing the order or multiplicity of chords.
///
/// # Returns
/// The registered action for a pressed shortcut event, or `None` for other input.
pub(crate) fn runtime_shortcut_action(event: &egui::Event) -> Option<RuntimeShortcutAction> {
    let mac = uses_mac_shortcuts();
    RUNTIME_SHORTCUTS
        .iter()
        .find(|shortcut| shortcut.matches_event(event, mac))
        .map(|shortcut| shortcut.action)
}

/// Looks up the display label for a registered runtime shortcut action.
///
/// The label is spelled for the platform whose chords dispatch matches.
///
/// # Returns
/// Human-readable shortcut key label for the action.
///
/// # Panics
/// Panics if a [`RuntimeShortcutAction`] variant is not present in [`RUNTIME_SHORTCUTS`],
/// or if its label names no chord for the active platform.
pub(crate) fn runtime_shortcut_label(action: RuntimeShortcutAction) -> String {
    RUNTIME_SHORTCUTS
        .iter()
        .find(|shortcut| shortcut.action == action)
        .and_then(|shortcut| platform_shortcut_keys(shortcut.help.keys, uses_mac_shortcuts()))
        .expect("runtime shortcut action must be registered for this platform")
}

/// Selects the native key spelling from a registry label shared across platforms.
///
/// # Arguments
/// - `keys`: Registry label using `Ctrl/Cmd`, `X (Win/Linux) or Y (macOS)`, or a
///   `(macOS)` suffix for a chord that exists only there.
/// - `macos`: Whether to spell the chord for macOS.
///
/// # Returns
/// The label for that platform, or `None` when the chord does not exist there.
pub(crate) fn platform_shortcut_keys(keys: &str, macos: bool) -> Option<String> {
    if let Some((other, mac)) = keys.split_once(" (Win/Linux) or ") {
        return Some(
            if macos {
                mac.trim_end_matches(" (macOS)")
            } else {
                other
            }
            .to_string(),
        );
    }
    if let Some(mac) = keys.strip_suffix(" (macOS)") {
        return macos.then(|| mac.to_string());
    }
    Some(keys.replace("Ctrl/Cmd", if macos { "Cmd" } else { "Ctrl" }))
}

/// A group of shared navigation/editing shortcut descriptions.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ShortcutSection {
    pub(crate) title: &'static str,
    pub(crate) entries: &'static [ShortcutHelpEntry],
}

const NAVIGATION_SHORTCUTS: &[ShortcutHelpEntry] = &[
    ShortcutHelpEntry {
        keys: "Home/End (Win/Linux) or Cmd+Left/Right (macOS)",
        description: "Move caret to line start/end",
    },
    ShortcutHelpEntry {
        keys: "Ctrl+Home/End (Win/Linux) or Cmd+Up/Down/Home/End (macOS)",
        description: "Move caret to document start/end",
    },
];

const EDITING_SHORTCUTS: &[ShortcutHelpEntry] = &[
    ShortcutHelpEntry {
        keys: "Tab / Shift+Tab",
        description: "Indent / unindent selected lines",
    },
    ShortcutHelpEntry {
        keys: "Ctrl+Left/Right (Win/Linux) or Option+Left/Right (macOS)",
        description: "Move caret by word",
    },
    ShortcutHelpEntry {
        keys: "Ctrl+Backspace/Delete (Win/Linux) or Option+Backspace/Delete (macOS)",
        description: "Delete one word backward/forward",
    },
    ShortcutHelpEntry {
        keys: "Cmd+Backspace / Ctrl+K (macOS)",
        description: "Delete to line start / end",
    },
];

const FIND_SHORTCUTS: &[ShortcutHelpEntry] = &[ShortcutHelpEntry {
    keys: "Enter / Shift+Enter",
    description: "Next / previous match in Find",
}];

/// Non-global editor and navigation chords shared by shortcut discovery.
pub(crate) const STATIC_SHORTCUT_SECTIONS: &[ShortcutSection] = &[
    ShortcutSection {
        title: "Navigation",
        entries: NAVIGATION_SHORTCUTS,
    },
    ShortcutSection {
        title: "Editing",
        entries: EDITING_SHORTCUTS,
    },
    ShortcutSection {
        title: "Find",
        entries: FIND_SHORTCUTS,
    },
];

#[cfg(test)]
mod tests {
    use super::super::tests::run_ui;
    use super::*;
    use std::collections::HashSet;

    fn pressed_actions(key: egui::Key, modifiers: egui::Modifiers) -> Vec<RuntimeShortcutAction> {
        let ctx = egui::Context::default();
        let mut actions = Vec::new();
        let _ = run_ui(
            &ctx,
            egui::RawInput {
                events: std::iter::once(egui::Event::ModifiersChanged(modifiers))
                    .chain(vec![egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers,
                    }])
                    .collect(),
                ..Default::default()
            },
            |ui| {
                let ctx = &ui.ctx().clone();

                actions = ctx.input(|input| pressed_runtime_shortcuts(input).collect());
            },
        );
        actions
    }

    fn command_modifiers() -> egui::Modifiers {
        egui::Modifiers {
            command: true,
            ..Default::default()
        }
    }

    fn command_shift_modifiers() -> egui::Modifiers {
        egui::Modifiers {
            command: true,
            shift: true,
            ..Default::default()
        }
    }

    #[test]
    fn native_paste_events_match_ordered_modifiers_without_duplicate_key_dispatch() {
        for (modifiers, expected) in [
            (command_modifiers(), Some(RuntimeShortcutAction::PlainPaste)),
            (
                command_shift_modifiers(),
                Some(RuntimeShortcutAction::PasteIntoEditor),
            ),
            (egui::Modifiers::NONE, None),
            (
                egui::Modifiers {
                    alt: true,
                    ..command_shift_modifiers()
                },
                None,
            ),
        ] {
            for include_key in [false, true] {
                let ctx = egui::Context::default();
                let mut events = vec![];
                if include_key {
                    events.push(egui::Event::Key {
                        key: egui::Key::V,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers,
                    });
                }
                events.push(egui::Event::Paste("clip".into()));
                events.push(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
                let mut input = egui::RawInput {
                    events: std::iter::once(egui::Event::ModifiersChanged(modifiers))
                        .chain(events)
                        .collect(),
                    ..Default::default()
                };
                let mut native_modifiers = egui::Modifiers::NONE;
                super::super::LocalPasteApp::stage_native_paste_shortcuts(
                    &mut input,
                    &mut native_modifiers,
                );
                let _ = run_ui(&ctx, input, |ui| {
                    let ctx = &ui.ctx().clone();

                    let actual =
                        ctx.input(|input| pressed_runtime_shortcuts(input).collect::<Vec<_>>());
                    assert_eq!(actual, expected.into_iter().collect::<Vec<_>>());
                });
            }
        }
    }

    #[test]
    fn runtime_shortcuts_preserve_native_order_and_repeated_actions() {
        let ctx = egui::Context::default();
        let mut actions = Vec::new();
        let modifiers = command_modifiers();
        let events = [egui::Key::S, egui::Key::N, egui::Key::S]
            .into_iter()
            .map(|key| egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            })
            .collect();
        let _ = run_ui(
            &ctx,
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ui| {
                let ctx = &ui.ctx().clone();

                actions = ctx.input(|input| pressed_runtime_shortcuts(input).collect());
            },
        );
        assert_eq!(
            actions,
            vec![
                RuntimeShortcutAction::Save,
                RuntimeShortcutAction::NewPaste,
                RuntimeShortcutAction::Save
            ]
        );
    }

    #[test]
    fn runtime_shortcut_registry_has_unique_actions_and_labels() {
        assert_eq!(
            runtime_shortcut_label(RuntimeShortcutAction::NewPaste),
            if cfg!(target_os = "macos") {
                "Cmd+N"
            } else {
                "Ctrl+N"
            }
        );
        assert_eq!(
            runtime_shortcut_label(RuntimeShortcutAction::ToggleShortcutHelp),
            "F1"
        );
        let mut actions = HashSet::new();
        let mut labels = HashSet::new();

        for shortcut in RUNTIME_SHORTCUTS {
            let label = runtime_shortcut_label(shortcut.action);
            assert!(!label.contains("Ctrl/Cmd"));
            assert!(
                actions.insert(shortcut.action),
                "duplicate runtime shortcut action: {:?}",
                shortcut.action
            );
            assert!(
                labels.insert(shortcut.help.keys),
                "duplicate runtime shortcut label: {}",
                shortcut.help.keys
            );
        }
    }

    #[test]
    fn delete_selected_adds_cmd_backspace_only_on_macos() {
        let delete = RUNTIME_SHORTCUTS
            .iter()
            .find(|shortcut| shortcut.action == RuntimeShortcutAction::DeleteSelected)
            .copied()
            .unwrap();
        let key = |key, modifiers| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        };
        let cmd = egui::Modifiers {
            command: true,
            mac_cmd: true,
            ..Default::default()
        };
        let ctrl = egui::Modifiers {
            command: true,
            ctrl: true,
            ..Default::default()
        };
        assert!(delete.matches_event(&key(egui::Key::Backspace, cmd), true));
        assert!(delete.matches_event(&key(egui::Key::Delete, cmd), true));
        assert!(delete.matches_event(&key(egui::Key::Delete, ctrl), false));
        assert!(!delete.matches_event(&key(egui::Key::Backspace, ctrl), false));
        for extra in [
            egui::Modifiers { shift: true, ..cmd },
            egui::Modifiers { alt: true, ..cmd },
            egui::Modifiers {
                ctrl: true,
                ..Default::default()
            },
        ] {
            assert!(!delete.matches_event(&key(egui::Key::Backspace, extra), true));
        }
        assert_eq!(
            platform_shortcut_keys(delete.help.keys, true).as_deref(),
            Some("Cmd+Backspace/Delete")
        );
        assert_eq!(
            platform_shortcut_keys(delete.help.keys, false).as_deref(),
            Some("Ctrl+Delete")
        );
    }

    #[test]
    fn runtime_shortcut_registry_matches_registered_chords() {
        for (key, modifiers, expected) in [
            (
                egui::Key::N,
                command_modifiers(),
                RuntimeShortcutAction::NewPaste,
            ),
            (
                egui::Key::S,
                command_modifiers(),
                RuntimeShortcutAction::Save,
            ),
            (
                egui::Key::Delete,
                command_modifiers(),
                RuntimeShortcutAction::DeleteSelected,
            ),
            (
                egui::Key::F,
                command_modifiers(),
                RuntimeShortcutAction::FocusSearch,
            ),
            (
                egui::Key::K,
                command_modifiers(),
                RuntimeShortcutAction::ToggleCommandPalette,
            ),
            (
                egui::Key::P,
                command_shift_modifiers(),
                RuntimeShortcutAction::ToggleCommandPaletteLegacy,
            ),
            (
                egui::Key::I,
                command_modifiers(),
                RuntimeShortcutAction::ToggleProperties,
            ),
            (
                egui::Key::V,
                command_modifiers(),
                RuntimeShortcutAction::PlainPaste,
            ),
            (
                egui::Key::V,
                command_shift_modifiers(),
                RuntimeShortcutAction::PasteIntoEditor,
            ),
        ] {
            assert_eq!(pressed_actions(key, modifiers), vec![expected]);
        }

        assert_eq!(
            pressed_actions(
                egui::Key::F1,
                egui::Modifiers {
                    shift: true,
                    ..Default::default()
                }
            ),
            vec![RuntimeShortcutAction::ToggleShortcutHelp],
            "F1 help toggle should preserve the prior key-only behavior"
        );
    }
}
