use std::{
    net::SocketAddr,
    sync::{mpsc, Arc},
    thread,
    time::{Duration, Instant},
};

use eframe::egui::{
    self, style::WidgetVisuals, Color32, CornerRadius, FontFamily, FontId, Frame, Layout, Margin,
    RichText, Stroke, TextStyle, Visuals,
};
use egui_extras::syntax_highlighting::{self, CodeTheme};

use tokio::sync::oneshot;

use crate::{
    config::Config,
    db::Database,
    error::AppError,
    models::paste::{Paste, UpdatePasteRequest},
    serve_router,
    AppState,
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
    selected_id: Option<String>,
    editor: EditorState,
    status: Option<StatusMessage>,
    theme: CodeTheme,
    style_applied: bool,
    _server: ServerHandle,
}

impl LocalPasteApp {
    /// Construct the GUI application using shared backend components.
    pub fn initialise() -> Result<Self, AppError> {
        let config = Config::from_env();
        let database = Database::new(&config.db_path)?;
        println!(
            "[localpaste-gui] opened database at {}",
            config.db_path
        );

        let state = AppState::new(config.clone(), database);
        let db = state.db.clone();
        let config_arc = state.config.clone();
        let allow_public = std::env::var("ALLOW_PUBLIC_ACCESS").is_ok();
        if allow_public {
            println!("[localpaste-gui] public access enabled (CORS allow-all)");
        }
        let server = ServerHandle::start(state.clone(), allow_public)?;

        let mut app = Self {
            db,
            config: config_arc,
            pastes: Vec::new(),
            selected_id: None,
            editor: EditorState::default(),
            status: None,
            theme: CodeTheme::from_style(&egui::Style::default()),
            style_applied: false,
            _server: server,
        };

        app.reload_pastes("startup");

        Ok(app)
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
        style.text_styles.insert(
            TextStyle::Body,
            FontId::new(16.0, FontFamily::Proportional),
        );
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
                println!(
                    "[localpaste-gui] refreshed {} pastes ({})",
                    loaded.len(),
                    reason
                );
                loaded.sort_by_key(|p| std::cmp::Reverse(p.updated_at));
                self.pastes = loaded;

                if let Some(selected) = self
                    .selected_id
                    .clone()
                    .or_else(|| self.pastes.first().map(|p| p.id.clone()))
                {
                    self.select_paste(selected, false);
                } else if self.pastes.is_empty() {
                    self.editor = EditorState::new_unsaved();
                }
            }
            Err(err) => {
                println!(
                    "[localpaste-gui] failed to reload pastes: {}",
                    err
                );
                self.push_status(
                    StatusLevel::Error,
                    format!("Failed to load pastes: {}", err),
                );
            }
        }
    }

    fn select_paste(&mut self, id: String, announce: bool) {
        if announce {
            println!("[localpaste-gui] selecting paste {}", id);
        }
        if let Some(paste) = self.pastes.iter().find(|p| p.id == id) {
            self.editor.apply_paste(paste.clone());
            self.selected_id = Some(paste.id.clone());
        } else if announce {
            self.push_status(
                StatusLevel::Error,
                format!("Paste {} is no longer available", id),
            );
        }
    }

    fn create_new_paste(&mut self) {
        self.editor = EditorState::new_unsaved();
        self.selected_id = None;
        println!("[localpaste-gui] ready for new unsaved paste");
        self.push_status(StatusLevel::Info, "New paste ready".to_string());
    }

    fn save_current_paste(&mut self) {
        if self.editor.name.trim().is_empty() {
            self.push_status(StatusLevel::Error, "Name cannot be empty".into());
            return;
        }

        if let Some(id) = &self.editor.paste_id {
            self.update_existing_paste(id.clone());
        } else {
            self.persist_new_paste();
        }
    }

    fn persist_new_paste(&mut self) {
        let mut paste = Paste::new(self.editor.content.clone(), self.editor.name.clone());
        paste.language = self.editor.language.clone();
        paste.tags = self.editor.tags.clone();

        match self.db.pastes.create(&paste) {
            Ok(_) => {
                println!(
                    "[localpaste-gui] created paste {} ({} chars)",
                    paste.id,
                    paste.content.len()
                );
                if let Err(err) = self.db.flush() {
                    println!(
                        "[localpaste-gui] warning: flush failed after create: {}",
                        err
                    );
                }
                self.push_status(
                    StatusLevel::Info,
                    format!("Created {}", paste.name),
                );
                self.editor.apply_paste(paste.clone());
                self.selected_id = Some(paste.id.clone());
                self.reload_pastes("after create");
            }
            Err(err) => {
                println!(
                    "[localpaste-gui] failed to create paste: {}",
                    err
                );
                self.push_status(
                    StatusLevel::Error,
                    format!("Save failed: {}", err),
                );
            }
        }
    }

    fn update_existing_paste(&mut self, id: String) {
        let update = UpdatePasteRequest {
            content: Some(self.editor.content.clone()),
            name: Some(self.editor.name.clone()),
            language: self.editor.language.clone(),
            folder_id: None,
            tags: Some(self.editor.tags.clone()),
        };

        match self.db.pastes.update(&id, update) {
            Ok(Some(updated)) => {
                println!(
                    "[localpaste-gui] updated paste {} ({} chars)",
                    id,
                    self.editor.content.len()
                );
                if let Err(err) = self.db.flush() {
                    println!(
                        "[localpaste-gui] warning: flush failed after update: {}",
                        err
                    );
                }
                self.editor.apply_paste(updated.clone());
                self.selected_id = Some(updated.id.clone());
                self.reload_pastes("after update");
                self.push_status(StatusLevel::Info, "Saved changes".into());
            }
            Ok(None) => {
                println!(
                    "[localpaste-gui] paste {} vanished during update",
                    id
                );
                self.push_status(
                    StatusLevel::Error,
                    "Paste disappeared before saving".into(),
                );
                self.reload_pastes("missing on update");
            }
            Err(err) => {
                println!(
                    "[localpaste-gui] failed to update paste {}: {}",
                    id, err
                );
                self.push_status(
                    StatusLevel::Error,
                    format!("Save failed: {}", err),
                );
            }
        }
    }

    fn delete_selected(&mut self) {
        if let Some(id) = self.selected_id.clone() {
            println!("[localpaste-gui] deleting paste {}", id);
            match self.db.pastes.delete(&id) {
                Ok(true) => {
                    if let Err(err) = self.db.flush() {
                        println!(
                            "[localpaste-gui] warning: flush failed after delete: {}",
                            err
                        );
                    }
                    self.push_status(StatusLevel::Info, "Deleted paste".into());
                    self.selected_id = None;
                    self.create_new_paste();
                    self.reload_pastes("after delete");
                }
                Ok(false) => {
                    self.push_status(
                        StatusLevel::Error,
                        "Paste was already deleted".into(),
                    );
                    self.reload_pastes("stale delete");
                }
                Err(err) => {
                    println!(
                        "[localpaste-gui] failed to delete paste {}: {}",
                        id, err
                    );
                    self.push_status(
                        StatusLevel::Error,
                        format!("Delete failed: {}", err),
                    );
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
        println!("[localpaste-gui] status: {}", message);
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
                    eprintln!("[localpaste-gui] server error: {}", err);
                }

                if let Err(err) = state.db.flush() {
                    eprintln!("[localpaste-gui] failed to flush database: {}", err);
                }
            })
            .map_err(|err| AppError::DatabaseError(format!("failed to spawn server: {}", err)))?;

        match ready_rx.recv() {
            Ok(Ok(addr)) => {
                if !addr.ip().is_loopback() {
                    println!(
                        "[localpaste-gui] warning: binding to non-localhost address {}",
                        addr
                    );
                }
                println!("[localpaste-gui] API listening on http://{}", addr);
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

                    ui.add_space(16.0);
                    let new_btn = egui::Button::new(
                        RichText::new("+ New Paste").color(Color32::WHITE),
                    )
                    .fill(COLOR_ACCENT)
                    .min_size(egui::vec2(ui.available_width(), 38.0));
                    if ui.add(new_btn).clicked() {
                        self.create_new_paste();
                    }

                    ui.add_space(12.0);
                    ui.add(egui::Separator::default());

                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("RECENT PASTES")
                            .size(11.0)
                            .color(COLOR_TEXT_MUTED),
                    );

                    ui.add_space(4.0);
                    egui::ScrollArea::vertical()
                        .auto_shrink([false; 2])
                        .show(ui, |ui| {
                            for paste in &self.pastes {
                                let selected = self
                                    .selected_id
                                    .as_ref()
                                    .map(|id| id == &paste.id)
                                    .unwrap_or(false);
                                let short_id: String =
                                    paste.id.chars().take(8).collect();
                                let label_text =
                                    format!("{} - {}", paste.name, short_id);
                                let label = if selected {
                                    RichText::new(label_text).color(COLOR_ACCENT)
                                } else {
                                    RichText::new(label_text)
                                        .color(COLOR_TEXT_PRIMARY)
                                };
                                let response =
                                    ui.selectable_label(selected, label);
                                if response.clicked() {
                                    pending_select = Some(paste.id.clone());
                                }
                            }
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
                            RichText::new(&status.text)
                                .color(Self::status_color(status.level)),
                        );
                    } else if self.editor.dirty {
                        ui.label(
                            RichText::new("Unsaved changes").color(COLOR_ACCENT),
                        );
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
                            .clone()
                            .unwrap_or_else(|| "auto".to_string());
                        ui.label(
                            RichText::new(language_label).color(COLOR_TEXT_MUTED),
                        );
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
                                self.editor.dirty = true;
                            }
                        });

                        ui.add_space(20.0);
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new("Language")
                                    .size(12.0)
                                    .color(COLOR_TEXT_MUTED),
                            );
                            egui::ComboBox::from_id_salt("language_select")
                                .selected_text(
                                    self.editor
                                        .language
                                        .clone()
                                        .unwrap_or_else(|| "auto".into()),
                                )
                                .show_ui(ui, |ui| {
                                    ui.set_min_width(160.0);
                                    if ui
                                        .selectable_value(
                                            &mut self.editor.language,
                                            None,
                                            "auto",
                                        )
                                        .clicked()
                                    {
                                        self.editor.dirty = true;
                                    }
                                    ui.separator();
                                    for option in LanguageSet::all().iter() {
                                        if ui
                                            .selectable_value(
                                                &mut self.editor.language,
                                                Some(option.to_string()),
                                                *option,
                                            )
                                            .clicked()
                                        {
                                            self.editor.dirty = true;
                                        }
                                    }
                                });
                        });

                        ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                            if self.editor.paste_id.is_some() {
                                let delete_btn = egui::Button::new(
                                    RichText::new("Delete")
                                        .color(Color32::WHITE),
                                )
                                .fill(COLOR_DANGER)
                                .min_size(egui::vec2(110.0, 36.0));
                                if ui.add(delete_btn).clicked() {
                                    self.delete_selected();
                                }
                            }

                            let save_btn = egui::Button::new(
                                RichText::new("Save").color(Color32::WHITE),
                            )
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
                    egui::ScrollArea::vertical()
                        .id_salt("editor_scroll")
                        .auto_shrink([false; 2])
                        .show(ui, |ui| {
                            let language = self
                                .editor
                                .language
                                .clone()
                                .unwrap_or_else(|| "plain".into());
                            let theme = self.theme.clone();
                            let mut layouter =
                                move |ui: &egui::Ui,
                                      text: &dyn egui::TextBuffer,
                                      wrap_width: f32| {
                                    let source = text.as_str();
                                    let mut job = syntax_highlighting::highlight(
                                        ui.ctx(),
                                        ui.style(),
                                        &theme,
                                        &language,
                                        source,
                                    );
                                    job.wrap.max_width = wrap_width;
                                    ui.fonts_mut(|f| f.layout_job(job))
                                };
                            let editor =
                                egui::TextEdit::multiline(&mut self.editor.content)
                                    .code_editor()
                                    .lock_focus(true)
                                    .frame(false)
                                    .background_color(COLOR_BG_PRIMARY)
                                    .desired_width(f32::INFINITY)
                                    .desired_rows(32)
                                    .layouter(&mut layouter);

                            let response = ui.add(editor);
                            if response.changed() {
                                self.editor.dirty = true;
                            }
                        });
                });
            });
    }
}

#[derive(Default)]
struct EditorState {
    paste_id: Option<String>,
    name: String,
    content: String,
    language: Option<String>,
    tags: Vec<String>,
    dirty: bool,
}

impl EditorState {
    fn new_unsaved() -> Self {
        Self {
            paste_id: None,
            name: "untitled".to_string(),
            content: String::new(),
            language: None,
            tags: Vec::new(),
            dirty: true,
        }
    }

    fn apply_paste(&mut self, paste: Paste) {
        self.paste_id = Some(paste.id);
        self.name = paste.name;
        self.content = paste.content;
        self.language = paste.language;
        self.tags = paste.tags;
        self.dirty = false;
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

struct LanguageSet;

impl LanguageSet {
    fn all() -> [&'static str; 12] {
        [
            "plain", "rust", "python", "javascript", "typescript", "go", "java", "c",
            "cpp", "sql", "shell", "markdown",
        ]
    }
}
