//! Native egui app skeleton for the LocalPaste rewrite.

mod editor;
mod highlight;
mod util;
mod virtual_view;

use crate::backend::{spawn_backend, BackendHandle, CoreCmd, CoreEvent, PasteSummary};
use editor::{EditorBuffer, EditorLineIndex, EditorMode};
use eframe::egui::{
    self,
    style::WidgetVisuals,
    text::{CCursor, CCursorRange},
    text_edit::TextEditOutput,
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, RichText, Stroke,
    TextStyle, Visuals,
};
use egui_extras::syntax_highlighting::CodeTheme;
use highlight::{
    build_virtual_line_job, spawn_highlight_worker, syntect_language_hint, syntect_theme_key,
    EditorLayoutCache, HighlightRender, HighlightRequest, HighlightRequestMeta, HighlightWorker,
    SyntectSettings,
};
use localpaste_core::models::paste::Paste;
use localpaste_core::{Config, Database};
use localpaste_server::{AppState, EmbeddedServer, PasteLockManager};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{info, warn};
use util::{display_language_label, word_range_at};
use virtual_view::{VirtualCursor, VirtualSelectionState};

/// Native egui application shell for the rewrite.
///
/// Owns the UI state and communicates with the background worker via channels so
/// the `update` loop never blocks on database I/O.
pub struct LocalPasteApp {
    backend: BackendHandle,
    pastes: Vec<PasteSummary>,
    selected_id: Option<String>,
    selected_paste: Option<Paste>,
    selected_content: EditorBuffer,
    editor_cache: EditorLayoutCache,
    editor_lines: EditorLineIndex,
    editor_mode: EditorMode,
    virtual_selection: VirtualSelectionState,
    highlight_worker: HighlightWorker,
    highlight_pending: Option<HighlightRequestMeta>,
    highlight_render: Option<HighlightRender>,
    highlight_staged: Option<HighlightRender>,
    highlight_version: u64,
    last_interaction_at: Option<Instant>,
    last_editor_click_at: Option<Instant>,
    last_editor_click_pos: Option<egui::Pos2>,
    syntect: SyntectSettings,
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
    focus_editor_next: bool,
    style_applied: bool,
    window_checked: bool,
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
pub(crate) const DEFAULT_WINDOW_SIZE: [f32; 2] = [1100.0, 720.0];
pub(crate) const MIN_WINDOW_SIZE: [f32; 2] = [900.0, 600.0];
const HIGHLIGHT_PLAIN_THRESHOLD: usize = 256 * 1024;
const HIGHLIGHT_DEBOUNCE: Duration = Duration::from_millis(150);
const HIGHLIGHT_DEBOUNCE_MIN_BYTES: usize = 64 * 1024;
const HIGHLIGHT_APPLY_IDLE: Duration = Duration::from_millis(200);
const EDITOR_DOUBLE_CLICK_WINDOW: Duration = Duration::from_millis(300);
const EDITOR_DOUBLE_CLICK_DISTANCE: f32 = 6.0;

struct StatusMessage {
    text: String,
    expires_at: Instant,
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
        let highlight_worker = spawn_highlight_worker();

