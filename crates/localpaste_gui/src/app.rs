//! Native egui app skeleton for the LocalPaste rewrite.

use crate::backend::{spawn_backend, BackendHandle, CoreCmd, CoreEvent, PasteSummary};
use eframe::egui::{
    self, style::WidgetVisuals, Color32, CornerRadius, FontData, FontDefinitions, FontFamily,
    FontId, Margin, RichText, Stroke, TextStyle, Visuals,
};
use egui_extras::syntax_highlighting::{self, CodeTheme};
use localpaste_core::models::paste::Paste;
use localpaste_core::{Config, Database};
use localpaste_server::{AppState, EmbeddedServer, PasteLockManager};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{info, warn};

/// Native egui application shell for the rewrite.
///
/// Owns the UI state and communicates with the background worker via channels so
/// the `update` loop never blocks on database I/O.
pub struct LocalPasteApp {
    backend: BackendHandle,
    pastes: Vec<PasteSummary>,
    selected_id: Option<String>,
    selected_paste: Option<Paste>,
    selected_content: String,
    db_path: String,
    locks: Arc<PasteLockManager>,
    _server: EmbeddedServer,
    server_addr: SocketAddr,
    server_used_fallback: bool,
    status: Option<StatusMessage>,
    save_status: SaveStatus,
    last_edit_at: Option<Instant>,
    save_in_flight: bool,
    autosave_delay: Duration,
    style_applied: bool,
    last_refresh_at: Instant,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum SaveStatus {
    Saved,
    Dirty,
    Saving,
}

const COLOR_BG_PRIMARY: Color32 = Color32::from_rgb(0x0d, 0x11, 0x17);
const COLOR_BG_SECONDARY: Color32 = Color32::from_rgb(0x16, 0x1b, 0x22);
const COLOR_BG_TERTIARY: Color32 = Color32::from_rgb(0x21, 0x26, 0x29);
const COLOR_TEXT_PRIMARY: Color32 = Color32::from_rgb(0xc9, 0xd1, 0xd9);
const COLOR_TEXT_SECONDARY: Color32 = Color32::from_rgb(0x8b, 0x94, 0x9e);
const COLOR_TEXT_MUTED: Color32 = Color32::from_rgb(0x6e, 0x76, 0x81);
const COLOR_ACCENT: Color32 = Color32::from_rgb(0xE5, 0x70, 0x00);
const COLOR_ACCENT_HOVER: Color32 = Color32::from_rgb(0xCE, 0x42, 0x2B);
const COLOR_BORDER: Color32 = Color32::from_rgb(0x30, 0x36, 0x3d);
const AUTO_REFRESH_INTERVAL: Duration = Duration::from_secs(3);
const STATUS_TTL: Duration = Duration::from_secs(5);
const FONT_0XPROTO: &str = "0xProto";
const EDITOR_FONT_FAMILY: &str = "Editor";
const EDITOR_TEXT_STYLE: &str = "Editor";
const HIGHLIGHT_PLAIN_THRESHOLD: usize = 256 * 1024;

struct StatusMessage {
    text: String,
    expires_at: Instant,
}

fn syntect_language_hint(language: &str) -> String {
    let lang = language.trim().to_ascii_lowercase();
    match lang.as_str() {
        "python" => "py".to_string(),
        "javascript" | "js" => "js".to_string(),
        "typescript" | "ts" => "ts".to_string(),
        "markdown" | "md" => "md".to_string(),
        "csharp" | "cs" => "cs".to_string(),
        "cpp" | "c++" => "cpp".to_string(),
        "shell" | "bash" | "sh" => "sh".to_string(),
        "plaintext" | "plain" | "text" => "txt".to_string(),
        "yaml" | "yml" => "yaml".to_string(),
        "toml" => "toml".to_string(),
        "json" => "json".to_string(),
        "rust" => "rs".to_string(),
        "go" => "go".to_string(),
        "html" => "html".to_string(),
        "xml" => "xml".to_string(),
        "sql" => "sql".to_string(),
        _ => lang,
    }
}

impl LocalPasteApp {
    /// Construct a new app instance from the current environment config.
    ///
    /// Opens the embedded database, spawns the backend worker thread, and kicks
    /// off the initial list request so the UI has data to render on first paint.
    ///
    /// # Returns
    /// The initialized [`LocalPasteApp`] ready to be handed to `eframe`.
    ///
    /// # Errors
    /// Returns an error if the database path is invalid or the underlying store
    /// cannot be opened.
    pub fn new() -> Result<Self, localpaste_core::AppError> {
        let config = Config::from_env();
        let db_path = config.db_path.clone();
        let autosave_delay = Duration::from_millis(config.auto_save_interval);
        let db = Database::new(&config.db_path)?;
        info!("native GUI opened database at {}", config.db_path);

        let locks = Arc::new(PasteLockManager::default());
        let server_db = db.share()?;
        let state = AppState::with_locks(config.clone(), server_db, locks.clone());
        let allow_public = std::env::var("ALLOW_PUBLIC_ACCESS").is_ok();
        if allow_public {
            warn!("Public access enabled - server will accept requests from any origin");
        }
        let server = EmbeddedServer::start(state, allow_public)?;
        let server_addr = server.addr();
        let server_used_fallback = server.used_fallback();

        let backend = spawn_backend(db);

        let mut app = Self {
            backend,
            pastes: Vec::new(),
            selected_id: None,
            selected_paste: None,
            selected_content: String::new(),
            db_path,
            locks,
            _server: server,
            server_addr,
            server_used_fallback,
            status: None,
            save_status: SaveStatus::Saved,
            last_edit_at: None,
            save_in_flight: false,
            autosave_delay,
            style_applied: false,
            last_refresh_at: Instant::now(),
        };
        app.request_refresh();
        Ok(app)
    }

