//! Native egui app skeleton for the LocalPaste rewrite.

use crate::backend::{spawn_backend, BackendHandle, CoreCmd, CoreEvent, PasteSummary};
use eframe::egui::{
    self, Color32, FontFamily, FontId, Margin, RichText, Stroke, TextStyle, Visuals,
};
use localpaste_core::models::paste::Paste;
use localpaste_core::{Config, Database};
use tracing::{info, warn};

pub struct LocalPasteApp {
    backend: BackendHandle,
    pastes: Vec<PasteSummary>,
    selected_id: Option<String>,
    selected_paste: Option<Paste>,
    selected_content: String,
    db_path: String,
    status: Option<String>,
    style_applied: bool,
}

const COLOR_BG_PRIMARY: Color32 = Color32::from_rgb(0x0d, 0x11, 0x17);
const COLOR_BG_SECONDARY: Color32 = Color32::from_rgb(0x16, 0x1b, 0x22);
const COLOR_BG_TERTIARY: Color32 = Color32::from_rgb(0x21, 0x26, 0x2d);
const COLOR_TEXT_PRIMARY: Color32 = Color32::from_rgb(0xc9, 0xd1, 0xd9);
const COLOR_TEXT_MUTED: Color32 = Color32::from_rgb(0x6e, 0x76, 0x81);
const COLOR_ACCENT: Color32 = Color32::from_rgb(0xE5, 0x70, 0x00);
const COLOR_ACCENT_HOVER: Color32 = Color32::from_rgb(0xCE, 0x42, 0x2B);
const COLOR_BORDER: Color32 = Color32::from_rgb(0x30, 0x36, 0x3d);

impl LocalPasteApp {
    pub fn new() -> Result<Self, localpaste_core::AppError> {
        let config = Config::from_env();
        let db_path = config.db_path.clone();
        let db = Database::new(&config.db_path)?;
        info!("native GUI opened database at {}", config.db_path);

        let backend = spawn_backend(db);
        let _ = backend.cmd_tx.send(CoreCmd::ListAll { limit: 512 });

        Ok(Self {
            backend,
            pastes: Vec::new(),
            selected_id: None,
            selected_paste: None,
            selected_content: String::new(),
            db_path,
            status: None,
            style_applied: false,
        })
    }

