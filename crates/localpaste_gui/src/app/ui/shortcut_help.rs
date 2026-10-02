//! Keyboard shortcut help surface.

use super::super::shortcuts::{ShortcutHelpEntry, RUNTIME_SHORTCUTS, STATIC_SHORTCUT_SECTIONS};
use super::super::*;
use eframe::egui;

impl LocalPasteApp {
    /// Open shortcut help as the sole keyboard-owning discovery surface.
    pub(in crate::app) fn open_shortcut_help(&mut self, ctx: &egui::Context) {
        if self.shortcut_help_open {
            self.shortcut_help_focus_requested = true;
            return;
        }
        self.command_palette_open = false;
        self.close_paste_picker();
        self.shortcut_help_return_focus = ctx.memory(|memory| memory.focused());
        self.shortcut_help_focus_requested = true;
        self.shortcut_help_open = true;
    }

    /// Dismiss help and return keyboard ownership to the input that opened it.
    pub(in crate::app) fn close_shortcut_help(&mut self, ctx: &egui::Context) {
        self.shortcut_help_open = false;
        self.shortcut_help_focus_requested = false;
        if let Some(id) = self.shortcut_help_return_focus.take() {
            if id == egui::Id::new(VIRTUAL_EDITOR_ID) {
                self.focus_editor_next = true;
            }
            ctx.memory_mut(|memory| memory.request_focus(id));
        }
    }

    /// Renders the keyboard shortcut help window.
    pub(crate) fn render_shortcut_help(&mut self, ctx: &egui::Context) {
        if !self.shortcut_help_open {
            return;
        }
        let mut open = self.shortcut_help_open;
        let mut close_requested = false;
        let close_on_escape =
            ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        let results_height = (ctx.content_rect().height() - 260.0).clamp(160.0, 360.0);

        with_muted_modal_chrome(ctx, || {
            egui::Window::new("Keyboard Shortcuts")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .default_width(600.0)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.set_width(600.0);
                    let previous_query = self.shortcut_help_query.clone();
                    ui.horizontal(|ui| {
                        let response = ui.add_sized(
                            [ui.available_width() - 84.0, ui.spacing().interact_size.y],
                            egui::TextEdit::singleline(&mut self.shortcut_help_query)
                                .id(egui::Id::new("shortcut_help_query"))
                                .hint_text("Search actions or keys, e.g. undo")
                                .return_key(None),
                        );
                        if self.shortcut_help_focus_requested {
                            response.request_focus();
                            self.shortcut_help_focus_requested = false;
                        }
                        if ui
                            .add_enabled(
                                !self.shortcut_help_query.is_empty(),
                                egui::Button::new("Clear"),
                            )
                            .clicked()
                        {
                            self.shortcut_help_query.clear();
                            response.request_focus();
                        }
                    });
                    ui.label(
                        egui::RichText::new(if cfg!(target_os = "macos") {
                            "Mac shortcuts. Search by action (undo) or keys (Cmd+Shift+K)."
                        } else {
                            "Search by action (undo) or keys (Ctrl+Shift+K)."
                        })
                        .small()
                        .color(COLOR_TEXT_SECONDARY),
                    );
                    let mut scroll = egui::ScrollArea::vertical()
                        .id_salt("shortcut_help_results")
                        .auto_shrink([false, false])
                        .max_height(results_height);
                    if self.shortcut_help_query != previous_query {
                        scroll = scroll.vertical_scroll_offset(0.0);
                    }
                    scroll.show(ui, |ui| {
                        render_shortcut_sections(ui, &self.shortcut_help_query);
                    });
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("Esc or F1 to close")
                                .small()
                                .color(COLOR_TEXT_SECONDARY),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            close_requested = ui.button("Close").clicked();
                        });
                    });
                });
        });
        if close_on_escape || close_requested || !open {
            self.close_shortcut_help(ctx);
        }
    }
}

fn render_shortcut_sections(ui: &mut egui::Ui, query: &str) {
    let app_entries = RUNTIME_SHORTCUTS
        .iter()
        .map(|shortcut| shortcut.help())
        .collect::<Vec<_>>();
    let sections = std::iter::once(("App actions", app_entries.as_slice())).chain(
        STATIC_SHORTCUT_SECTIONS
            .iter()
            .map(|section| (section.title, section.entries)),
    );
    let mut shown_section = false;
    for (title, entries) in sections {
        let matches = entries
            .iter()
            .filter_map(|entry| {
                let keys = platform_shortcut_keys(entry.keys, cfg!(target_os = "macos"))?;
                shortcut_matches(*entry, query).then_some((keys, entry.description))
            })
            .collect::<Vec<_>>();
        if matches.is_empty() {
            continue;
        }
        if shown_section {
            ui.add_space(6.0);
            ui.separator();
        }
        shown_section = true;
        section_title(ui, title);
        let description_width = (ui.available_width() - 196.0).max(100.0);
        egui::Grid::new(("shortcut_help_section", title))
            .num_columns(2)
            .min_row_height(0.0)
            .spacing(egui::vec2(16.0, 8.0))
            .show(ui, |ui| {
                for (keys, description) in matches {
                    shortcut_cell(
                        ui,
                        180.0,
                        egui::RichText::new(keys)
                            .monospace()
                            .color(COLOR_ACCENT_TEXT),
                    );
                    shortcut_cell(
                        ui,
                        description_width,
                        egui::RichText::new(description).color(COLOR_TEXT_PRIMARY),
                    );
                    ui.end_row();
                }
            });
    }
    if !shown_section {
        ui.label("No matching shortcuts.");
        ui.label(
            egui::RichText::new(
                "Try an action such as undo, or clear the search to see all shortcuts.",
            )
            .color(COLOR_TEXT_SECONDARY),
        );
    }
}

/// Keep each wrapped table cell left aligned within its fixed column width.
fn shortcut_cell(ui: &mut egui::Ui, width: f32, text: egui::RichText) {
    ui.allocate_ui_with_layout(
        egui::vec2(width, 0.0),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.set_width(width);
            ui.add(egui::Label::new(text).wrap());
        },
    );
}

/// Select the native key spelling from the registry's shared platform labels.
fn platform_shortcut_keys(keys: &str, macos: bool) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_displays_native_keys_and_hides_unavailable_platform_chords() {
        assert_eq!(
            platform_shortcut_keys("Ctrl/Cmd+Shift+K", true).as_deref(),
            Some("Cmd+Shift+K")
        );
        assert_eq!(
            platform_shortcut_keys("Ctrl/Cmd+Shift+K", false).as_deref(),
            Some("Ctrl+Shift+K")
        );
        let navigation = "Home/End (Win/Linux) or Cmd+Left/Right (macOS)";
        assert_eq!(
            platform_shortcut_keys(navigation, true).as_deref(),
            Some("Cmd+Left/Right")
        );
        assert_eq!(
            platform_shortcut_keys(navigation, false).as_deref(),
            Some("Home/End")
        );
        assert_eq!(
            platform_shortcut_keys("Cmd+Backspace / Ctrl+K (macOS)", true).as_deref(),
            Some("Cmd+Backspace / Ctrl+K")
        );
        assert!(platform_shortcut_keys("Cmd+Backspace / Ctrl+K (macOS)", false).is_none());
    }

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
