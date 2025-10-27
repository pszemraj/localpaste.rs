use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{mpsc, Arc},
    thread,
    time::{Duration, Instant},
};

use eframe::egui::{
    self, style::WidgetVisuals, CollapsingHeader, Color32, CornerRadius, FontFamily, FontId, Frame,
    Layout, Margin, RichText, Stroke, TextStyle, Visuals,
};
use egui_extras::syntax_highlighting::{self, CodeTheme};
use tokio::sync::oneshot;
use tracing::{debug, error, info, warn};

use crate::{
    config::Config,
    db::{Database, TransactionOps},
    error::AppError,
    models::folder::Folder,
    models::paste::{Paste, UpdatePasteRequest},
    naming, serve_router, AppState,
};

const COLOR_BG_PRIMARY: Color32 = Color32::from_rgb(0x0d, 0x11, 0x17);
const COLOR_BG_SECONDARY: Color32 = Color32::from_rgb(0x16, 0x1b, 0x22);
const COLOR_BG_TERTIARY: Color32 = Color32::from_rgb(0x21, 0x26, 0x2d);
const COLOR_TEXT_PRIMARY: Color32 = Color32::from_rgb(0xc9, 0xd1, 0xd9);
const COLOR_TEXT_SECONDARY: Color32 = Color32::from_rgb(0x8b, 0x94, 0x9e);
const COLOR_TEXT_MUTED: Color32 = Color32::from_rgb(0x6e, 0x76, 0x81);
const COLOR_ACCENT: Color32 = Color32::from_rgb(0xE5, 0x70, 0x00);
const COLOR_ACCENT_HOVER: Color32 = Color32::from_rgb(0xCE, 0x42, 0x2B);
const COLOR_DANGER: Color32 = Color32::from_rgb(0xF8, 0x51, 0x49);
const COLOR_BORDER: Color32 = Color32::from_rgb(0x30, 0x36, 0x3d);

const ICON_SIZE: usize = 96;

pub fn app_icon() -> egui::IconData {
    fn write_pixel(rgba: &mut [u8], x: usize, y: usize, color: [u8; 4]) {
        if x >= ICON_SIZE || y >= ICON_SIZE {
            return;
        }
        let idx = (y * ICON_SIZE + x) * 4;
        rgba[idx..idx + 4].copy_from_slice(&color);
    }

    let mut rgba = vec![0u8; ICON_SIZE * ICON_SIZE * 4];
    let bg = COLOR_BG_PRIMARY.to_array();
    for y in 0..ICON_SIZE {
        for x in 0..ICON_SIZE {
            write_pixel(&mut rgba, x, y, bg);
        }
    }

    let frame = COLOR_BORDER.to_array();
    let frame_thickness = 4;
    for x in 0..ICON_SIZE {
        for t in 0..frame_thickness {
            write_pixel(&mut rgba, x, t, frame);
            write_pixel(&mut rgba, x, ICON_SIZE - 1 - t, frame);
        }
    }
    for y in 0..ICON_SIZE {
        for t in 0..frame_thickness {
            write_pixel(&mut rgba, t, y, frame);
            write_pixel(&mut rgba, ICON_SIZE - 1 - t, y, frame);
        }
    }

    let accent = COLOR_ACCENT.to_array();
    let highlight = COLOR_TEXT_PRIMARY.to_array();
    let shadow = COLOR_ACCENT_HOVER.to_array();

    // Stylized "L"
    let l_x = ICON_SIZE / 6;
    for y in ICON_SIZE / 4..ICON_SIZE - ICON_SIZE / 6 {
        for dx in 0..3 {
            write_pixel(&mut rgba, l_x + dx, y, accent);
        }
    }
    for x in l_x..=ICON_SIZE / 2 {
        for dy in 0..3 {
            write_pixel(&mut rgba, x, ICON_SIZE - ICON_SIZE / 6 + dy, accent);
        }
    }

    // Stylized "P"
    let p_x = ICON_SIZE / 2 + ICON_SIZE / 12;
    let p_top = ICON_SIZE / 4;
    let p_bottom = ICON_SIZE - ICON_SIZE / 6;
    for y in p_top..p_bottom {
        for dx in 0..3 {
            write_pixel(&mut rgba, p_x + dx, y, accent);
        }
    }
    let loop_height = (p_bottom - p_top) / 2;
    for x in p_x..p_x + ICON_SIZE / 4 {
        for dy in 0..3 {
            write_pixel(&mut rgba, x, p_top + loop_height + dy, accent);
        }
    }
    for y in p_top..=p_top + loop_height {
        for dx in 0..3 {
            write_pixel(&mut rgba, p_x + ICON_SIZE / 4 - dx, y, accent);
        }
    }

    // Highlight seam
    for offset in 0..ICON_SIZE / 2 {
        let x = ICON_SIZE / 6 + offset;
        let y = ICON_SIZE / 6 + offset / 2;
        write_pixel(&mut rgba, x, y, highlight);
    }

    // Accent shadow
    for offset in 0..ICON_SIZE / 3 {
        let x = ICON_SIZE / 2 + offset;
        let y = ICON_SIZE - ICON_SIZE / 4 + offset / 4;
        write_pixel(&mut rgba, x, y, shadow);
    }

    egui::IconData {
        rgba,
        width: ICON_SIZE as u32,
        height: ICON_SIZE as u32,
    }
}

