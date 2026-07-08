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

    fn pressed(self, input: &egui::InputState) -> bool {
        match self.chord {
            ShortcutChord::PlainCommand(key) => {
                is_plain_command_shortcut(input.modifiers) && input.key_pressed(key)
            }
            ShortcutChord::CommandShift(key) => {
                is_command_shift_shortcut(input.modifiers) && input.key_pressed(key)
            }
            ShortcutChord::AnyModifier(key) => input.key_pressed(key),
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
            description: "Toggle command palette (legacy)",
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
    RUNTIME_SHORTCUTS
        .iter()
        .filter(|shortcut| shortcut.pressed(input))
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