    fn ensure_style(&mut self, ctx: &egui::Context) {
        if self.style_applied {
            return;
        }

        let mut fonts = FontDefinitions::default();
        fonts.font_data.insert(
            FONT_0XPROTO.to_string(),
            FontData::from_static(include_bytes!(
                "../../../assets/fonts/0xProto/0xProto-Regular-NL.ttf"
            ))
            .into(),
        );
        let editor_family = FontFamily::Name(EDITOR_FONT_FAMILY.into());
        fonts.families.insert(
            editor_family.clone(),
            vec![
                FONT_0XPROTO.to_string(),
                "Hack".to_string(),
                "Ubuntu-Light".to_string(),
                "NotoEmoji-Regular".to_string(),
                "emoji-icon-font".to_string(),
            ],
        );
        let editor_font_ready = fonts.font_data.contains_key(FONT_0XPROTO);
        if !editor_font_ready {
            warn!("0xProto font missing; falling back to monospace in editor");
        }
        ctx.set_fonts(fonts);

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
        style.visuals.text_edit_bg_color = Some(COLOR_BG_TERTIARY);

        style.visuals.widgets.noninteractive = WidgetVisuals {
            bg_fill: COLOR_BG_SECONDARY,
            weak_bg_fill: COLOR_BG_SECONDARY,
            bg_stroke: Stroke::new(1.0, COLOR_BORDER),
            corner_radius: CornerRadius::same(6),
            fg_stroke: Stroke::new(1.0, COLOR_TEXT_SECONDARY),
            expansion: 0.0,
        };
        style.visuals.widgets.inactive = WidgetVisuals {
            bg_fill: COLOR_BG_TERTIARY,
            weak_bg_fill: COLOR_BG_TERTIARY,
            bg_stroke: Stroke::new(1.0, COLOR_BORDER),
            corner_radius: CornerRadius::same(6),
            fg_stroke: Stroke::new(1.0, COLOR_TEXT_PRIMARY),
            expansion: 0.0,
        };
        style.visuals.widgets.hovered = WidgetVisuals {
            bg_fill: COLOR_ACCENT_HOVER,
            weak_bg_fill: COLOR_ACCENT_HOVER,
            bg_stroke: Stroke::new(1.0, COLOR_ACCENT_HOVER),
            corner_radius: CornerRadius::same(6),
            fg_stroke: Stroke::new(1.0, Color32::WHITE),
            expansion: 0.5,
        };
        style.visuals.widgets.active = WidgetVisuals {
            bg_fill: COLOR_ACCENT,
            weak_bg_fill: COLOR_ACCENT,
            bg_stroke: Stroke::new(1.0, COLOR_ACCENT),
            corner_radius: CornerRadius::same(6),
            fg_stroke: Stroke::new(1.0, Color32::WHITE),
            expansion: 0.5,
        };
        style.visuals.widgets.open = WidgetVisuals {
            bg_fill: COLOR_ACCENT,
            weak_bg_fill: COLOR_ACCENT,
            bg_stroke: Stroke::new(1.0, COLOR_ACCENT),
            corner_radius: CornerRadius::same(6),
            fg_stroke: Stroke::new(1.0, Color32::WHITE),
            expansion: 0.0,
        };

        style.spacing.window_margin = Margin::same(12);
        style.spacing.button_padding = egui::vec2(14.0, 8.0);
        style.spacing.item_spacing = egui::vec2(12.0, 8.0);
        style.spacing.interact_size.y = 34.0;
        style.spacing.text_edit_width = 280.0;
        style.spacing.indent = 18.0;
        style.spacing.menu_margin = Margin::same(8);
        style.spacing.combo_width = 220.0;

        style.text_styles.insert(
            TextStyle::Heading,
            FontId::new(24.0, FontFamily::Proportional),
        );
        style
            .text_styles
            .insert(TextStyle::Body, FontId::new(16.0, FontFamily::Proportional));
        style.text_styles.insert(
            TextStyle::Button,
            FontId::new(15.0, FontFamily::Proportional),
        );
        style.text_styles.insert(
            TextStyle::Monospace,
            FontId::new(15.0, FontFamily::Monospace),
        );
        style.text_styles.insert(
            TextStyle::Name(EDITOR_TEXT_STYLE.into()),
            FontId::new(
                15.0,
                if editor_font_ready {
                    FontFamily::Name(EDITOR_FONT_FAMILY.into())
                } else {
                    FontFamily::Monospace
                },
            ),
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
                        self.clear_selection();
                    }
                }
            }
            CoreEvent::PasteLoaded { paste } => {
                if self.selected_id.as_deref() == Some(paste.id.as_str()) {
                    self.selected_content = paste.content.clone();
                    self.selected_paste = Some(paste);
                    self.save_status = SaveStatus::Saved;
                    self.last_edit_at = None;
                    self.save_in_flight = false;
                }
            }
            CoreEvent::PasteCreated { paste } => {
                let summary = PasteSummary::from_paste(&paste);
                self.pastes.insert(0, summary);
                self.select_paste(paste.id.clone());
                self.selected_content = paste.content.clone();
                self.selected_paste = Some(paste);
                self.save_status = SaveStatus::Saved;
                self.last_edit_at = None;
                self.save_in_flight = false;
                self.set_status("Created new paste.");
            }
            CoreEvent::PasteSaved { paste } => {
                if let Some(item) = self.pastes.iter_mut().find(|item| item.id == paste.id) {
                    *item = PasteSummary::from_paste(&paste);
                }
                if self.selected_id.as_deref() == Some(paste.id.as_str()) {
                    let mut updated = paste;
                    updated.content = self.selected_content.clone();
                    self.selected_paste = Some(updated);
                    self.save_status = SaveStatus::Saved;
                    self.last_edit_at = None;
                    self.save_in_flight = false;
                }
            }
            CoreEvent::PasteDeleted { id } => {
                self.pastes.retain(|paste| paste.id != id);
                if self.selected_id.as_deref() == Some(id.as_str()) {
                    self.clear_selection();
                    self.set_status("Paste deleted.");
                } else {
                    self.set_status("Paste deleted; list refreshed.");
                }
                self.request_refresh();
            }
            CoreEvent::PasteMissing { id } => {
                self.pastes.retain(|paste| paste.id != id);
                if self.selected_id.as_deref() == Some(id.as_str()) {
                    self.clear_selection();
                    self.set_status("Selected paste was deleted; list refreshed.");
                } else {
                    self.set_status("Paste was deleted; list refreshed.");
                }
                self.request_refresh();
            }
            CoreEvent::Error { message } => {
                warn!("backend error: {}", message);
                self.set_status(message);
                if self.save_status == SaveStatus::Saving {
                    self.save_status = SaveStatus::Dirty;
                }
                self.save_in_flight = false;
            }
        }
    }

    fn request_refresh(&mut self) {
        let _ = self.backend.cmd_tx.send(CoreCmd::ListAll { limit: 512 });
        self.last_refresh_at = Instant::now();
    }

    fn select_paste(&mut self, id: String) {
        if let Some(prev) = self.selected_id.take() {
            self.locks.unlock(&prev);
        }
        self.selected_id = Some(id.clone());
        self.locks.lock(&id);
        self.selected_paste = None;
        self.selected_content.clear();
        self.save_status = SaveStatus::Saved;
        self.last_edit_at = None;
        self.save_in_flight = false;
        let _ = self.backend.cmd_tx.send(CoreCmd::GetPaste { id });
    }

    fn clear_selection(&mut self) {
        if let Some(prev) = self.selected_id.take() {
            self.locks.unlock(&prev);
        }
        self.selected_paste = None;
        self.selected_content.clear();
        self.save_status = SaveStatus::Saved;
        self.last_edit_at = None;
        self.save_in_flight = false;
    }

    fn set_status(&mut self, text: impl Into<String>) {
        self.status = Some(StatusMessage {
            text: text.into(),
            expires_at: Instant::now() + STATUS_TTL,
        });
    }

    fn create_new_paste(&mut self) {
        let _ = self.backend.cmd_tx.send(CoreCmd::CreatePaste {
            content: String::new(),
        });
    }

    fn delete_selected(&mut self) {
        if let Some(id) = self.selected_id.clone() {
            self.locks.unlock(&id);
            let _ = self.backend.cmd_tx.send(CoreCmd::DeletePaste { id });
        }
    }

    fn mark_dirty(&mut self) {
        if self.selected_id.is_some() {
            self.save_status = SaveStatus::Dirty;
            self.last_edit_at = Some(Instant::now());
        }
    }

    fn maybe_autosave(&mut self) {
        if self.save_in_flight || self.save_status != SaveStatus::Dirty {
            return;
        }
        let Some(last_edit) = self.last_edit_at else {
            return;
        };
        if last_edit.elapsed() < self.autosave_delay {
            return;
        }
        let Some(id) = self.selected_id.clone() else {
            return;
        };
        let content = self.selected_content.clone();
        self.save_in_flight = true;
        self.save_status = SaveStatus::Saving;
        let _ = self
            .backend
            .cmd_tx
            .send(CoreCmd::UpdatePaste { id, content });
    }

    fn selected_index(&self) -> Option<usize> {
        let id = self.selected_id.as_ref()?;
        self.pastes.iter().position(|paste| paste.id == *id)
    }
}

