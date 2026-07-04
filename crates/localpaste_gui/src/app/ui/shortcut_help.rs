//! Keyboard shortcut help surface.

use super::super::*;
use eframe::egui;

#[derive(Debug, Clone, Copy)]
struct ShortcutEntry {
    keys: &'static str,
    description: &'static str,
}

#[derive(Debug, Clone, Copy)]
struct ShortcutSection {
    title: &'static str,
    entries: &'static [ShortcutEntry],
}

const CORE_ACTION_SHORTCUTS: &[ShortcutEntry] = &[
    ShortcutEntry {
        keys: "Ctrl/Cmd+N",
        description: "Create new paste",
    },
    ShortcutEntry {
        keys: "Ctrl/Cmd+S",
        description: "Save content and metadata",
    },
    ShortcutEntry {
        keys: "Ctrl/Cmd+Delete",
        description: "Delete selected paste (when text inputs are unfocused)",
    },
    ShortcutEntry {
        keys: "Ctrl/Cmd+F",
        description: "Focus sidebar search",
    },
    ShortcutEntry {
        keys: "Ctrl/Cmd+Shift+P",
        description: "Toggle command palette",
    },
    ShortcutEntry {
        keys: "Ctrl/Cmd+K",
        description: "Toggle command palette (legacy)",
    },
    ShortcutEntry {
        keys: "Ctrl/Cmd+I",
        description: "Toggle properties drawer",
    },
    ShortcutEntry {
        keys: "F1",
        description: "Toggle this help",
    },
];

const NAVIGATION_SHORTCUTS: &[ShortcutEntry] = &[
    ShortcutEntry {
        keys: "Arrow Up/Down",
        description: "Navigate paste list, palette, and history rows",
    },
    ShortcutEntry {
        keys: "Enter",
        description: "Open selected command palette result",
    },
    ShortcutEntry {
        keys: "Esc",
        description: "Close command palette/window",
    },
    ShortcutEntry {
        keys: "Home/End (Win/Linux) or Cmd+Left/Right (macOS)",
        description: "Move caret to line start/end",
    },
    ShortcutEntry {
        keys: "Ctrl+Home/End (Win/Linux) or Cmd+Up/Down/Home/End (macOS)",
        description: "Move caret to document start/end",
    },
    ShortcutEntry {
        keys: "Page Up/Down",
        description: "Move caret by visible editor page",
    },
];

const EDITING_SHORTCUTS: &[ShortcutEntry] = &[
    ShortcutEntry {
        keys: "Ctrl/Cmd+A",
        description: "Select all editor text",
    },
    ShortcutEntry {
        keys: "Ctrl/Cmd+C",
        description: "Copy selected text",
    },
    ShortcutEntry {
        keys: "Ctrl/Cmd+X",
        description: "Cut selected text",
    },
    ShortcutEntry {
        keys: "Ctrl/Cmd+V",
        description: "Paste in editor; otherwise create new paste",
    },
    ShortcutEntry {
        keys: "Ctrl/Cmd+Shift+V",
        description: "Force paste as new paste",
    },
    ShortcutEntry {
        keys: "Ctrl/Cmd+Z",
        description: "Undo editor edit",
    },
    ShortcutEntry {
        keys: "Ctrl/Cmd+Y or Ctrl/Cmd+Shift+Z",
        description: "Redo editor edit",
    },
    ShortcutEntry {
        keys: "Ctrl+Left/Right (Win/Linux) or Option+Left/Right (macOS)",
        description: "Move caret by word",
    },
    ShortcutEntry {
        keys: "Ctrl+Backspace/Delete (Win/Linux) or Option+Backspace/Delete (macOS)",
        description: "Delete one word backward/forward",
    },
    ShortcutEntry {
        keys: "Cmd+Backspace / Ctrl+K (macOS)",
        description: "Delete to line start / end",
    },
];

const FIND_SHORTCUTS: &[ShortcutEntry] = &[
    ShortcutEntry {
        keys: "Enter",
        description: "Find next match while Find field is focused",
    },
    ShortcutEntry {
        keys: "Shift+Enter",
        description: "Find previous match while Find field is focused",
    },
    ShortcutEntry {
        keys: "Esc",
        description: "Close Find field",
    },
];

const SHORTCUT_SECTIONS: &[ShortcutSection] = &[
    ShortcutSection {
        title: "Core actions",
        entries: CORE_ACTION_SHORTCUTS,
    },
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
                    render_shortcut_sections(ui, SHORTCUT_SECTIONS);
                });
        });
        if close_on_escape {
            open = false;
        }
        self.shortcut_help_open = open;
    }
}

fn render_shortcut_sections(ui: &mut egui::Ui, sections: &[ShortcutSection]) {
    for (index, section) in sections.iter().enumerate() {
        if index > 0 {
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(6.0);
        }
        ui.label(
            egui::RichText::new(section.title)
                .small()
                .color(COLOR_TEXT_MUTED),
        );
        for entry in section.entries {
            shortcut_row(ui, entry);
        }
    }
}

fn shortcut_row(ui: &mut egui::Ui, entry: &ShortcutEntry) {
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

    #[test]
    fn shortcut_help_entries_exclude_command_palette_queries() {
        for section in SHORTCUT_SECTIONS {
            for entry in section.entries {
                let key_label = entry.keys.to_ascii_lowercase();
                assert!(
                    !key_label.contains("query") && !key_label.contains("palette query"),
                    "keyboard shortcut help must not list non-shortcut command query '{}'",
                    entry.keys
                );
            }
        }
    }

    #[test]
    fn shortcut_help_entries_cover_core_runtime_shortcuts() {
        let labels = SHORTCUT_SECTIONS
            .iter()
            .flat_map(|section| section.entries.iter().map(|entry| entry.keys))
            .collect::<Vec<_>>();
        for expected in [
            "Ctrl/Cmd+N",
            "Ctrl/Cmd+S",
            "Ctrl/Cmd+Delete",
            "Ctrl/Cmd+F",
            "Ctrl/Cmd+Shift+P",
            "Ctrl/Cmd+K",
            "Ctrl/Cmd+I",
            "Ctrl/Cmd+V",
            "Ctrl/Cmd+Shift+V",
            "F1",
        ] {
            assert!(
                labels.contains(&expected),
                "missing shortcut help row for {expected}"
            );
        }
    }
}