/// Primary egui application state.
pub struct LocalPasteApp {
    db: Arc<Database>,
    config: Arc<Config>,
    pastes: Vec<Paste>,
    folders: Vec<Folder>,
    paste_index: HashMap<String, usize>,
    folder_index: HashMap<String, usize>,
    selected_id: Option<String>,
    folder_focus: Option<String>,
    editor: EditorState,
    status: Option<StatusMessage>,
    theme: CodeTheme,
    style_applied: bool,
    folder_dialog: Option<FolderDialog>,
    _server: ServerHandle,
}

impl LocalPasteApp {
    /// Construct the GUI application using shared backend components.
    pub fn initialise() -> Result<Self, AppError> {
        let config = Config::from_env();
        let database = Database::new(&config.db_path)?;
        info!("opened database at {}", config.db_path);

        let state = AppState::new(config.clone(), database);
        let db = state.db.clone();
        let config_arc = state.config.clone();
        let allow_public = std::env::var("ALLOW_PUBLIC_ACCESS").is_ok();
        if allow_public {
            info!("public access enabled (CORS allow-all)");
        }
        let server = if std::env::var("LOCALPASTE_GUI_DISABLE_SERVER").is_ok() {
            info!("API background server disabled via LOCALPASTE_GUI_DISABLE_SERVER");
            ServerHandle::noop()
        } else {
            ServerHandle::start(state.clone(), allow_public)?
        };

        let mut app = Self {
            db,
            config: config_arc,
            pastes: Vec::new(),
            folders: Vec::new(),
            paste_index: HashMap::new(),
            folder_index: HashMap::new(),
            selected_id: None,
            folder_focus: None,
            editor: EditorState::default(),
            status: None,
            theme: CodeTheme::from_style(&egui::Style::default()),
            style_applied: false,
            folder_dialog: None,
            _server: server,
        };

        app.reload_pastes("startup");
        app.reload_folders("startup");

        Ok(app)
    }

    fn ensure_style(&mut self, ctx: &egui::Context) {
        if self.style_applied {
            return;
        }

        let mut style = (*ctx.style()).clone();
        style.visuals = Visuals::dark();
        style.visuals.override_text_color = None;
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
            TextStyle::Small,
            FontId::new(12.0, FontFamily::Proportional),
        );