        let mut app = Self {
            backend,
            pastes: Vec::new(),
            selected_id: None,
            selected_paste: None,
            selected_content: EditorBuffer::new(String::new()),
            editor_cache: EditorLayoutCache::default(),
            editor_lines: EditorLineIndex::default(),
            editor_mode: EditorMode::from_env(),
            virtual_selection: VirtualSelectionState::default(),
            highlight_worker,
            highlight_pending: None,
            highlight_render: None,
            highlight_staged: None,
            highlight_version: 0,
            syntect: SyntectSettings::default(),
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
            focus_editor_next: false,
            style_applied: false,
            window_checked: false,
            last_refresh_at: Instant::now(),
            last_interaction_at: None,
            last_editor_click_at: None,
            last_editor_click_pos: None,
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
                    self.selected_content.reset(paste.content.clone());
                    self.editor_cache = EditorLayoutCache::default();
                    self.editor_lines.reset();
                    self.virtual_selection.clear();
                    self.clear_highlight_state();
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
                self.selected_content.reset(paste.content.clone());
                self.editor_cache = EditorLayoutCache::default();
                self.editor_lines.reset();
                self.virtual_selection.clear();
                self.clear_highlight_state();
                self.selected_paste = Some(paste);
                self.save_status = SaveStatus::Saved;
                self.last_edit_at = None;
                self.save_in_flight = false;
                self.focus_editor_next = true;
                self.set_status("Created new paste.");
            }
            CoreEvent::PasteSaved { paste } => {
                if let Some(item) = self.pastes.iter_mut().find(|item| item.id == paste.id) {
                    *item = PasteSummary::from_paste(&paste);
                }
                if self.selected_id.as_deref() == Some(paste.id.as_str()) {
                    let mut updated = paste;
                    updated.content = self.selected_content.to_string();
                    self.selected_paste = Some(updated);
                    self.clear_highlight_state();
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
        self.selected_content.reset(String::new());
        self.editor_cache = EditorLayoutCache::default();
        self.editor_lines.reset();
        self.virtual_selection.clear();
        self.clear_highlight_state();
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
        self.selected_content.reset(String::new());
        self.editor_cache = EditorLayoutCache::default();
        self.editor_lines.reset();
        self.virtual_selection.clear();
        self.clear_highlight_state();
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
        self.create_new_paste_with_content(String::new());
    }

    fn create_new_paste_with_content(&mut self, content: String) {
        let _ = self.backend.cmd_tx.send(CoreCmd::CreatePaste { content });
    }

    fn delete_selected(&mut self) {
        if let Some(id) = self.selected_id.clone() {
            self.locks.unlock(&id);
            let _ = self.backend.cmd_tx.send(CoreCmd::DeletePaste { id });
        }
    }

    fn clear_highlight_state(&mut self) {
        self.highlight_pending = None;
        self.highlight_render = None;
        self.highlight_staged = None;
        self.highlight_version = self.highlight_version.wrapping_add(1);
    }

    fn queue_highlight_render(&mut self, render: HighlightRender) {
        let Some(selected_id) = self.selected_id.as_deref() else {
            return;
        };
        if render.paste_id != selected_id {
            return;
        }
        if let Some(pending) = &self.highlight_pending {
            if pending.matches_render(&render) {
                self.highlight_pending = None;
            }
        }
        self.highlight_staged = Some(render);
    }

    fn apply_staged_highlight(&mut self) {
        let Some(render) = self.highlight_staged.take() else {
            return;
        };
        self.highlight_render = Some(render);
        self.highlight_version = self.highlight_version.wrapping_add(1);
    }

    fn maybe_apply_staged_highlight(&mut self, now: Instant) {
        if self.highlight_staged.is_none() {
            return;
        }
        let idle = self
            .last_interaction_at
            .map(|last| now.duration_since(last) >= HIGHLIGHT_APPLY_IDLE)
            .unwrap_or(true);
        if idle {
            self.apply_staged_highlight();
        }
    }

    fn handle_large_editor_click(
        &mut self,
        output: &TextEditOutput,
        text: &str,
        is_large_buffer: bool,
    ) {
        if !is_large_buffer || !output.response.clicked() {
            return;
        }
        let now = Instant::now();
        let click_pos = output.response.interact_pointer_pos();
        let is_double = if let (Some(last_at), Some(last_pos), Some(pos)) = (
            self.last_editor_click_at,
            self.last_editor_click_pos,
            click_pos,
        ) {
            now.duration_since(last_at) <= EDITOR_DOUBLE_CLICK_WINDOW
                && last_pos.distance(pos) <= EDITOR_DOUBLE_CLICK_DISTANCE
        } else {
            false
        };
        self.last_editor_click_at = Some(now);
        self.last_editor_click_pos = click_pos;

        if !is_double {
            return;
        }
        let Some(range) = output.cursor_range else {
            return;
        };
        let Some((start, end)) = word_range_at(text, range.primary.index) else {
            return;
        };
        let mut state = output.state.clone();
        state.cursor.set_char_range(Some(CCursorRange::two(
            CCursor::new(start),
            CCursor::new(end),
        )));
        state.store(&output.response.ctx, output.response.id);
    }

    fn virtual_selection_text(&mut self) -> Option<String> {
        let (start, end) = self.virtual_selection.selection_bounds()?;
        let text = self.selected_content.as_str();
        self.editor_lines
            .ensure_for(self.selected_content.revision(), text);
        let mut out = String::new();
        for line_idx in start.line..=end.line {
            let line = self.editor_lines.line_without_newline(text, line_idx);
            let line_chars = line.chars().count();
            let start_char = if line_idx == start.line {
                start.column.min(line_chars)
            } else {
                0
            };
            let end_char = if line_idx == end.line {
                end.column.min(line_chars)
            } else {
                line_chars
            };
            if start_char < end_char {
                let start_byte =
                    egui::text_selection::text_cursor_state::byte_index_from_char_index(
                        line, start_char,
                    );
                let end_byte = egui::text_selection::text_cursor_state::byte_index_from_char_index(
                    line, end_char,
                );
                out.push_str(&line[start_byte..end_byte]);
            }
            if line_idx < end.line {
                out.push('\n');
            }
        }
        if out.is_empty() {
            None
        } else {
            Some(out)
        }
    }

    fn should_request_highlight(
        &self,
        revision: u64,
        text_len: usize,
        language_hint: &str,
        theme_key: &str,
        debounce_active: bool,
        paste_id: &str,
    ) -> bool {
        if text_len >= HIGHLIGHT_PLAIN_THRESHOLD {
            return false;
        }
        if let Some(pending) = &self.highlight_pending {
            if pending.matches(revision, text_len, language_hint, theme_key, paste_id) {
                return false;
            }
        }
        if let Some(render) = &self.highlight_render {
            if render.matches_exact(revision, text_len, language_hint, theme_key, paste_id) {
                return false;
            }
        }
        if let Some(render) = &self.highlight_staged {
            if render.matches_exact(revision, text_len, language_hint, theme_key, paste_id) {
                return false;
            }
        }
        if debounce_active && (self.highlight_pending.is_some() || self.highlight_render.is_some())
        {
            return false;
        }
        true
    }

    fn dispatch_highlight_request(
        &mut self,
        revision: u64,
        text: String,
        language_hint: &str,
        theme_key: &str,
        paste_id: &str,
    ) {
        let text_len = text.len();
        let request = HighlightRequest {
            paste_id: paste_id.to_string(),
            revision,
            text,
            language_hint: language_hint.to_string(),
            theme_key: theme_key.to_string(),
        };
        self.highlight_pending = Some(HighlightRequestMeta {
            paste_id: paste_id.to_string(),
            revision,
            text_len,
            language_hint: language_hint.to_string(),
            theme_key: theme_key.to_string(),
        });
        let _ = self.highlight_worker.tx.send(request);
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
        let content = self.selected_content.to_string();
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
        if !self.window_checked {
            let min_size = egui::vec2(MIN_WINDOW_SIZE[0], MIN_WINDOW_SIZE[1]);
            let current_size = ctx.input(|input| {
                input
                    .viewport()
                    .inner_rect
                    .map(|rect| rect.size())
                    .unwrap_or(min_size)
            });
            if current_size.x < min_size.x || current_size.y < min_size.y {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(min_size));
            }
            self.window_checked = true;
        }

        if let Some(status) = &self.status {
            if Instant::now() >= status.expires_at {
                self.status = None;
            }
        }

        while let Ok(event) = self.backend.evt_rx.try_recv() {
            self.apply_event(event);
        }

        while let Ok(render) = self.highlight_worker.rx.try_recv() {
            self.queue_highlight_render(render);
        }

        let mut copy_virtual = false;
        ctx.input(|input| {
            if !input.events.is_empty() || input.pointer.any_down() {
                self.last_interaction_at = Some(Instant::now());
            }
            if input.modifiers.command && input.key_pressed(egui::Key::N) {
                self.create_new_paste();
            }
            if input.modifiers.command && input.key_pressed(egui::Key::Delete) {
                self.delete_selected();
            }
            if input.modifiers.command && input.key_pressed(egui::Key::C) {
                copy_virtual = true;
            }
        });
        if copy_virtual
            && self.editor_mode == EditorMode::VirtualPreview
            && !ctx.wants_keyboard_input()
        {
            if let Some(selection) = self.virtual_selection_text() {
                ctx.copy_text(selection);
            }
        }

        if self.highlight_staged.is_some() {
            self.maybe_apply_staged_highlight(Instant::now());
        }

        let mut pasted_text: Option<String> = None;
        ctx.input(|input| {
            for event in &input.events {
                if let egui::Event::Paste(text) = event {
                    pasted_text = Some(text.clone());
                }
            }
        });
        if !ctx.wants_keyboard_input() {
            if let Some(text) = pasted_text {
                if !text.trim().is_empty() {
                    self.create_new_paste_with_content(text);
                }
            }
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
                                let lang_label = display_language_label(
                                    paste.language.as_deref(),
                                    paste.content_len >= HIGHLIGHT_PLAIN_THRESHOLD,
                                );
                                let label = format!("{}  ({})", paste.name, lang_label);
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
                let is_large = self.selected_content.len() >= HIGHLIGHT_PLAIN_THRESHOLD;
                let lang_label = display_language_label(language.as_deref(), is_large);
                ui.horizontal(|ui| {
                    ui.heading(RichText::new(name).color(COLOR_TEXT_PRIMARY));
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(format!("({})", lang_label))
                            .color(COLOR_TEXT_MUTED)
                            .small(),
                    );
                });
                ui.label(
                    RichText::new(id.clone())
                        .small()
                        .monospace()
                        .color(COLOR_TEXT_MUTED),
                );
                ui.add_space(8.0);
                if self.editor_mode == EditorMode::VirtualPreview {
                    ui.label(
                        RichText::new("Virtual preview (read-only)")
                            .small()
                            .color(COLOR_TEXT_MUTED),
                    );
                    ui.add_space(4.0);
                }
                let editor_height = ui.available_height();
                let mut response = None;
                let editor_style = TextStyle::Name(EDITOR_TEXT_STYLE.into());
                let editor_font = ui
                    .style()
                    .text_styles
                    .get(&editor_style)
                    .cloned()
                    .unwrap_or_else(|| TextStyle::Monospace.resolve(ui.style()));
                let language_hint = syntect_language_hint(language.as_deref().unwrap_or("text"));
                let debounce_active = self
                    .last_edit_at
                    .map(|last| {
                        self.selected_content.len() >= HIGHLIGHT_DEBOUNCE_MIN_BYTES
                            && last.elapsed() < HIGHLIGHT_DEBOUNCE
                    })
                    .unwrap_or(false);
                let theme = (!is_large).then(|| CodeTheme::from_memory(ui.ctx(), ui.style()));
                let theme_key = theme
                    .as_ref()
                    .map(syntect_theme_key)
                    .unwrap_or("base16-mocha.dark");
                let revision = self.selected_content.revision();
                let text_len = self.selected_content.len();
                let async_mode = text_len >= HIGHLIGHT_DEBOUNCE_MIN_BYTES && !is_large;
                let should_request = async_mode
                    && self.should_request_highlight(
                        revision,
                        text_len,
                        &language_hint,
                        theme_key,
                        debounce_active,
                        id.as_str(),
                    );
                if should_request {
                    let content_snapshot = self.selected_content.to_string();
                    self.dispatch_highlight_request(
                        revision,
                        content_snapshot,
                        &language_hint,
                        theme_key,
                        id.as_str(),
                    );
                }
                let has_render = self
                    .highlight_render
                    .as_ref()
                    .filter(|render| render.matches_context(id.as_str(), &language_hint, theme_key))
                    .is_some();
                let use_plain = if is_large {
                    true
                } else if async_mode {
                    !has_render
                } else {
                    debounce_active
                };
                let highlight_render = self.highlight_render.take();
                let highlight_render_match = highlight_render.as_ref().filter(|render| {
                    render.matches_context(id.as_str(), &language_hint, theme_key)
                });
                let row_height = ui.text_style_height(&editor_style);
                let use_virtual_preview = self.editor_mode == EditorMode::VirtualPreview;

                let scroll = egui::ScrollArea::vertical()
                    .id_salt("editor_scroll")
                    .max_height(editor_height)
                    .auto_shrink([false; 2]);
                if use_virtual_preview {
                    let text = self.selected_content.as_str();
                    self.editor_lines
                        .ensure_for(self.selected_content.revision(), text);
                    let line_count = self.editor_lines.line_count();
                    scroll.show_rows(ui, row_height, line_count, |ui, range| {
                        ui.set_min_width(ui.available_width());
                        let sense = egui::Sense::click_and_drag();
                        struct RowRender {
                            line_idx: usize,
                            rect: egui::Rect,
                            galley: Arc<egui::Galley>,
                            line_chars: usize,
                        }
                        enum RowAction<'a> {
                            Double {
                                cursor: VirtualCursor,
                                line: &'a str,
                            },
                            DragStart {
                                cursor: VirtualCursor,
                            },
                            Click {
                                cursor: VirtualCursor,
                            },
                        }
                        let mut rows = Vec::with_capacity(range.len());
                        let mut pending_action: Option<RowAction<'_>> = None;
                        for line_idx in range {
                            let line = self.editor_lines.line_without_newline(text, line_idx);
                            let render_line = highlight_render_match
                                .and_then(|render| render.lines.get(line_idx));
                            let job = build_virtual_line_job(
                                ui,
                                line,
                                &editor_font,
                                render_line,
                                use_plain,
                            );
                            let line_chars = line.chars().count();
                            let galley = ui.fonts_mut(|f| f.layout_job(job));
                            let row_width = ui.available_width();
                            let (rect, response) =
                                ui.allocate_exact_size(egui::vec2(row_width, row_height), sense);
                            if response.hovered() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
                            }
                            if pending_action.is_none()
                                && (response.double_clicked()
                                    || response.drag_started()
                                    || response.clicked())
                            {
                                if let Some(pointer_pos) = response.interact_pointer_pos() {
                                    let local_pos = pointer_pos - rect.min;
                                    let cursor = galley.cursor_from_pos(local_pos);
                                    let vcursor = VirtualCursor {
                                        line: line_idx,
                                        column: cursor.index,
                                    };
                                    if response.double_clicked() {
                                        pending_action = Some(RowAction::Double {
                                            cursor: vcursor,
                                            line,
                                        });
                                    } else if response.drag_started() {
                                        pending_action =
                                            Some(RowAction::DragStart { cursor: vcursor });
                                    } else if response.clicked() {
                                        pending_action = Some(RowAction::Click { cursor: vcursor });
                                    }
                                }
                            }
                            rows.push(RowRender {
                                line_idx,
                                rect,
                                galley,
                                line_chars,
                            });
                        }

                        if let Some(action) = pending_action {
                            match action {
                                RowAction::Double { cursor, line } => {
                                    if let Some((start, end)) = word_range_at(line, cursor.column) {
                                        self.virtual_selection.select_range(
                                            VirtualCursor {
                                                line: cursor.line,
                                                column: start,
                                            },
                                            VirtualCursor {
                                                line: cursor.line,
                                                column: end,
                                            },
                                        );
                                    } else {
                                        self.virtual_selection.set_cursor(cursor);
                                    }
                                }
                                RowAction::DragStart { cursor } => {
                                    self.virtual_selection.begin_drag(cursor);
                                }
                                RowAction::Click { cursor } => {
                                    self.virtual_selection.set_cursor(cursor);
                                }
                            }
                        }

                        let pointer_pos = ui.input(|input| input.pointer.interact_pos());
                        let pointer_down = ui.input(|input| input.pointer.primary_down());
                        if pointer_down {
                            if let Some(pointer_pos) = pointer_pos {
                                let target_row = rows
                                    .iter()
                                    .find(|row| {
                                        pointer_pos.y >= row.rect.min.y
                                            && pointer_pos.y <= row.rect.max.y
                                    })
                                    .or_else(|| {
                                        let first = rows.first()?;
                                        let last = rows.last()?;
                                        if pointer_pos.y < first.rect.min.y {
                                            Some(first)
                                        } else if pointer_pos.y > last.rect.max.y {
                                            Some(last)
                                        } else {
                                            None
                                        }
                                    });
                                if let Some(row) = target_row {
                                    let clamped_pos = egui::pos2(
                                        pointer_pos.x.clamp(row.rect.min.x, row.rect.max.x),
                                        pointer_pos.y.clamp(row.rect.min.y, row.rect.max.y),
                                    );
                                    let local_pos = clamped_pos - row.rect.min;
                                    let cursor = row.galley.cursor_from_pos(local_pos);
                                    let vcursor = VirtualCursor {
                                        line: row.line_idx,
                                        column: cursor.index,
                                    };
                                    self.virtual_selection.update_drag(vcursor);
                                }
                            }
                        } else {
                            self.virtual_selection.end_drag();
                        }

                        for row in rows {
                            let mut galley = row.galley;
                            if let Some(selection) = self
                                .virtual_selection
                                .selection_for_line(row.line_idx, row.line_chars)
                            {
                                let cursor_range = CCursorRange::two(
                                    CCursor::new(selection.start),
                                    CCursor::new(selection.end),
                                );
                                egui::text_selection::visuals::paint_text_selection(
                                    &mut galley,
                                    ui.visuals(),
                                    &cursor_range,
                                    None,
                                );
                            }
                            ui.painter()
                                .galley(row.rect.min, galley, ui.visuals().text_color());
                        }
                    });
                } else {
                    scroll.show(ui, |ui| {
                        ui.set_min_size(egui::vec2(ui.available_width(), editor_height));
                        let rows_that_fit = ((editor_height / row_height).ceil() as usize).max(1);

                        let edit = egui::TextEdit::multiline(&mut self.selected_content)
                            .font(editor_style)
                            .desired_width(f32::INFINITY)
                            .desired_rows(rows_that_fit)
                            .lock_focus(true)
                            .hint_text("Start typing...");

                        let mut editor_cache = std::mem::take(&mut self.editor_cache);
                        let syntect = &self.syntect;
                        let highlight_version = self.highlight_version;
                        let mut layouter =
                            |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap_width: f32| {
                                editor_cache.layout(
                                    ui,
                                    text,
                                    wrap_width,
                                    language_hint.as_str(),
                                    use_plain,
                                    theme.as_ref(),
                                    highlight_render_match,
                                    highlight_version,
                                    &editor_font,
                                    syntect,
                                )
                            };
                        let disable_builtin_double_click = async_mode;
                        let previous_double_click = if disable_builtin_double_click {
                            Some(ui.ctx().options_mut(|options| {
                                let previous = options.input_options.max_double_click_delay;
                                options.input_options.max_double_click_delay = 0.0;
                                previous
                            }))
                        } else {
                            None
                        };
                        let output = edit.layouter(&mut layouter).show(ui);
                        if let Some(previous) = previous_double_click {
                            ui.ctx().options_mut(|options| {
                                options.input_options.max_double_click_delay = previous;
                            });
                        }
                        self.editor_cache = editor_cache;
                        if disable_builtin_double_click && output.response.clicked() {
                            let text_snapshot = self.selected_content.to_string();
                            self.handle_large_editor_click(&output, &text_snapshot, true);
                        }
                        if self.focus_editor_next || output.response.clicked() {
                            output.response.request_focus();
                            self.focus_editor_next = false;
                        }
                        response = Some(output.response);
                    });
                }
                self.highlight_render = highlight_render;
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
                                self.selected_content.chars_len()
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
    use eframe::egui::TextBuffer;
    use syntect::util::LinesWithEndings;
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
                content_len: 7,
            }],
            selected_id: Some("alpha".to_string()),
            selected_paste: Some(Paste::new("content".to_string(), "Alpha".to_string())),
            selected_content: EditorBuffer::new("content".to_string()),
            editor_cache: EditorLayoutCache::default(),
            editor_lines: EditorLineIndex::default(),
            editor_mode: EditorMode::TextEdit,
            virtual_selection: VirtualSelectionState::default(),
            highlight_worker: spawn_highlight_worker(),
            highlight_pending: None,
            highlight_render: None,
            highlight_staged: None,
            highlight_version: 0,
            syntect: SyntectSettings::default(),
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
            focus_editor_next: false,
            style_applied: false,
            window_checked: false,
            last_refresh_at: Instant::now(),
            last_interaction_at: None,
            last_editor_click_at: None,
            last_editor_click_pos: None,
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
        assert_eq!(harness.app.selected_content.len(), 0);
        assert!(harness.app.status.is_some());
    }

    #[test]
    fn paste_missing_non_selected_removes_list_entry() {
        let mut harness = make_app();
        harness.app.pastes.push(PasteSummary {
            id: "beta".to_string(),
            name: "Beta".to_string(),
            language: None,
            content_len: 4,
        });

        harness.app.apply_event(CoreEvent::PasteMissing {
            id: "beta".to_string(),
        });

        assert_eq!(harness.app.pastes.len(), 1);
        assert_eq!(harness.app.pastes[0].id, "alpha");
        assert_eq!(harness.app.selected_id.as_deref(), Some("alpha"));
        assert!(harness.app.selected_paste.is_some());
    }

    #[test]
    fn editor_buffer_tracks_char_len() {
        let mut buffer = EditorBuffer::new("ab".to_string());
        assert_eq!(buffer.chars_len(), 2);

        buffer.insert_text("\u{00E9}", 1);
        assert_eq!(buffer.chars_len(), 3);

        buffer.delete_char_range(1..2);
        assert_eq!(buffer.chars_len(), 2);

        buffer.replace_with("xyz");
        assert_eq!(buffer.chars_len(), 3);

        buffer.clear();
        assert_eq!(buffer.chars_len(), 0);
    }

    #[test]
    fn highlight_cache_reuses_layout_when_unchanged() {
        let mut cache = EditorLayoutCache::default();
        let buffer = EditorBuffer::new("def foo():\n    return 1\n".to_string());
        let syntect = SyntectSettings::default();

        egui::__run_test_ctx(|ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let font = FontId::monospace(14.0);
                let theme = CodeTheme::dark(14.0);
                let _ = cache.layout(
                    ui,
                    &buffer,
                    400.0,
                    "py",
                    false,
                    Some(&theme),
                    None,
                    0,
                    &font,
                    &syntect,
                );
                let first_ms = cache.last_highlight_ms;
                let line_count = LinesWithEndings::from(buffer.as_str()).count();
                let _ = cache.layout(
                    ui,
                    &buffer,
                    400.0,
                    "py",
                    false,
                    Some(&theme),
                    None,
                    0,
                    &font,
                    &syntect,
                );

                assert_eq!(cache.last_highlight_ms, first_ms);
                assert_eq!(cache.highlight_line_count(), line_count);
            });
        });
    }

    #[test]
    fn highlight_cache_updates_after_line_edit() {
        let mut cache = EditorLayoutCache::default();
        let mut buffer = EditorBuffer::new("line1\nline2\nline3\n".to_string());
        let syntect = SyntectSettings::default();

        egui::__run_test_ctx(|ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let font = FontId::monospace(14.0);
                let theme = CodeTheme::dark(14.0);
                let _ = cache.layout(
                    ui,
                    &buffer,
                    400.0,
                    "py",
                    false,
                    Some(&theme),
                    None,
                    0,
                    &font,
                    &syntect,
                );

                buffer.insert_text("x", 0);

                let _ = cache.layout(
                    ui,
                    &buffer,
                    400.0,
                    "py",
                    false,
                    Some(&theme),
                    None,
                    0,
                    &font,
                    &syntect,
                );
                let line_count = LinesWithEndings::from(buffer.as_str()).count();
                assert_eq!(cache.highlight_line_count(), line_count);
            });
        });
    }

    #[test]
    fn editor_line_index_tracks_lines_and_trailing_newlines() {
        let buffer = EditorBuffer::new("alpha\nbeta\n".to_string());
        let mut index = EditorLineIndex::default();
        index.ensure_for(buffer.revision(), buffer.as_str());

        assert_eq!(index.line_count(), 3);
        assert_eq!(index.line_without_newline(buffer.as_str(), 0), "alpha");
        assert_eq!(index.line_without_newline(buffer.as_str(), 1), "beta");
        assert_eq!(index.line_without_newline(buffer.as_str(), 2), "");
    }

    #[test]
    fn virtual_selection_text_multiline_preserves_single_newlines() {
        let mut harness = make_app();
        harness
            .app
            .selected_content
            .reset("alpha\nbeta\ngamma".to_string());
        harness.app.virtual_selection.select_range(
            VirtualCursor { line: 0, column: 2 },
            VirtualCursor { line: 2, column: 3 },
        );

        let copied = harness.app.virtual_selection_text().expect("copied text");
        assert_eq!(copied, "pha\nbeta\ngam");
    }

    #[test]
    fn virtual_selection_text_preserves_blank_line_boundaries() {
        let mut harness = make_app();
        harness.app.selected_content.reset("a\n\nb".to_string());
        harness.app.virtual_selection.select_range(
            VirtualCursor { line: 0, column: 1 },
            VirtualCursor { line: 2, column: 0 },
        );

        let copied = harness.app.virtual_selection_text().expect("copied text");
        assert_eq!(copied, "\n\n");
    }

    #[test]
    fn staged_highlight_waits_for_idle() {
        let mut harness = make_app();
        let render = HighlightRender {
            paste_id: "alpha".to_string(),
            revision: 0,
            text_len: harness.app.selected_content.len(),
            language_hint: "py".to_string(),
            theme_key: "base16-mocha.dark".to_string(),
            lines: Vec::new(),
        };
        harness.app.highlight_staged = Some(render.clone());
        let now = Instant::now();
        harness.app.last_interaction_at = Some(now);
        harness.app.maybe_apply_staged_highlight(now);
        assert!(harness.app.highlight_render.is_none());

        let idle_now = now + HIGHLIGHT_APPLY_IDLE + Duration::from_millis(10);
        harness.app.maybe_apply_staged_highlight(idle_now);
        assert!(harness.app.highlight_render.is_some());
    }

    #[test]
    fn highlight_request_skips_when_staged_matches() {
        let mut harness = make_app();
        let render = HighlightRender {
            paste_id: "alpha".to_string(),
            revision: 0,
            text_len: harness.app.selected_content.len(),
            language_hint: "py".to_string(),
            theme_key: "base16-mocha.dark".to_string(),
            lines: Vec::new(),
        };
        harness.app.highlight_staged = Some(render);
        let should = harness.app.should_request_highlight(
            0,
            harness.app.selected_content.len(),
            "py",
            "base16-mocha.dark",
            false,
            "alpha",
        );
        assert!(!should);
    }

    #[test]
    fn word_range_at_selects_word() {
        let text = "hello world";
        let (start, end) = word_range_at(text, 1).expect("range");
        let selected: String = text.chars().skip(start).take(end - start).collect();
        assert_eq!(selected, "hello");
    }
}
