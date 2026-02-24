//! Keyboard shortcut help surface.

use super::super::*;
use eframe::egui;

impl LocalPasteApp {
    /// Renders the keyboard shortcut help window.
    pub(crate) fn render_shortcut_help(&mut self, ctx: &egui::Context) {
        if !self.shortcut_help_open {
            return;
        }
        let mut open = self.shortcut_help_open;

        egui::Window::new("Keyboard Shortcuts")
            .open(&mut open)
            .resizable(false)
            .default_width(420.0)
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new("Core actions")
                        .small()
                        .color(COLOR_TEXT_MUTED),
                );
                shortcut_row(ui, "Ctrl/Cmd+N", "Create new paste");
                shortcut_row(ui, "Ctrl/Cmd+S", "Save content and metadata");
                shortcut_row(
                    ui,
                    "Ctrl/Cmd+Delete",
                    "Delete selected paste (when text inputs are unfocused)",
                );
                shortcut_row(ui, "Ctrl/Cmd+F", "Focus sidebar search");
                shortcut_row(ui, "Ctrl/Cmd+Shift+P", "Toggle command palette");
                shortcut_row(ui, "Ctrl/Cmd+K", "Toggle command palette (legacy)");
                shortcut_row(ui, "Ctrl/Cmd+I", "Toggle properties drawer");
                shortcut_row(ui, "F1", "Toggle this help");

                ui.add_space(6.0);
                ui.separator();
                ui.add_space(6.0);

                ui.label(
                    egui::RichText::new("Editor/palette")
                        .small()
                        .color(COLOR_TEXT_MUTED),
                );
                shortcut_row(ui, "Arrow Up/Down", "Navigate paste list and palette");
                shortcut_row(ui, "Home/End", "Move caret to line start/end");
                shortcut_row(
                    ui,
                    "Ctrl+Home/End (Win/Linux) or Cmd+Up/Down (macOS)",
                    "Move caret to document start/end",
                );
                shortcut_row(ui, "Enter", "Open selected command palette result");
                shortcut_row(ui, "Esc", "Close command palette/window");
                shortcut_row(ui, "Ctrl/Cmd+C", "Copy selected text");
                shortcut_row(
                    ui,
                    "Ctrl/Cmd+V",
                    "Paste in editor; otherwise create new paste",
                );
                shortcut_row(ui, "Ctrl/Cmd+Shift+V", "Force paste as new paste");
            });
        self.shortcut_help_open = open;
    }
}

fn shortcut_row(ui: &mut egui::Ui, keys: &str, description: &str) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(keys).monospace().color(COLOR_ACCENT));
        ui.label(egui::RichText::new(description).color(COLOR_TEXT_PRIMARY));
    });
}