        ctx.set_style(style.clone());
        self.theme = CodeTheme::from_style(&style);
        self.style_applied = true;
    }

    fn reload_pastes(&mut self, reason: &str) {
        match self.db.pastes.list(512, None) {
            Ok(mut loaded) => {
                info!("refreshed {} pastes ({})", loaded.len(), reason);
                loaded.sort_by_key(|p| std::cmp::Reverse(p.updated_at));
                self.pastes = loaded;
                self.rebuild_paste_index();

                if let Some(selected) = self
                    .selected_id
                    .clone()
                    .or_else(|| self.pastes.first().map(|p| p.id.clone()))
                {
                    self.select_paste(selected, false);
                } else if self.pastes.is_empty() {
                    self.editor = EditorState::new_unsaved(self.folder_focus.clone());
                }
            }
            Err(err) => {
                error!("failed to reload pastes: {}", err);
                self.push_status(
                    StatusLevel::Error,
                    format!("Failed to load pastes: {}", err),
                );
            }
        }
    }

    fn reload_folders(&mut self, reason: &str) {
        match self.db.folders.list() {
            Ok(mut loaded) => {
                info!("refreshed {} folders ({})", loaded.len(), reason);
                loaded.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
                self.folders = loaded;
                self.rebuild_folder_index();
            }
            Err(err) => {
                error!("failed to reload folders: {}", err);
                self.push_status(
                    StatusLevel::Error,
                    format!("Failed to load folders: {}", err),
                );
            }
        }
    }

    fn rebuild_paste_index(&mut self) {
        self.paste_index.clear();
        for (idx, paste) in self.pastes.iter().enumerate() {
            self.paste_index.insert(paste.id.clone(), idx);
        }
    }

    fn rebuild_folder_index(&mut self) {
        self.folder_index.clear();
        for (idx, folder) in self.folders.iter().enumerate() {
            self.folder_index.insert(folder.id.clone(), idx);
        }
        if let Some(focus) = self.folder_focus.clone() {
            if !self.folder_index.contains_key(&focus) {
                self.folder_focus = None;
            }
        }
    }

    fn find_paste(&self, id: &str) -> Option<&Paste> {
        self.paste_index
            .get(id)
            .and_then(|idx| self.pastes.get(*idx))
    }

    fn find_folder(&self, id: &str) -> Option<&Folder> {
        self.folder_index
            .get(id)
            .and_then(|idx| self.folders.get(*idx))
    }

    fn folder_path(&self, id: &str) -> String {
        let mut segments = Vec::new();
        let mut current = Some(id.to_string());
        let mut guard = 0;
        while let Some(curr) = current {
            if let Some(folder) = self.find_folder(&curr) {
                segments.push(folder.name.clone());
                current = folder.parent_id.clone();
            } else {
                break;
            }
            guard += 1;
            if guard > 64 {
                break;
            }
        }
        if segments.is_empty() {
            return "Unfiled".to_string();
        }
        segments.reverse();
        segments.join(" / ")
    }

    fn count_pastes_in(&self, folder_id: Option<&str>) -> usize {
        self.pastes
            .iter()
            .filter(|p| p.folder_id.as_deref() == folder_id)
            .count()
    }

    fn folder_choices(&self) -> Vec<(String, String)> {
        let mut items = self
            .folders
            .iter()
            .map(|folder| (folder.id.clone(), self.folder_path(&folder.id)))
            .collect::<Vec<_>>();
        items.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
        items
    }

    fn folder_name_exists(&self, parent: Option<&str>, name: &str) -> bool {
        self.folders.iter().any(|folder| {
            folder.parent_id.as_deref() == parent && folder.name.eq_ignore_ascii_case(name)
        })
    }

    fn try_create_folder(&mut self, dialog: &mut FolderDialog) -> bool {
        let trimmed = dialog.name.trim();
        if trimmed.is_empty() {
            dialog.error = Some("Folder name cannot be empty".to_string());
            return false;
        }
        let parent_ref = dialog.parent_id.as_deref();
        if let Some(parent_id) = parent_ref {
            if self.find_folder(parent_id).is_none() {
                dialog.error = Some("Selected parent folder no longer exists.".to_string());
                return false;
            }
        }
        if self.folder_name_exists(parent_ref, trimmed) {
            dialog.error = Some("A folder with that name already exists here.".to_string());
            return false;
        }

        let folder = Folder::with_parent(trimmed.to_string(), dialog.parent_id.clone());
        match self.db.folders.create(&folder) {
            Ok(_) => {
                if let Err(err) = self.db.flush() {
                    warn!("flush failed after folder create: {}", err);
                }
                let new_id = folder.id.clone();
                self.reload_folders("after create folder");
                self.folder_focus = Some(new_id.clone());
                self.push_status(StatusLevel::Info, format!("Created folder \"{}\"", trimmed));
                true
            }
            Err(err) => {
                dialog.error = Some(err.to_string());
                false
            }
        }
    }

    fn select_paste(&mut self, id: String, announce: bool) {
        if announce {
            debug!("selecting paste {}", id);
        }
        if let Some(paste) = self.pastes.iter().find(|p| p.id == id) {
            self.editor.apply_paste(paste.clone());
            self.selected_id = Some(paste.id.clone());
            self.folder_focus = paste.folder_id.clone();
        } else if announce {
            self.push_status(
                StatusLevel::Error,
                format!("Paste {} is no longer available", id),
            );
        }
    }

    fn render_folder_tree(&mut self, ui: &mut egui::Ui, pending_select: &mut Option<String>) {
        let unfiled_count = self.count_pastes_in(None);
        let unfiled_selected = self.folder_focus.is_none();
        let unfiled_label = if unfiled_selected {
            RichText::new(format!("Unfiled ({})", unfiled_count)).color(COLOR_ACCENT)
        } else {
            RichText::new(format!("Unfiled ({})", unfiled_count)).color(COLOR_TEXT_PRIMARY)
        };

        let unfiled = CollapsingHeader::new(unfiled_label)
            .id_salt("folder-unfiled")
            .default_open(true)
            .show(ui, |ui| {
                ui.indent("unfiled-list", |ui| {
                    self.render_paste_entries(ui, None, pending_select);
                });
            });
        if unfiled.header_response.clicked() {
            self.folder_focus = None;
        }

        self.render_folder_children(ui, None, pending_select);
    }

    fn render_folder_children(
        &mut self,
        ui: &mut egui::Ui,
        parent: Option<&str>,
        pending_select: &mut Option<String>,
    ) {
        let child_ids: Vec<String> = self
            .folders
            .iter()
            .filter(|folder| folder.parent_id.as_deref() == parent)
            .map(|folder| folder.id.clone())
            .collect();

        for folder_id in child_ids {
            if let Some(folder) = self.find_folder(folder_id.as_str()).cloned() {
                let paste_count = self.count_pastes_in(Some(folder.id.as_str()));
                let is_selected = self.folder_focus.as_deref() == Some(folder.id.as_str());
                let label = if is_selected {
                    RichText::new(format!("{} ({})", folder.name, paste_count)).color(COLOR_ACCENT)
                } else {
                    RichText::new(format!("{} ({})", folder.name, paste_count))
                        .color(COLOR_TEXT_PRIMARY)
                };
                let default_open = is_selected || folder.parent_id.is_none();
                let collapse = CollapsingHeader::new(label)
                    .id_salt(format!("folder-{}", folder.id))
                    .default_open(default_open)
                    .show(ui, |ui| {
                        ui.indent(format!("folder-indent-{}", folder.id), |ui| {
                            self.render_paste_entries(ui, Some(folder.id.as_str()), pending_select);
                            self.render_folder_children(
                                ui,
                                Some(folder.id.as_str()),
                                pending_select,
                            );
                        });
                    });
                if collapse.header_response.clicked() {
                    self.folder_focus = Some(folder.id.clone());
                }
            }
        }
    }

    fn render_paste_entries(
        &mut self,
        ui: &mut egui::Ui,
        folder_id: Option<&str>,
        pending_select: &mut Option<String>,
    ) {
        let entries: Vec<String> = self
            .pastes
            .iter()
            .filter(|paste| paste.folder_id.as_deref() == folder_id)
            .map(|paste| paste.id.clone())
            .collect();

        if entries.is_empty() {
            let message = if folder_id.is_some() {
                "Empty folder"
            } else {
                "No pastes yet"
            };
            ui.label(RichText::new(message).size(11.0).color(COLOR_TEXT_MUTED));
            return;
        }

        for paste_id in entries {
            if let Some(paste) = self.find_paste(paste_id.as_str()) {
                let selected = self
                    .selected_id
                    .as_ref()
                    .map(|id| id == &paste.id)
                    .unwrap_or(false);
                let short_id: String = paste.id.chars().take(8).collect();
                let label_text = format!("{} · {}", paste.name, short_id);
                let label = if selected {
                    RichText::new(label_text).color(COLOR_ACCENT)
                } else {
                    RichText::new(label_text).color(COLOR_TEXT_PRIMARY)
                };
                let response = ui.selectable_label(selected, label);
                if response.clicked() {
                    *pending_select = Some(paste.id.clone());
                }
            }
        }
    }

    fn handle_auto_save(&mut self, ctx: &egui::Context) {
        if !self.editor.dirty {
            return;
        }
        if self.editor.name.trim().is_empty() {
            return;
        }
        if let Some(last) = self.editor.last_modified {
            let interval = Duration::from_millis(self.config.auto_save_interval);
            let elapsed = last.elapsed();
            if elapsed >= interval {
                self.save_current_paste();
                ctx.request_repaint_after(Duration::from_millis(250));
            } else {
                ctx.request_repaint_after(interval - elapsed);
            }
        }
    }

    fn create_new_paste(&mut self) {
        let folder = self.folder_focus.clone();
        self.editor = EditorState::new_unsaved(folder.clone());
        self.folder_focus = folder;
        self.selected_id = None;
        self.push_status(StatusLevel::Info, "New paste ready".to_string());
    }

    fn save_current_paste(&mut self) {
        if self.editor.name.trim().is_empty() {
            self.push_status(StatusLevel::Error, "Name cannot be empty".into());
            return;
        }
        if !self.validate_editor_state() {
            return;
        }

        if let Some(id) = &self.editor.paste_id {
            self.update_existing_paste(id.clone());
        } else {
            self.persist_new_paste();
        }
    }

    fn validate_editor_state(&mut self) -> bool {
        let content_len = self.editor.content.len();
        if content_len > self.config.max_paste_size {
            self.push_status(
                StatusLevel::Error,
                format!(
                    "Paste is {} bytes; limit is {} bytes",
                    content_len, self.config.max_paste_size
                ),
            );
            return false;
        }

        if let Some(ref folder_id) = self.editor.folder_id {
            match self.db.folders.get(folder_id.as_str()) {
                Ok(Some(_)) => {}
                Ok(None) => {
                    self.push_status(
                        StatusLevel::Error,
                        "Selected folder no longer exists".to_string(),
                    );
                    self.folder_focus = None;
                    self.editor.folder_id = None;
                    self.reload_folders("folder missing before save");
                    return false;
                }
                Err(err) => {
                    error!("failed to verify folder {} before save: {}", folder_id, err);
                    self.push_status(
                        StatusLevel::Error,
                        "Failed to verify selected folder".to_string(),
                    );
                    return false;
                }
            }
        }

        true
    }

    fn persist_new_paste(&mut self) {
        let mut paste = Paste::new(self.editor.content.clone(), self.editor.name.clone());
        paste.language = self.editor.language.clone();
        paste.tags = self.editor.tags.clone();
        paste.folder_id = self.editor.folder_id.clone();

        let result = if let Some(ref folder_id) = paste.folder_id {
            TransactionOps::create_paste_with_folder(&self.db, &paste, folder_id)
        } else {
            self.db.pastes.create(&paste)
        };

        match result {
            Ok(_) => {
                info!("created paste {} ({} chars)", paste.id, paste.content.len());
                if let Err(err) = self.db.flush() {
                    warn!("flush failed after create: {}", err);
                }
                self.push_status(StatusLevel::Info, format!("Created {}", paste.name));
                self.editor.apply_paste(paste.clone());
                self.selected_id = Some(paste.id.clone());
                self.folder_focus = paste.folder_id.clone();
                self.reload_pastes("after create");
                self.reload_folders("after create");
            }
            Err(err) => {
                error!("failed to create paste: {}", err);
                self.push_status(StatusLevel::Error, format!("Save failed: {}", err));
            }
        }
    }

    fn update_existing_paste(&mut self, id: String) {
        let previous = match self.db.pastes.get(&id) {
            Ok(Some(paste)) => paste,
            Ok(None) => {
                self.push_status(StatusLevel::Error, "Paste disappeared before saving".into());
                self.reload_pastes("missing on update");
                return;
            }
            Err(err) => {
                error!("failed to read paste {} before update: {}", id, err);
                self.push_status(StatusLevel::Error, format!("Save failed: {}", err));
                return;
            }
        };

        let folder_value = self.editor.folder_id.clone().unwrap_or_default();
        let update = UpdatePasteRequest {
            content: Some(self.editor.content.clone()),
            name: Some(self.editor.name.clone()),
            language: self.editor.language.clone(),
            folder_id: Some(folder_value.clone()),
            tags: Some(self.editor.tags.clone()),
        };

        let result = if previous.folder_id.as_deref() != self.editor.folder_id.as_deref() {
            let new_folder = if folder_value.is_empty() {
                None
            } else {
                Some(folder_value.as_str())
            };
            TransactionOps::move_paste_between_folders(
                &self.db,
                &id,
                previous.folder_id.as_deref(),
                new_folder,
                update.clone(),
            )
        } else {
            self.db.pastes.update(&id, update.clone())
        };

        match result {
            Ok(Some(updated)) => {
                info!("updated paste {} ({} chars)", id, self.editor.content.len());
                if let Err(err) = self.db.flush() {
                    warn!("flush failed after update: {}", err);
                }
                self.editor.apply_paste(updated.clone());
                self.selected_id = Some(updated.id.clone());
                self.reload_pastes("after update");
                self.reload_folders("after update");
                self.folder_focus = updated.folder_id.clone();
                self.push_status(StatusLevel::Info, "Saved changes".into());
            }
            Ok(None) => {
                warn!("paste {} vanished during update", id);
                self.push_status(StatusLevel::Error, "Paste disappeared before saving".into());
                self.reload_pastes("missing on update");
                self.reload_folders("missing on update");
            }
            Err(err) => {
                error!("failed to update paste {}: {}", id, err);
                self.push_status(StatusLevel::Error, format!("Save failed: {}", err));
            }
        }
    }

    fn delete_selected(&mut self) {
        if let Some(id) = self.selected_id.clone() {
            debug!("deleting paste {}", id);
            match self.db.pastes.delete(&id) {
                Ok(true) => {
                    if let Err(err) = self.db.flush() {
                        warn!("flush failed after delete: {}", err);
                    }
                    self.push_status(StatusLevel::Info, "Deleted paste".into());
                    self.selected_id = None;
                    self.editor = EditorState::new_unsaved(self.folder_focus.clone());
                    self.reload_pastes("after delete");
                    self.reload_folders("after delete");
                }
                Ok(false) => {
                    self.push_status(StatusLevel::Error, "Paste was already deleted".into());
                    self.reload_pastes("stale delete");
                    self.reload_folders("stale delete");
                }
                Err(err) => {
                    error!("failed to delete paste {}: {}", id, err);
                    self.push_status(StatusLevel::Error, format!("Delete failed: {}", err));
                }
            }
        }
    }

    fn push_status(&mut self, level: StatusLevel, message: String) {
        self.status = Some(StatusMessage {
            text: message.clone(),
            level,
            expires_at: Instant::now() + Duration::from_secs(4),
        });
        debug!("status: {}", message);
    }

    fn status_color(level: StatusLevel) -> Color32 {
        match level {
            StatusLevel::Info => COLOR_ACCENT,
            StatusLevel::Error => COLOR_DANGER,
        }
    }
}

