//! Keyboard shortcut help surface.

use super::super::shortcuts::{ShortcutHelpEntry, RUNTIME_SHORTCUTS};
use super::super::*;
use eframe::egui;

#[derive(Debug, Clone, Copy)]
struct ShortcutSection {
    title: &'static str,
    entries: &'static [ShortcutHelpEntry],
}

const NAVIGATION_SHORTCUTS: &[ShortcutHelpEntry] = &[
    ShortcutHelpEntry {
        keys: "Arrow Up/Down",
        description: "Navigate paste list, palette, and history rows",
    },
    ShortcutHelpEntry {
        keys: "Enter",
        description: "Open selected command palette result",
    },
    ShortcutHelpEntry {
        keys: "Esc",
        description: "Close command palette/window",
    },
    ShortcutHelpEntry {
        keys: "Home/End (Win/Linux) or Cmd+Left/Right (macOS)",
        description: "Move caret to line start/end",
    },
    ShortcutHelpEntry {
        keys: "Ctrl+Home/End (Win/Linux) or Cmd+Up/Down/Home/End (macOS)",
        description: "Move caret to document start/end",
    },
    ShortcutHelpEntry {
        keys: "Page Up/Down",
        description: "Move caret by visible editor page",
    },
];

const EDITING_SHORTCUTS: &[ShortcutHelpEntry] = &[
    ShortcutHelpEntry {
        keys: "Ctrl/Cmd+A",
        description: "Select all editor text",
    },
    ShortcutHelpEntry {
        keys: "Ctrl/Cmd+C",
        description: "Copy selected text",
    },
    ShortcutHelpEntry {
        keys: "Ctrl/Cmd+X",
        description: "Cut selected text",
    },
    ShortcutHelpEntry {
        keys: "Ctrl/Cmd+Z",
        description: "Undo editor edit",
    },
    ShortcutHelpEntry {
        keys: "Ctrl/Cmd+Y or Ctrl/Cmd+Shift+Z",
        description: "Redo editor edit",
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

const FIND_SHORTCUTS: &[ShortcutHelpEntry] = &[
    ShortcutHelpEntry {
        keys: "Enter",
        description: "Find next match while Find field is focused",
    },
    ShortcutHelpEntry {
        keys: "Shift+Enter",
        description: "Find previous match while Find field is focused",
    },
    ShortcutHelpEntry {
        keys: "Esc",
        description: "Close Find field",
    },
];

const STATIC_SHORTCUT_SECTIONS: &[ShortcutSection] = &[
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

impl LocalPasteApp {
    /// Renders the keyboard shortcut help window.
    pub(crate) fn render_shortcut_help(&mut self, ctx: &egui::Context) {
        if !self.shortcut_help_open {
            return;
        }
        let mut open = self.shortcut_help_open;
        let close_on_escape = ctx.input(|input| input.key_pressed(egui::Key::Escape));

        with_muted_modal_chrome(ctx, || {
            egui::Window::new("Keyboard Shortcuts")
                .open(&mut open)
                .resizable(false)
                .default_width(560.0)
                .show(ctx, |ui| {
                    render_shortcut_sections(ui);
                });
        });
        if close_on_escape {
            open = false;
        }
        self.shortcut_help_open = open;
    }
}

fn render_shortcut_sections(ui: &mut egui::Ui) {
    section_title(ui, "App actions");
    for shortcut in RUNTIME_SHORTCUTS {
        shortcut_row(ui, shortcut.help());
    }
    for section in STATIC_SHORTCUT_SECTIONS {
        ui.add_space(6.0);
        ui.separator();
        ui.add_space(6.0);
        section_title(ui, section.title);
        for entry in section.entries {
            shortcut_row(ui, *entry);
        }
    }
}

fn section_title(ui: &mut egui::Ui, title: &'static str) {
    ui.label(egui::RichText::new(title).small().color(COLOR_TEXT_MUTED));
}

fn shortcut_row(ui: &mut egui::Ui, entry: ShortcutHelpEntry) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(entry.keys)
                .monospace()
                .color(COLOR_ACCENT_TEXT),
        );
        ui.label(egui::RichText::new(entry.description).color(COLOR_TEXT_PRIMARY));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_displayed_shortcut_entries() -> Vec<ShortcutHelpEntry> {
        RUNTIME_SHORTCUTS
            .iter()
            .map(|shortcut| shortcut.help())
            .chain(
                STATIC_SHORTCUT_SECTIONS
                    .iter()
                    .flat_map(|section| section.entries.iter().copied()),
            )
            .collect()
    }

    #[test]
    fn shortcut_help_entries_exclude_command_palette_queries() {
        for entry in all_displayed_shortcut_entries() {
            let key_label = entry.keys.to_ascii_lowercase();
            assert!(
                !key_label.contains("query") && !key_label.contains("palette query"),
                "keyboard shortcut help must not list non-shortcut command query '{}'",
                entry.keys
            );
        }
    }

    #[test]
    fn shortcut_help_entries_include_all_registered_runtime_shortcuts() {
        let labels = all_displayed_shortcut_entries()
            .into_iter()
            .map(|entry| entry.keys)
            .collect::<Vec<_>>();
        for shortcut in RUNTIME_SHORTCUTS {
            let expected = shortcut.help().keys;
            assert!(
                labels.contains(&expected),
                "missing shortcut help row for {expected}"
            );
        }
    }
}