    fn ensure_style(&mut self, ctx: &egui::Context) {
        if self.style_applied {
            return;
        }

        let mut style = (*ctx.style()).clone();
        style.visuals = Visuals::dark();
        style.visuals.override_text_color = Some(COLOR_TEXT_PRIMARY);
        style.visuals.window_fill = COLOR_BG_PRIMARY;
        style.visuals.panel_fill = COLOR_BG_SECONDARY;
        style.visuals.extreme_bg_color = COLOR_BG_PRIMARY;
        style.visuals.faint_bg_color = COLOR_BG_TERTIARY;
        style.visuals.window_stroke = Stroke::new(1.0, COLOR_BORDER);
        style.visuals.hyperlink_color = COLOR_ACCENT;
        style.visuals.selection.bg_fill = COLOR_ACCENT;
        style.visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);
        style.visuals.widgets.inactive.bg_fill = COLOR_BG_TERTIARY;
        style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, COLOR_BORDER);
        style.visuals.widgets.hovered.bg_fill = COLOR_ACCENT_HOVER;
        style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, COLOR_ACCENT_HOVER);
        style.visuals.widgets.active.bg_fill = COLOR_ACCENT;
        style.visuals.widgets.active.bg_stroke = Stroke::new(1.0, COLOR_ACCENT);
        style.visuals.text_edit_bg_color = Some(COLOR_BG_TERTIARY);

        style.spacing.window_margin = Margin::same(12);
        style.spacing.item_spacing = egui::vec2(10.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.spacing.interact_size.y = 30.0;

        style.text_styles.insert(
            TextStyle::Heading,
            FontId::new(22.0, FontFamily::Proportional),
        );
        style
            .text_styles
            .insert(TextStyle::Body, FontId::new(15.0, FontFamily::Proportional));
        style.text_styles.insert(
            TextStyle::Monospace,
            FontId::new(14.0, FontFamily::Monospace),
        );
        style.text_styles.insert(
            TextStyle::Small,
            FontId::new(12.0, FontFamily::Proportional),
        );

        ctx.set_style(style);
        self.style_applied = true;
    }

    fn apply_event(&mut self, event: CoreEvent) {
        match event {
            CoreEvent::PasteList { items } => {
                self.pastes = items;
                let selection_valid = self
                    .selected_id
                    .as_ref()
                    .map(|id| self.pastes.iter().any(|p| p.id == *id))
                    .unwrap_or(false);
                if !selection_valid {
                    if let Some(first) = self.pastes.first() {
                        self.select_paste(first.id.clone());
                    } else {
                        self.selected_id = None;
                        self.selected_paste = None;
                        self.selected_content.clear();
                    }
                }
            }
            CoreEvent::PasteLoaded { paste } => {
                if self.selected_id.as_deref() == Some(paste.id.as_str()) {
                    self.selected_content = paste.content.clone();
                    self.selected_paste = Some(paste);
                }
            }
            CoreEvent::PasteMissing { id } => {
                self.pastes.retain(|paste| paste.id != id);
                if self.selected_id.as_deref() == Some(id.as_str()) {
                    self.selected_id = None;
                    self.selected_paste = None;
                    self.selected_content.clear();
                    self.status = Some("Selected paste was deleted; list refreshed.".to_string());
                } else {
                    self.status = Some("Paste was deleted; list refreshed.".to_string());
                }
                self.request_refresh();
            }
            CoreEvent::Error { message } => {
                warn!("backend error: {}", message);
                self.status = Some(message);
            }
        }
    }

    fn request_refresh(&self) {
        let _ = self.backend.cmd_tx.send(CoreCmd::ListAll { limit: 512 });
    }

    fn select_paste(&mut self, id: String) {
        self.selected_id = Some(id.clone());
        self.selected_paste = None;
        self.selected_content.clear();
        let _ = self.backend.cmd_tx.send(CoreCmd::GetPaste { id });
    }

    fn selected_index(&self) -> Option<usize> {
        let id = self.selected_id.as_ref()?;
        self.pastes.iter().position(|paste| paste.id == *id)
    }
}

