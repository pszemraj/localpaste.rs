//! Keyboard shortcut help surface.

use super::super::shortcuts::{ShortcutHelpEntry, RUNTIME_SHORTCUTS, STATIC_SHORTCUT_SECTIONS};
use super::super::*;
use eframe::egui;

impl LocalPasteApp {
    /// Open shortcut help as the sole keyboard-owning discovery surface.
    pub(in crate::app) fn open_shortcut_help(&mut self) {
        self.command_palette_open = false;
        self.close_paste_picker();
        self.shortcut_help_open = true;
    }

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
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.shortcut_help_query)
                            .hint_text("Search actions or keys...")
                            .return_key(None),
                    );
                    response.request_focus();
                    egui::ScrollArea::vertical()
                        .max_height(480.0)
                        .show(ui, |ui| {
                            render_shortcut_sections(ui, &self.shortcut_help_query);
                        });
                });
        });
        if close_on_escape {
            open = false;
        }
        self.shortcut_help_open = open;
    }
}

fn render_shortcut_sections(ui: &mut egui::Ui, query: &str) {
    section_title(ui, "App actions");
    for shortcut in RUNTIME_SHORTCUTS {
        if shortcut_matches(shortcut.help(), query) {
            shortcut_row(ui, shortcut.help());
        }
    }
    for section in STATIC_SHORTCUT_SECTIONS {
        if !section
            .entries
            .iter()
            .any(|entry| shortcut_matches(*entry, query))
        {
            continue;
        }
        ui.add_space(6.0);
        ui.separator();
        ui.add_space(6.0);
        section_title(ui, section.title);
        for entry in section.entries {
            if shortcut_matches(*entry, query) {
                shortcut_row(ui, *entry);
            }
        }
    }
}

fn shortcut_matches(entry: ShortcutHelpEntry, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    if entry.description.to_lowercase().contains(&query) {
        return true;
    }
    let compact = |value: &str| {
        value
            .to_lowercase()
            .chars()
            .filter(|ch| !ch.is_whitespace() && *ch != '+')
            .collect::<String>()
    };
    let keys = compact(entry.keys);
    let query = compact(&query);
    keys.contains(&query)
        || keys.replace("ctrl/cmd", "ctrl").contains(&query)
        || keys.replace("ctrl/cmd", "cmd").contains(&query)
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
    fn help_search_matches_actions_and_platform_key_spelling() {
        let entry = ShortcutHelpEntry {
            keys: "Ctrl/Cmd+Shift+K",
            description: "Open paste picker",
        };
        for query in ["picker", "Cmd+Shift+K", "Ctrl+Shift+K", "cmd shift k"] {
            assert!(shortcut_matches(entry, query), "{query}");
        }
        assert!(!shortcut_matches(entry, "export"));
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