struct ServerHandle {
    shutdown: Option<oneshot::Sender<()>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl ServerHandle {
    fn noop() -> Self {
        Self {
            shutdown: None,
            thread: None,
        }
    }

    fn start(state: AppState, allow_public: bool) -> Result<Self, AppError> {
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("localpaste-server".into())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(err) => {
                        let _ = ready_tx.send(Err(format!("failed to start runtime: {}", err)));
                        return;
                    }
                };

                let bind_addr = resolve_bind_address(&state.config);
                let listener = match rt.block_on(tokio::net::TcpListener::bind(bind_addr)) {
                    Ok(listener) => {
                        let _ = ready_tx.send(Ok(bind_addr));
                        listener
                    }
                    Err(err) => {
                        let _ =
                            ready_tx.send(Err(format!("failed to bind server socket: {}", err)));
                        return;
                    }
                };

                let shutdown = async {
                    let _ = shutdown_rx.await;
                };

                if let Err(err) = rt.block_on(serve_router(
                    listener,
                    state.clone(),
                    allow_public,
                    shutdown,
                )) {
                    error!("server error: {}", err);
                }

                if let Err(err) = state.db.flush() {
                    error!("failed to flush database: {}", err);
                }
            })
            .map_err(|err| AppError::DatabaseError(format!("failed to spawn server: {}", err)))?;

        match ready_rx.recv() {
            Ok(Ok(addr)) => {
                if !addr.ip().is_loopback() {
                    warn!("binding to non-localhost address {}", addr);
                }
                info!("API listening on http://{}", addr);
                Ok(Self {
                    shutdown: Some(shutdown_tx),
                    thread: Some(thread),
                })
            }
            Ok(Err(message)) => {
                let _ = shutdown_tx.send(());
                let _ = thread.join();
                Err(AppError::DatabaseError(message))
            }
            Err(_) => {
                let _ = shutdown_tx.send(());
                let _ = thread.join();
                Err(AppError::Internal)
            }
        }
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

