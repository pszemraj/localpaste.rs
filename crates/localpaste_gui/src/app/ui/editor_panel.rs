//! Central editor panel rendering for the virtual editor.

use super::super::shortcuts::runtime_shortcut_label;
use super::super::*;
use super::editor_panel_virtual::VirtualEditorRenderOptions;
use eframe::egui;

impl LocalPasteApp {
    /// Renders the primary editor panel, including metadata toolbar and mode UI.
    ///
    /// # Panics
    /// Panics if a virtual-editor highlight row fails internal consistency checks.
    pub(crate) fn render_editor_panel(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            let selected_meta = self.selected_paste.as_ref().map(|paste| paste.id.clone());

            if let Some(id) = selected_meta {
                let editor_id = egui::Id::new(VIRTUAL_EDITOR_ID);
                let editor_had_virtual_focus =
                    ctx.memory(|m| m.has_focus(editor_id)) || self.virtual_editor_state.has_focus;
                let language = self.edit_language.clone();
                let is_large = self.active_text_len_bytes() >= HIGHLIGHT_PLAIN_THRESHOLD;
                let mutation_block_reason = self.mutation_shortcut_block_reason();
                let toolbar = self.render_editor_toolbar(ui, editor_had_virtual_focus);
                if let Some(tag) = toolbar.tag_search {
                    self.set_search_query(tag);
                }
                if toolbar.apply_metadata {
                    self.save_metadata_now();
                }
                if toolbar.copy {
                    self.queue_clipboard_text(self.active_snapshot());
                    self.set_status("Copied paste content.");
                }
                if toolbar.copy_link {
                    self.queue_clipboard_text(super::super::util::api_paste_link_for_copy(self.server_addr, &id));
                    self.set_status("Copied API paste link.");
                }
                if toolbar.duplicate {
                    self.create_new_paste_with_content(self.active_snapshot());
                    self.set_status("Duplicated paste into a new draft.");
                }
                if toolbar.export {
                    self.export_selected_paste();
                }
                if toolbar.find {
                    self.open_editor_find();
                }
                if toolbar.properties {
                    self.properties_drawer_open = true;
                }
                if toolbar.delete {
                    self.delete_selected();
                }
                if toolbar.diff {
                    self.open_diff_modal();
                }
                if toolbar.history {
                    self.open_history_modal();
                }
                let preserve_virtual_editor_focus = toolbar.preserve_editor_focus;

                self.render_editor_find_bar(ui);
                if self.editor_find.open {
                    ui.add_space(4.0);
                }

                ui.label(
                    RichText::new(id.clone())
                        .small()
                        .monospace()
                        .color(COLOR_TEXT_MUTED),
                );
                if let Some(reason) = mutation_block_reason {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(reason).small().color(COLOR_TEXT_MUTED),
                    );
                }
                ui.add_space(6.0);
                let editor_height = ui.available_height();
                let editor_style = TextStyle::Name(EDITOR_TEXT_STYLE.into());
                let editor_font = ui
                    .style()
                    .text_styles
                    .get(&editor_style)
                    .cloned()
                    .unwrap_or_else(|| TextStyle::Monospace.resolve(ui.style()));
                let language_hint = syntect_language_hint(language.as_deref().unwrap_or("text"));
                let theme = (!is_large).then(|| CodeTheme::from_memory(ui.ctx(), ui.style()));
                let theme_key = theme
                    .as_ref()
                    .map(syntect_theme_key)
                    .unwrap_or("base16-mocha.dark");
                let revision = self.active_revision();
                let text_len = self.active_text_len_bytes();
                if is_large
                    && (self.highlight_pending.is_some()
                        || self.highlight_render.is_some()
                        || self.highlight_staged.is_some())
                {
                    // Crossing into plain-threshold mode should drop any stale
                    // staged/current highlight state so large buffers stay plain.
                    self.clear_highlight_state();
                }
                let async_mode = !is_large;
                let debounce_window = self.highlight_debounce_window(text_len, async_mode);
                let debounce_active = self
                    .last_edit_at
                    .map(|last| last.elapsed() < debounce_window)
                    .unwrap_or(false);
                let should_request = async_mode
                    && self.should_request_highlight(
                        &language_hint,
                        theme_key,
                        debounce_active,
                        id.as_str(),
                    );
                self.track_highlight_debounce_wake(
                    async_mode && debounce_active && !should_request,
                    debounce_window,
                );
                if should_request {
                    let request_text =
                        HighlightRequestText::Rope(self.virtual_editor_buffer.rope().clone());
                    self.dispatch_highlight_request(
                        revision,
                        request_text,
                        &language_hint,
                        theme_key,
                        id.as_str(),
                    );
                }
                let has_render = self
                    .highlight_render
                    .as_ref()
                    .filter(|render| {
                        render.matches_exact(
                            revision,
                            text_len,
                            &language_hint,
                            theme_key,
                            id.as_str(),
                        )
                    })
                    .is_some();
                let has_context_render = self
                    .highlight_render
                    .as_ref()
                    .filter(|render| render.matches_context(id.as_str(), &language_hint, theme_key))
                    .is_some();
                let has_staged_render = self
                    .highlight_staged
                    .as_ref()
                    .filter(|render| {
                        render.matches_exact(
                            revision,
                            text_len,
                            &language_hint,
                            theme_key,
                            id.as_str(),
                        )
                    })
                    .is_some();
                let has_staged_context = self
                    .highlight_staged
                    .as_ref()
                    .filter(|render| render.matches_context(id.as_str(), &language_hint, theme_key))
                    .is_some();
                // Once large, force plain rendering and do not allow
                // context-only highlight fallback.
                let use_plain = is_large || !(has_context_render || has_staged_context);
                if self.highlight_trace_enabled {
                    self.trace_highlight(
                        "frame",
                        format!(
                            "revision={} text_len={} async={} has_render={} has_render_ctx={} has_staged={} has_staged_ctx={} use_plain={}",
                            revision,
                            text_len,
                            async_mode,
                            has_render,
                            has_context_render,
                            has_staged_render,
                            has_staged_context,
                            use_plain
                        )
                        .as_str(),
                    );
                }
                let highlight_render = self.highlight_render.take();
                let highlight_render_match = highlight_render
                    .as_ref()
                    .filter(|render| {
                        render.matches_exact(
                            revision,
                            text_len,
                            &language_hint,
                            theme_key,
                            id.as_str(),
                        )
                    })
                    .or_else(|| {
                        if async_mode {
                            highlight_render.as_ref().filter(|render| {
                                render.matches_context(id.as_str(), &language_hint, theme_key)
                            })
                        } else {
                            None
                        }
                    });
                let row_height = ui.fonts_mut(|fonts| fonts.row_height(&editor_font));
                self.render_virtual_editor_panel(
                    ui,
                    row_height,
                    editor_height,
                    &editor_font,
                    VirtualEditorRenderOptions {
                        highlight_render_match,
                        use_plain,
                        preserve_focus_from_editor_chrome: preserve_virtual_editor_focus,
                    },
                );
                self.highlight_render = highlight_render;
                self.render_version_dialogs(ctx);
            } else if self.selected_id.is_some() {
                ui.label(RichText::new("Loading paste...").color(COLOR_TEXT_MUTED));
            } else {
                render_empty_editor_state(ui);
            }
        });
    }
}

