//! Keyboard shortcut help surface.

use super::super::shortcuts::{ShortcutHelpEntry, RUNTIME_SHORTCUTS, STATIC_SHORTCUT_SECTIONS};
use super::super::*;
use eframe::egui;

impl LocalPasteApp {
    /// Render ordered native event slices on either side of discovery focus changes.
    ///
    /// Earlier input must finish in its current widget before an opening or
    /// dismissal chord transfers ownership. Deferred events precede newly arrived
    /// events, and each rendered slice consumes at least one queued event.
    ///
    /// # Arguments
    /// - `ctx`: Context used to request the next ordered frame.
    /// - `input`: Native batch edited before egui derives focus or pointer state.
    pub(in crate::app) fn stage_discovery_input(
        &mut self,
        ctx: &egui::Context,
        input: &mut egui::RawInput,
    ) {
        let mut events = std::mem::take(&mut self.deferred_discovery_events);
        events.append(&mut input.events);
        let first_boundary = events.iter().position(|event| {
            discovery_toggle(event) || (self.discovery_open() && discovery_escape(event))
        });
        // Opening needs a rendered sizing pass before its query accepts input.
        // Dismissal can immediately deliver its suffix to the existing opener.
        let split_at = first_boundary.and_then(|index| {
            if index > 0 {
                Some(index)
            } else if events
                .first()
                .is_some_and(|event| self.discovery_boundary_closes(event))
            {
                events
                    .iter()
                    .enumerate()
                    .skip(1)
                    .find_map(|(index, event)| {
                        (discovery_toggle(event) || discovery_escape(event)).then_some(index)
                    })
            } else {
                Some(1)
            }
        });
        if let Some(index) = split_at {
            for event in events.split_off(index) {
                if matches!(event, egui::Event::WindowFocused(_)) {
                    // Native activation is current machine state. Preserve it
                    // alongside RawInput.focused even while edits are queued.
                    events.push(event);
                } else {
                    self.deferred_discovery_events.push(event);
                }
            }
            if !self.deferred_discovery_events.is_empty() {
                ctx.request_repaint();
            }
        }
        input.events = events;
    }

    /// Whether a discovery query owns keyboard input independently of version dialogs.
    fn discovery_open(&self) -> bool {
        self.command_palette_open || self.paste_picker_open || self.shortcut_help_open
    }

    /// Whether this boundary dismisses the currently open discovery workflow.
    fn discovery_boundary_closes(&self, event: &egui::Event) -> bool {
        discovery_escape(event) && self.discovery_open()
            || match super::super::shortcuts::runtime_shortcut_action(event) {
                Some(
                    super::super::shortcuts::RuntimeShortcutAction::ToggleCommandPalette
                    | super::super::shortcuts::RuntimeShortcutAction::ToggleCommandPaletteLegacy,
                ) => self.command_palette_open,
                Some(super::super::shortcuts::RuntimeShortcutAction::TogglePastePicker) => {
                    self.paste_picker_open
                }
                Some(super::super::shortcuts::RuntimeShortcutAction::ToggleShortcutHelp) => {
                    self.shortcut_help_open
                }
                _ => false,
            }
    }

    /// Transfer focus before background inputs can consume this slice's query text.
    pub(in crate::app) fn focus_discovery_input(&self, ctx: &egui::Context) {
        let input_id = if self.command_palette_open {
            COMMAND_PALETTE_INPUT_ID
        } else if self.paste_picker_open {
            PASTE_PICKER_INPUT_ID
        } else if self.shortcut_help_open {
            "shortcut_help_query"
        } else {
            return;
        };
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(input_id)));
    }

    /// Dismiss discovery at the start of its ordered slice, before the opener renders.
    ///
    /// # Arguments
    /// - `ctx`: Context whose focus and current Escape event are updated.
    /// - `event`: The current ordered native event.
    ///
    /// # Returns
    /// `true` when Escape dismissed an open discovery surface.
    pub(in crate::app) fn dismiss_discovery_on_escape(
        &mut self,
        ctx: &egui::Context,
        event: &egui::Event,
    ) -> bool {
        if !self.discovery_open() || !discovery_escape(event) {
            return false;
        }
        self.command_palette_open = false;
        self.close_paste_picker();
        self.shortcut_help_open = false;
        self.shortcut_help_focus_requested = false;
        ctx.input_mut(|input| {
            input.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
        });
        self.restore_discovery_focus(ctx);
        true
    }

    /// Preserve the original input when opening or switching discovery surfaces.
    pub(in crate::app) fn remember_discovery_focus(&mut self, ctx: &egui::Context) {
        if !self.command_palette_open && !self.paste_picker_open && !self.shortcut_help_open {
            self.discovery_return_focus = ctx.memory(|memory| memory.focused());
        }
    }

    /// Return keyboard ownership after dismissing a discovery surface.
    pub(in crate::app) fn restore_discovery_focus(&mut self, ctx: &egui::Context) {
        if let Some(id) = self.discovery_return_focus.take() {
            if id == egui::Id::new(VIRTUAL_EDITOR_ID) {
                self.focus_editor_next = true;
            }
            ctx.memory_mut(|memory| memory.request_focus(id));
        }
    }

    /// Open shortcut help as the sole keyboard-owning discovery surface.
    pub(in crate::app) fn open_shortcut_help(&mut self, ctx: &egui::Context) {
        if self.shortcut_help_open {
            self.shortcut_help_focus_requested = true;
            return;
        }
        self.remember_discovery_focus(ctx);
        self.command_palette_open = false;
        self.close_paste_picker();
        self.shortcut_help_focus_requested = true;
        self.shortcut_help_open = true;
        self.focus_discovery_input(ctx);
    }

    /// Dismiss help and return keyboard ownership to the input that opened it.
    pub(in crate::app) fn close_shortcut_help(&mut self, ctx: &egui::Context) {
        self.shortcut_help_open = false;
        self.shortcut_help_focus_requested = false;
        self.restore_discovery_focus(ctx);
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
                                .hint_text("Search shortcuts...")
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

/// Whether the event starts, ends, or switches a discovery input workflow.
fn discovery_toggle(event: &egui::Event) -> bool {
    matches!(
        super::super::shortcuts::runtime_shortcut_action(event),
        Some(
            super::super::shortcuts::RuntimeShortcutAction::ToggleCommandPalette
                | super::super::shortcuts::RuntimeShortcutAction::ToggleCommandPaletteLegacy
                | super::super::shortcuts::RuntimeShortcutAction::TogglePastePicker
                | super::super::shortcuts::RuntimeShortcutAction::ToggleShortcutHelp
        )
    )
}

/// Whether an unmodified Escape press dismisses discovery.
fn discovery_escape(event: &egui::Event) -> bool {
    matches!(event, egui::Event::Key {
        key: egui::Key::Escape,
        pressed: true,
        modifiers,
        ..
    } if modifiers.is_none())
}

/// Current native activation, honoring the final focus event in synthetic/native batches.
///
/// # Returns
/// Whether the native window can receive keyboard focus at this point in the frame.
pub(in crate::app) fn native_window_has_focus(ctx: &egui::Context) -> bool {
    ctx.input(|input| {
        input
            .events
            .iter()
            .rev()
            .find_map(|event| match event {
                egui::Event::WindowFocused(focused) => Some(*focused),
                _ => None,
            })
            .unwrap_or(input.focused)
    })
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