impl eframe::App for LocalPasteApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ensure_style(ctx);

        if let Some(status) = &self.status {
            if Instant::now() >= status.expires_at {
                self.status = None;
            }
        }

        while let Ok(event) = self.backend.evt_rx.try_recv() {
            self.apply_event(event);
        }

        ctx.input(|input| {
            if input.modifiers.command && input.key_pressed(egui::Key::N) {
                self.create_new_paste();
            }
            if input.modifiers.command && input.key_pressed(egui::Key::Delete) {
                self.delete_selected();
            }
        });

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
                    ui.heading(RichText::new("LocalPaste.rs").color(COLOR_ACCENT));
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new(&self.db_path)
                            .monospace()
                            .color(COLOR_TEXT_SECONDARY),
                    );
                });
            });

        egui::SidePanel::left("sidebar")
            .default_width(260.0)
            .show(ctx, |ui| {
                ui.heading(
                    RichText::new(format!("Pastes ({})", self.pastes.len()))
                        .color(COLOR_TEXT_PRIMARY),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("+ New Paste").clicked() {
                        self.create_new_paste();
                    }
                    if ui
                        .add_enabled(self.selected_id.is_some(), egui::Button::new("Delete"))
                        .clicked()
                    {
                        self.delete_selected();
                    }
                });
                ui.add_space(8.0);
                let mut pending_select: Option<String> = None;
                let row_height = ui.spacing().interact_size.y;
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
                    if let Some(lang) = language.as_deref() {
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
                let editor_height = ui.available_height();
                let mut response = None;
                egui::ScrollArea::vertical()
                    .id_salt("editor_scroll")
                    .max_height(editor_height)
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        let editor_style = TextStyle::Name(EDITOR_TEXT_STYLE.into());
                        let editor_font = ui.style().text_styles.get(&editor_style).cloned();
                        let language_hint =
                            syntect_language_hint(language.as_deref().unwrap_or("text"));
                        let use_plain = self.selected_content.len() >= HIGHLIGHT_PLAIN_THRESHOLD;

                        let edit = egui::TextEdit::multiline(&mut self.selected_content)
                            .font(editor_style)
                            .desired_width(f32::INFINITY)
                            .desired_rows(1)
                            .lock_focus(true)
                            .hint_text("Start typing...");

                        let edit = if use_plain {
                            ui.add(edit)
                        } else {
                            let theme = CodeTheme::from_memory(ui.ctx(), ui.style());
                            let mut layouter =
                                move |ui: &egui::Ui,
                                      text: &dyn egui::TextBuffer,
                                      wrap_width: f32| {
                                    let text = text.as_str();
                                    let mut job = syntax_highlighting::highlight(
                                        ui.ctx(),
                                        ui.style(),
                                        &theme,
                                        text,
                                        language_hint.as_str(),
                                    );
                                    job.wrap.max_width = wrap_width;
                                    if let Some(font_id) = &editor_font {
                                        for section in &mut job.sections {
                                            section.format.font_id =
                                                FontId::new(font_id.size, font_id.family.clone());
                                        }
                                    }
                                    ui.fonts_mut(|f| f.layout_job(job))
                                };
                            ui.add(edit.layouter(&mut layouter))
                        };
                        response = Some(edit);
                    });
                if response.map(|r| r.changed()).unwrap_or(false) {
                    self.mark_dirty();
                }
            } else if self.selected_id.is_some() {
                ui.label(RichText::new("Loading paste...").color(COLOR_TEXT_MUTED));
            } else {
                ui.label(RichText::new("Select a paste from the sidebar.").color(COLOR_TEXT_MUTED));
            }
        });

        egui::TopBottomPanel::bottom("status")
            .resizable(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if self.selected_id.is_some() {
                        let (label, color) = match self.save_status {
                            SaveStatus::Saved => ("Saved", COLOR_TEXT_SECONDARY),
                            SaveStatus::Dirty => ("Unsaved", egui::Color32::YELLOW),
                            SaveStatus::Saving => ("Saving...", COLOR_TEXT_MUTED),
                        };
                        ui.label(egui::RichText::new(label).color(color));
                        ui.separator();
                    }
                    if let Some(status) = &self.status {
                        ui.label(egui::RichText::new(&status.text).color(egui::Color32::YELLOW));
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let api_label = if self.server_used_fallback {
                        format!("API: http://{} (auto)", self.server_addr)
                    } else {
                        format!("API: http://{}", self.server_addr)
                    };
                    ui.label(
                        egui::RichText::new(api_label)
                            .small()
                            .color(COLOR_TEXT_SECONDARY),
                    );
                    if self.selected_id.is_some() {
                        ui.separator();
                        ui.label(
                            egui::RichText::new(format!(
                                "{} chars",
                                self.selected_content.chars().count()
                            ))
                            .small()
                            .color(COLOR_TEXT_MUTED),
                        );
                    }
                });
            });

        self.maybe_autosave();
        if self.last_refresh_at.elapsed() >= AUTO_REFRESH_INTERVAL {
            self.request_refresh();
        }
        let mut repaint_after = if self.save_status == SaveStatus::Dirty {
            self.autosave_delay.min(AUTO_REFRESH_INTERVAL)
        } else {
            AUTO_REFRESH_INTERVAL
        };
        if let Some(status) = &self.status {
            let until = status.expires_at.saturating_duration_since(Instant::now());
            repaint_after = repaint_after.min(until);
        }
        ctx.request_repaint_after(repaint_after);
    }
}