fn resolve_bind_address(config: &Config) -> SocketAddr {
    std::env::var("BIND")
        .ok()
        .and_then(|s| s.parse::<SocketAddr>().ok())
        .unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], config.port)))
}

impl eframe::App for LocalPasteApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ensure_style(ctx);

        ctx.input(|input| {
            if input.modifiers.command && input.key_pressed(egui::Key::S) {
                self.save_current_paste();
            }
            if input.modifiers.command && input.key_pressed(egui::Key::N) {
                self.create_new_paste();
            }
            if input.modifiers.command && input.key_pressed(egui::Key::Delete) {
                self.delete_selected();
            }
        });

        if let Some(status) = &self.status {
            if Instant::now() >= status.expires_at {
                self.status = None;
                ctx.request_repaint();
            } else {
                ctx.request_repaint_after(Duration::from_millis(250));
            }
        }

        let mut pending_select: Option<String> = None;
        egui::SidePanel::left("sidebar")
            .default_width(280.0)
            .resizable(true)
            .frame(Frame {
                fill: COLOR_BG_SECONDARY,
                stroke: Stroke::new(1.0, COLOR_BORDER),
                inner_margin: Margin::symmetric(16, 16),
                ..Default::default()
            })
            .show(ctx, |ui| {
                ui.vertical(|ui| {
                    ui.heading(RichText::new("LocalPaste.rs").color(COLOR_ACCENT));
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(self.config.db_path.as_str())
                            .monospace()
                            .size(12.0)
                            .color(COLOR_TEXT_MUTED),
                    );

                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        let paste_btn =
                            egui::Button::new(RichText::new("+ New Paste").color(Color32::WHITE))
                                .fill(COLOR_ACCENT)
                                .min_size(egui::vec2(ui.available_width() * 0.5, 36.0));
                        if ui.add(paste_btn).clicked() {
                            self.create_new_paste();
                        }
                        let folder_btn =
                            egui::Button::new(RichText::new("+ New Folder").color(Color32::WHITE))
                                .fill(COLOR_ACCENT_HOVER)
                                .min_size(egui::vec2(ui.available_width(), 36.0));
                        if ui.add(folder_btn).clicked() {
                            self.folder_dialog = Some(FolderDialog::new(self.folder_focus.clone()));
                        }
                    });

                    if let Some(focus) = self.folder_focus.clone() {
                        if let Some(path) = self
                            .find_folder(focus.as_str())
                            .map(|_| self.folder_path(&focus))
                        {
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new(format!("Selected folder: {}", path))
                                    .size(12.0)
                                    .color(COLOR_TEXT_MUTED),
                            );
                        }
                    }

                    ui.add_space(12.0);
                    ui.add(egui::Separator::default());
                    ui.add_space(6.0);
                    ui.label(RichText::new("BROWSER").size(11.0).color(COLOR_TEXT_MUTED));
                    ui.add_space(4.0);

                    egui::ScrollArea::vertical()
                        .auto_shrink([false; 2])
                        .show(ui, |ui| {
                            self.render_folder_tree(ui, &mut pending_select);
                        });
                });
            });
        if let Some(id) = pending_select {
            self.select_paste(id, true);
        }

        egui::TopBottomPanel::bottom("status_bar")
            .frame(Frame {
                fill: COLOR_BG_SECONDARY,
                stroke: Stroke::new(1.0, COLOR_BORDER),
                inner_margin: Margin::symmetric(16, 10),
                ..Default::default()
            })
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if let Some(status) = &self.status {
                        ui.label(
                            RichText::new(&status.text).color(Self::status_color(status.level)),
                        );
                    } else if self.editor.dirty {
                        ui.label(RichText::new("Unsaved changes").color(COLOR_ACCENT));
                    } else {
                        ui.label(RichText::new("Ready").color(COLOR_TEXT_MUTED));
                    }

                    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{} chars", self.editor.content.len()))
                                .color(COLOR_TEXT_MUTED),
                        );
                        let language_label = self
                            .editor
                            .language
                            .as_deref()
                            .and_then(LanguageSet::label)
                            .unwrap_or("Auto");
                        ui.label(RichText::new(language_label).color(COLOR_TEXT_MUTED));
                    });
                });
            });

        egui::CentralPanel::default()
            .frame(Frame {
                fill: COLOR_BG_PRIMARY,
                stroke: Stroke::NONE,
                inner_margin: Margin::same(0),
                ..Default::default()
            })
            .show(ctx, |ui| {
                ui.add_space(8.0);
                Frame {
                    fill: COLOR_BG_SECONDARY,
                    stroke: Stroke::new(1.0, COLOR_BORDER),
                    inner_margin: Margin::symmetric(16, 12),
                    corner_radius: CornerRadius::same(8),
                    ..Default::default()
                }
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new("Paste Name")
                                    .size(12.0)
                                    .color(COLOR_TEXT_MUTED),
                            );
                            let response = ui.add(
                                egui::TextEdit::singleline(&mut self.editor.name)
                                    .desired_width(240.0)
                                    .background_color(COLOR_BG_TERTIARY),
                            );
                            if response.changed() {
                                self.editor.mark_dirty();
                            }
                        });

                        ui.add_space(20.0);
                        ui.vertical(|ui| {
                            ui.label(RichText::new("Language").size(12.0).color(COLOR_TEXT_MUTED));
                            let current_language_label = self
                                .editor
                                .language
                                .as_deref()
                                .and_then(LanguageSet::label)
                                .unwrap_or("Auto");
                            egui::ComboBox::from_id_salt("language_select")
                                .selected_text(current_language_label)
                                .show_ui(ui, |ui| {
                                    ui.set_min_width(160.0);
                                    if ui
                                        .selectable_value(&mut self.editor.language, None, "Auto")
                                        .clicked()
                                    {
                                        self.editor.mark_dirty();
                                    }
                                    ui.separator();
                                    for option in LanguageSet::options() {
                                        if ui
                                            .selectable_value(
                                                &mut self.editor.language,
                                                Some(option.id.to_string()),
                                                option.label,
                                            )
                                            .clicked()
                                        {
                                            self.editor.mark_dirty();
                                        }
                                    }
                                });
                        });

                        ui.add_space(20.0);
                        ui.vertical(|ui| {
                            ui.label(RichText::new("Folder").size(12.0).color(COLOR_TEXT_MUTED));
                            let current_label = self
                                .editor
                                .folder_id
                                .as_deref()
                                .and_then(|id| self.find_folder(id))
                                .map(|_| {
                                    self.editor
                                        .folder_id
                                        .as_deref()
                                        .map(|id| self.folder_path(id))
                                        .unwrap_or_else(|| "Unfiled".to_string())
                                })
                                .unwrap_or_else(|| "Unfiled".to_string());
                            egui::ComboBox::from_id_salt("folder_select")
                                .selected_text(current_label)
                                .show_ui(ui, |ui| {
                                    ui.set_min_width(180.0);
                                    if ui
                                        .selectable_value(
                                            &mut self.editor.folder_id,
                                            None,
                                            "Unfiled",
                                        )
                                        .clicked()
                                    {
                                        self.folder_focus = None;
                                        self.editor.mark_dirty();
                                    }
                                    let choices = self.folder_choices();
                                    if !choices.is_empty() {
                                        ui.separator();
                                    }
                                    for (id, label) in choices {
                                        if ui
                                            .selectable_value(
                                                &mut self.editor.folder_id,
                                                Some(id.clone()),
                                                label,
                                            )
                                            .clicked()
                                        {
                                            self.folder_focus = Some(id);
                                            self.editor.mark_dirty();
                                        }
                                    }
                                });
                        });

                        ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                            if self.editor.paste_id.is_some() {
                                let delete_btn = egui::Button::new(
                                    RichText::new("Delete").color(Color32::WHITE),
                                )
                                .fill(COLOR_DANGER)
                                .min_size(egui::vec2(110.0, 36.0));
                                if ui.add(delete_btn).clicked() {
                                    self.delete_selected();
                                }
                            }

                            let save_btn =
                                egui::Button::new(RichText::new("Save").color(Color32::WHITE))
                                    .fill(COLOR_ACCENT)
                                    .min_size(egui::vec2(110.0, 36.0));
                            if ui.add(save_btn).clicked() {
                                self.save_current_paste();
                            }
                        });
                    });
                });

                ui.add_space(12.0);
                Frame {
                    fill: COLOR_BG_TERTIARY,
                    stroke: Stroke::new(1.0, COLOR_BORDER),
                    inner_margin: Margin::symmetric(16, 16),
                    corner_radius: CornerRadius::same(8),
                    ..Default::default()
                }
                .show(ui, |ui| {
                    let text_style = TextStyle::Monospace;
                    egui::ScrollArea::vertical()
                        .id_salt("editor_scroll")
                        .auto_shrink([false; 2])
                        .show(ui, |ui| {
                            let highlight_language = self
                                .editor
                                .language
                                .clone()
                                .or_else(|| {
                                    crate::models::paste::detect_language(&self.editor.content)
                                })
                                .unwrap_or_else(|| "plain".to_string());
                            let highlight_token =
                                LanguageSet::highlight_token(highlight_language.as_str());
                            let theme = self.theme.clone();
                            let mut layouter =
                                move |ui: &egui::Ui,
                                      text: &dyn egui::TextBuffer,
                                      wrap_width: f32| {
                                    let syntax_id = highlight_token
                                        .unwrap_or_else(|| highlight_language.as_str());
                                    let mut job = syntax_highlighting::highlight(
                                        ui.ctx(),
                                        ui.style(),
                                        &theme,
                                        text.as_str(),
                                        syntax_id,
                                    );
                                    job.wrap.max_width = wrap_width;
                                    ui.fonts_mut(|f| f.layout_job(job))
                                };

                            let editor = egui::TextEdit::multiline(&mut self.editor.content)
                                .font(text_style)
                                .desired_width(f32::INFINITY)
                                .desired_rows(32)
                                .frame(false)
                                .layouter(&mut layouter);

                            let response = ui.add(editor);
                            if self.editor.needs_focus {
                                if !response.has_focus() {
                                    response.request_focus();
                                }
                                self.editor.needs_focus = false;
                            }
                            if response.changed() {
                                #[cfg(debug_assertions)]
                                {
                                    debug!("editor changed ({} chars)", self.editor.content.len());
                                }
                                self.editor.mark_dirty();
                            }
                        });
                });
            });
        if let Some(mut dialog) = self.folder_dialog.take() {
            let mut open = true;
            let mut keep_dialog = true;
            egui::Window::new("Create Folder")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label(
                        RichText::new("Choose a name and parent for the folder.")
                            .color(COLOR_TEXT_MUTED),
                    );
                    ui.add_space(8.0);
                    let name_response = ui.add(
                        egui::TextEdit::singleline(&mut dialog.name)
                            .desired_width(260.0)
                            .hint_text("Folder name"),
                    );
                    if name_response.changed() {
                        dialog.error = None;
                    }
                    ui.add_space(10.0);
                    ui.label(RichText::new("Parent").size(12.0).color(COLOR_TEXT_MUTED));
                    let parent_label = dialog
                        .parent_id
                        .as_deref()
                        .and_then(|id| self.find_folder(id).map(|_| self.folder_path(id)))
                        .unwrap_or_else(|| "Unfiled".to_string());
                    egui::ComboBox::from_id_salt("folder_dialog_parent")
                        .selected_text(parent_label)
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_value(&mut dialog.parent_id, None, "Unfiled")
                                .clicked()
                            {
                                dialog.error = None;
                            }
                            let choices = self.folder_choices();
                            for (id, label) in choices {
                                if ui
                                    .selectable_value(
                                        &mut dialog.parent_id,
                                        Some(id.clone()),
                                        label,
                                    )
                                    .clicked()
                                {
                                    dialog.error = None;
                                }
                            }
                        });
                    if let Some(error) = &dialog.error {
                        ui.add_space(8.0);
                        ui.label(RichText::new(error).color(COLOR_DANGER));
                    }
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            keep_dialog = false;
                        }
                        let create_btn = ui.add_enabled(
                            !dialog.name.trim().is_empty(),
                            egui::Button::new("Create"),
                        );
                        if create_btn.clicked() && self.try_create_folder(&mut dialog) {
                            keep_dialog = false;
                        }
                    });
                });
            if open && keep_dialog {
                self.folder_dialog = Some(dialog);
            }
        }
        self.handle_auto_save(ctx);
    }
}