impl eframe::App for LocalPasteApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ensure_style(ctx);

        while let Ok(event) = self.backend.evt_rx.try_recv() {
            self.apply_event(event);
        }

        if !ctx.wants_keyboard_input() && !self.pastes.is_empty() {
            let mut direction: i32 = 0;
            ctx.input(|input| {
                if input.key_pressed(egui::Key::ArrowDown) {
                    direction = 1;
                } else if input.key_pressed(egui::Key::ArrowUp) {
                    direction = -1;
                }
            });

            if direction != 0 {
                let current = self.selected_index().unwrap_or(0) as i32;
                let max_index = (self.pastes.len() - 1) as i32;
                let next = (current + direction).clamp(0, max_index) as usize;
                if self.selected_index() != Some(next) {
                    let next_id = self.pastes[next].id.clone();
                    self.select_paste(next_id);
                }
            }
        }

        egui::TopBottomPanel::top("top")
            .resizable(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(RichText::new("LocalPaste Native").color(COLOR_ACCENT));
                    ui.add_space(12.0);
                    if ui.button("Refresh").clicked() {
                        self.request_refresh();
                    }
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new(&self.db_path)
                            .monospace()
                            .color(COLOR_TEXT_MUTED),
                    );
                });
            });

        egui::SidePanel::left("sidebar")
            .default_width(260.0)
            .show(ctx, |ui| {
                ui.heading(RichText::new(format!("Pastes ({})", self.pastes.len())));
                ui.add_space(8.0);
                let mut pending_select: Option<String> = None;
                let row_height = 28.0;
                egui::ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .show_rows(ui, row_height, self.pastes.len(), |ui, range| {
                        for idx in range {
                            if let Some(paste) = self.pastes.get(idx) {
                                let selected =
                                    self.selected_id.as_deref() == Some(paste.id.as_str());
                                let label = match &paste.language {
                                    Some(lang) => format!("{}  ({})", paste.name, lang),
                                    None => paste.name.clone(),
                                };
                                if ui
                                    .selectable_label(selected, RichText::new(label))
                                    .clicked()
                                {
                                    pending_select = Some(paste.id.clone());
                                }
                            }
                        }
                    });
                if let Some(id) = pending_select {
                    self.select_paste(id);
                }
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading(RichText::new("Editor").color(COLOR_TEXT_PRIMARY));
            ui.add_space(12.0);

            let selected_meta = self
                .selected_paste
                .as_ref()
                .map(|paste| (paste.name.clone(), paste.language.clone(), paste.id.clone()));

            if let Some((name, language, id)) = selected_meta {
                ui.horizontal(|ui| {
                    ui.heading(RichText::new(name).color(COLOR_TEXT_PRIMARY));
                    ui.add_space(8.0);
                    if let Some(lang) = language {
                        ui.label(
                            RichText::new(format!("({})", lang))
                                .color(COLOR_TEXT_MUTED)
                                .small(),
                        );
                    }
                });
                ui.label(
                    RichText::new(id)
                        .small()
                        .monospace()
                        .color(COLOR_TEXT_MUTED),
                );
                ui.add_space(8.0);
                ui.add_enabled(
                    false,
                    egui::TextEdit::multiline(&mut self.selected_content)
                        .desired_width(f32::INFINITY)
                        .desired_rows(18),
                );
            } else if self.selected_id.is_some() {
                ui.label("Loading paste...");
            } else {
                ui.label("Select a paste from the sidebar.");
            }
        });

        egui::TopBottomPanel::bottom("status")
            .resizable(false)
            .show(ctx, |ui| {
                if let Some(status) = &self.status {
                    ui.label(egui::RichText::new(status).color(egui::Color32::YELLOW));
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::unbounded;

    fn make_app() -> LocalPasteApp {
        let (cmd_tx, _cmd_rx) = unbounded();
        let (_evt_tx, evt_rx) = unbounded();
        LocalPasteApp {
            backend: BackendHandle { cmd_tx, evt_rx },
            pastes: vec![PasteSummary {
                id: "alpha".to_string(),
                name: "Alpha".to_string(),
                language: None,
            }],
            selected_id: Some("alpha".to_string()),
            selected_paste: Some(Paste::new("content".to_string(), "Alpha".to_string())),
            selected_content: "content".to_string(),
            db_path: "test".to_string(),
            status: None,
        }
    }

    #[test]
    fn paste_missing_clears_selection_and_removes_list_entry() {
        let mut app = make_app();
        app.apply_event(CoreEvent::PasteMissing {
            id: "alpha".to_string(),
        });

        assert!(app.pastes.is_empty());
        assert!(app.selected_id.is_none());
        assert!(app.selected_paste.is_none());
        assert!(app.selected_content.is_empty());
        assert!(app.status.is_some());
    }

    #[test]
    fn paste_missing_non_selected_removes_list_entry() {
        let mut app = make_app();
        app.pastes.push(PasteSummary {
            id: "beta".to_string(),
            name: "Beta".to_string(),
            language: None,
        });

        app.apply_event(CoreEvent::PasteMissing {
            id: "beta".to_string(),
        });

        assert_eq!(app.pastes.len(), 1);
        assert_eq!(app.pastes[0].id, "alpha");
        assert_eq!(app.selected_id.as_deref(), Some("alpha"));
        assert!(app.selected_paste.is_some());
    }
}