/// Shortcut hints under the empty editor message, in display order.
const EMPTY_STATE_HINTS: [(&str, RuntimeShortcutAction); 5] = [
    ("New paste", RuntimeShortcutAction::NewPaste),
    ("Paste as new paste", RuntimeShortcutAction::PlainPaste),
    ("Open a paste", RuntimeShortcutAction::TogglePastePicker),
    (
        "Command palette",
        RuntimeShortcutAction::ToggleCommandPalette,
    ),
    (
        "Keyboard shortcuts",
        RuntimeShortcutAction::ToggleShortcutHelp,
    ),
];
/// Width of the hint block; descriptions sit left and keys right within it.
const EMPTY_STATE_WIDTH: f32 = 280.0;

/// Renders the centered placeholder shown when no paste is selected.
///
/// Labels are real widgets so the message stays in the accessibility tree.
/// Key labels come from the runtime shortcut registry, so they read `Cmd` on
/// macOS and `Ctrl` elsewhere.
fn render_empty_editor_state(ui: &mut egui::Ui) {
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, egui::Sense::hover());
    let line_height = ui.spacing().interact_size.y;
    let title_gap = line_height * 0.75;
    let width = EMPTY_STATE_WIDTH.min(area.width());
    let block_height = line_height * (EMPTY_STATE_HINTS.len() + 1) as f32 + title_gap;
    let mut line = egui::Rect::from_center_size(
        egui::pos2(
            area.center().x,
            area.center().y - (block_height - line_height) / 2.0,
        ),
        egui::vec2(width, line_height),
    );
    let mut child = |rect: egui::Rect, layout: egui::Layout| {
        ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(layout))
    };
    child(line, egui::Layout::top_down(egui::Align::Center))
        .label(RichText::new("Select a paste from the sidebar.").color(COLOR_TEXT_SECONDARY));
    line = line.translate(egui::vec2(0.0, line_height + title_gap));
    for (description, action) in EMPTY_STATE_HINTS {
        child(line, egui::Layout::left_to_right(egui::Align::Center))
            .label(RichText::new(description).color(COLOR_TEXT_MUTED));
        child(line, egui::Layout::right_to_left(egui::Align::Center))
            .label(RichText::new(runtime_shortcut_label(action)).color(COLOR_TEXT_MUTED));
        line = line.translate(egui::vec2(0.0, line_height));
    }
}