struct FolderDialog {
    name: String,
    parent_id: Option<String>,
    error: Option<String>,
}

impl FolderDialog {
    fn new(parent_id: Option<String>) -> Self {
        Self {
            name: String::new(),
            parent_id,
            error: None,
        }
    }
}

struct EditorState {
    paste_id: Option<String>,
    name: String,
    content: String,
    language: Option<String>,
    folder_id: Option<String>,
    tags: Vec<String>,
    dirty: bool,
    last_modified: Option<Instant>,
    needs_focus: bool,
}

impl EditorState {
    fn new_unsaved(folder_id: Option<String>) -> Self {
        Self {
            name: naming::generate_name(),
            folder_id,
            needs_focus: true,
            ..Default::default()
        }
    }

    fn apply_paste(&mut self, paste: Paste) {
        self.paste_id = Some(paste.id);
        self.name = paste.name;
        self.content = paste.content;
        self.language = paste.language;
        self.folder_id = paste.folder_id;
        self.tags = paste.tags;
        self.mark_pristine();
        self.needs_focus = true;
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
        self.last_modified = Some(Instant::now());
    }

    fn mark_pristine(&mut self) {
        self.dirty = false;
        self.last_modified = None;
    }
}

impl Default for EditorState {
    fn default() -> Self {
        Self {
            paste_id: None,
            name: "untitled".to_string(),
            content: String::new(),
            language: None,
            folder_id: None,
            tags: Vec::new(),
            dirty: false,
            last_modified: None,
            needs_focus: false,
        }
    }
}