impl Drop for LocalPasteApp {
    fn drop(&mut self) {
        if let Some(id) = self.selected_id.take() {
            self.locks.unlock(&id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::unbounded;
    use tempfile::TempDir;

    struct TestHarness {
        _dir: TempDir,
        app: LocalPasteApp,
    }

    fn make_app() -> TestHarness {
        let (cmd_tx, _cmd_rx) = unbounded();
        let (_evt_tx, evt_rx) = unbounded();
        let dir = TempDir::new().expect("temp dir");
        let db_path = dir.path().join("db");
        let db_path_str = db_path.to_string_lossy().to_string();
        let db = Database::new(&db_path_str).expect("db");
        let locks = Arc::new(PasteLockManager::default());
        let server_db = db.share().expect("share db");
        let config = Config {
            db_path: db_path_str.clone(),
            port: 0,
            max_paste_size: 10 * 1024 * 1024,
            auto_save_interval: 2000,
            auto_backup: false,
        };
        let state = AppState::with_locks(config, server_db, locks.clone());
        let server = EmbeddedServer::start(state, false).expect("server");
        let server_addr = server.addr();
        let server_used_fallback = server.used_fallback();

        let app = LocalPasteApp {
            backend: BackendHandle { cmd_tx, evt_rx },
            pastes: vec![PasteSummary {
                id: "alpha".to_string(),
                name: "Alpha".to_string(),
                language: None,
            }],
            selected_id: Some("alpha".to_string()),
            selected_paste: Some(Paste::new("content".to_string(), "Alpha".to_string())),
            selected_content: "content".to_string(),
            db_path: db_path_str,
            locks,
            _server: server,
            server_addr,
            server_used_fallback,
            status: None,
            save_status: SaveStatus::Saved,
            last_edit_at: None,
            save_in_flight: false,
            autosave_delay: Duration::from_millis(2000),
            style_applied: false,
            last_refresh_at: Instant::now(),
        };

        TestHarness { _dir: dir, app }
    }

    #[test]
    fn paste_missing_clears_selection_and_removes_list_entry() {
        let mut harness = make_app();
        harness.app.apply_event(CoreEvent::PasteMissing {
            id: "alpha".to_string(),
        });

        assert!(harness.app.pastes.is_empty());
        assert!(harness.app.selected_id.is_none());
        assert!(harness.app.selected_paste.is_none());
        assert!(harness.app.selected_content.is_empty());
        assert!(harness.app.status.is_some());
    }

    #[test]
    fn paste_missing_non_selected_removes_list_entry() {
        let mut harness = make_app();
        harness.app.pastes.push(PasteSummary {
            id: "beta".to_string(),
            name: "Beta".to_string(),
            language: None,
        });

        harness.app.apply_event(CoreEvent::PasteMissing {
            id: "beta".to_string(),
        });

        assert_eq!(harness.app.pastes.len(), 1);
        assert_eq!(harness.app.pastes[0].id, "alpha");
        assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));
        assert!(harness.app.selected_paste.is_some());
    }
}
