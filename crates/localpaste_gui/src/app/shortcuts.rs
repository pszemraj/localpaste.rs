//! Runtime shortcut registry shared by input dispatch and discoverability UI.

use super::interaction_helpers::{is_command_shift_shortcut, is_plain_command_shortcut};
use eframe::egui;

/// Runtime shortcut actions handled by the app frame loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum RuntimeShortcutAction {
    NewPaste,
    Save,
    DeleteSelected,
    FocusSearch,
    ToggleCommandPalette,
    ToggleCommandPaletteLegacy,
    TogglePastePicker,
    ToggleProperties,
    PlainPaste,
    PasteAsNew,
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

    fn matches_event(self, event: &egui::Event) -> bool {
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
        chord: ShortcutChord::PlainCommand(egui::Key::Delete),
        help: ShortcutHelpEntry {
            keys: "Ctrl/Cmd+Delete",
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
        chord: ShortcutChord::CommandShift(egui::Key::P),
        help: ShortcutHelpEntry {
            keys: "Ctrl/Cmd+Shift+P",
            description: "Toggle command palette",
        },
    },
    RuntimeShortcut {
        action: RuntimeShortcutAction::ToggleCommandPaletteLegacy,
        chord: ShortcutChord::PlainCommand(egui::Key::K),
        help: ShortcutHelpEntry {
            keys: "Ctrl/Cmd+K",
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
            description: "Paste in editor; otherwise create new paste",
        },
    },
    RuntimeShortcut {
        action: RuntimeShortcutAction::PasteAsNew,
        chord: ShortcutChord::CommandShift(egui::Key::V),
        help: ShortcutHelpEntry {
            keys: "Ctrl/Cmd+Shift+V",
            description: "Force paste as new paste",
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
    input.events.iter().filter_map(runtime_shortcut_action)
}

/// Matches one native event without changing the order or multiplicity of chords.
///
/// # Returns
/// The registered action for a pressed shortcut event, or `None` for other input.
pub(crate) fn runtime_shortcut_action(event: &egui::Event) -> Option<RuntimeShortcutAction> {
    RUNTIME_SHORTCUTS
        .iter()
        .find(|shortcut| shortcut.matches_event(event))
        .map(|shortcut| shortcut.action)
}

/// Looks up the display label for a registered runtime shortcut action.
///
/// # Returns
/// Human-readable shortcut key label for the action.
///
/// # Panics
/// Panics if a [`RuntimeShortcutAction`] variant is not present in [`RUNTIME_SHORTCUTS`].
pub(crate) fn runtime_shortcut_label(action: RuntimeShortcutAction) -> &'static str {
    RUNTIME_SHORTCUTS
        .iter()
        .find(|shortcut| shortcut.action == action)
        .map(|shortcut| shortcut.help.keys)
        .expect("runtime shortcut action must be registered")
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
    use super::*;
    use std::collections::HashSet;

    fn pressed_actions(key: egui::Key, modifiers: egui::Modifiers) -> Vec<RuntimeShortcutAction> {
        let ctx = egui::Context::default();
        let mut actions = Vec::new();
        let _ = ctx.run(
            egui::RawInput {
                modifiers,
                events: vec![egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers,
                }],
                ..Default::default()
            },
            |ctx| {
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
        let _ = ctx.run(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ctx| {
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
        let mut actions = HashSet::new();
        let mut labels = HashSet::new();

        for shortcut in RUNTIME_SHORTCUTS {
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
                egui::Key::P,
                command_shift_modifiers(),
                RuntimeShortcutAction::ToggleCommandPalette,
            ),
            (
                egui::Key::K,
                command_modifiers(),
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
                RuntimeShortcutAction::PasteAsNew,
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