struct StatusMessage {
    text: String,
    level: StatusLevel,
    expires_at: Instant,
}

#[derive(Clone, Copy)]
enum StatusLevel {
    Info,
    Error,
}

#[derive(Clone, Copy)]
struct LanguageOption {
    id: &'static str,
    label: &'static str,
    highlight: Option<&'static str>,
}

struct LanguageSet;

impl LanguageSet {
    fn options() -> &'static [LanguageOption] {
        const OPTIONS: &[LanguageOption] = &[
            LanguageOption {
                id: "plain",
                label: "Plain Text",
                highlight: None,
            },
            LanguageOption {
                id: "c",
                label: "C",
                highlight: Some("c"),
            },
            LanguageOption {
                id: "cpp",
                label: "C++",
                highlight: Some("cpp"),
            },
            LanguageOption {
                id: "csharp",
                label: "C#",
                highlight: Some("cs"),
            },
            LanguageOption {
                id: "css",
                label: "CSS",
                highlight: Some("css"),
            },
            LanguageOption {
                id: "go",
                label: "Go",
                highlight: Some("go"),
            },
            LanguageOption {
                id: "html",
                label: "HTML",
                highlight: Some("html"),
            },
            LanguageOption {
                id: "java",
                label: "Java",
                highlight: Some("java"),
            },
            LanguageOption {
                id: "javascript",
                label: "JavaScript",
                highlight: Some("js"),
            },
            LanguageOption {
                id: "json",
                label: "JSON",
                highlight: Some("json"),
            },
            LanguageOption {
                id: "latex",
                label: "LaTeX",
                highlight: Some("tex"),
            },
            LanguageOption {
                id: "markdown",
                label: "Markdown",
                highlight: Some("md"),
            },
            LanguageOption {
                id: "python",
                label: "Python",
                highlight: Some("py"),
            },
            LanguageOption {
                id: "rust",
                label: "Rust",
                highlight: Some("rs"),
            },
            LanguageOption {
                id: "shell",
                label: "Shell / Bash",
                highlight: Some("sh"),
            },
            LanguageOption {
                id: "sql",
                label: "SQL",
                highlight: Some("sql"),
            },
            LanguageOption {
                id: "toml",
                label: "TOML",
                highlight: Some("toml"),
            },
            LanguageOption {
                id: "typescript",
                label: "TypeScript",
                highlight: Some("ts"),
            },
            LanguageOption {
                id: "xml",
                label: "XML",
                highlight: Some("xml"),
            },
            LanguageOption {
                id: "yaml",
                label: "YAML",
                highlight: Some("yml"),
            },
        ];
        OPTIONS
    }

    fn label(id: &str) -> Option<&'static str> {
        Self::options()
            .iter()
            .find_map(|opt| if opt.id == id { Some(opt.label) } else { None })
    }

    fn highlight_token(id: &str) -> Option<&'static str> {
        Self::options()
            .iter()
            .find(|opt| opt.id == id)
            .and_then(|opt| opt.highlight)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn init_app(max_size: usize) -> (LocalPasteApp, TempDir) {
        let temp = TempDir::new().expect("temp dir");
        let db_path = temp.path().join("db");

        std::env::set_var("DB_PATH", db_path.to_string_lossy().to_string());
        std::env::set_var("MAX_PASTE_SIZE", max_size.to_string());
        std::env::set_var("BIND", "127.0.0.1:0");
        std::env::set_var("LOCALPASTE_GUI_DISABLE_SERVER", "1");

        let app = LocalPasteApp::initialise().expect("app init");

        std::env::remove_var("BIND");
        std::env::remove_var("MAX_PASTE_SIZE");
        std::env::remove_var("DB_PATH");
        std::env::remove_var("LOCALPASTE_GUI_DISABLE_SERVER");

        (app, temp)
    }

    #[test]
    fn validate_editor_blocks_oversize_content() {
        let (mut app, _guard) = init_app(16);
        assert_eq!(app.config.max_paste_size, 16);
        app.editor.name = "large".to_string();
        app.editor.content = "x".repeat(32);

        assert!(
            !app.validate_editor_state(),
            "oversize paste should be rejected"
        );
    }

    #[test]
    fn validate_editor_rejects_missing_folder() {
        let (mut app, _guard) = init_app(1024);
        app.editor.name = "orphan".to_string();
        app.editor.content = "ok".to_string();
        app.editor.folder_id = Some("missing-folder".to_string());
        app.folder_focus = app.editor.folder_id.clone();

        assert!(
            !app.validate_editor_state(),
            "missing folder should cause validation failure"
        );
        assert!(
            app.editor.folder_id.is_none(),
            "editor folder_id should be cleared when folder is missing"
        );
        assert!(
            app.folder_focus.is_none(),
            "folder_focus should reset when validation clears folder"
        );
    }
}
