//! egui desktop UI for LocalPaste.

use std::{
    collections::HashMap,
    fs,
    net::SocketAddr,
    sync::{mpsc, Arc},
    thread,
    time::{Duration, Instant},
};

#[cfg(any(feature = "debug-tools", feature = "profile"))]
use std::collections::VecDeque;

use eframe::egui::{
    self, style::WidgetVisuals, CollapsingHeader, Color32, CornerRadius, FontFamily, FontId, Frame,
    Layout, Margin, Popup, RichText, Stroke, TextStyle, Visuals,
};
use egui_extras::syntax_highlighting::{highlight, CodeTheme};
use rfd::FileDialog;
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
const AUTO_DETECT_MIN_CHARS: usize = 64;
const AUTO_DETECT_THRESHOLD: usize = 512;
const AUTO_DETECT_DEBOUNCE: Duration = Duration::from_millis(300);
const MAX_DETECT_CHARS: usize = 10_000;
/// Content size threshold above which syntax highlighting is disabled.
const HIGHLIGHT_PLAIN_THRESHOLD: usize = 256 * 1024;

/// Frame time threshold for "slow frame" warnings (16ms = ~60 FPS target).
#[cfg(feature = "debug-tools")]
const SLOW_FRAME_THRESHOLD_MS: f32 = 16.0;

/// Number of frame times to keep for rolling statistics.
#[cfg(any(feature = "debug-tools", feature = "profile"))]
const FRAME_TIME_HISTORY_SIZE: usize = 100;

/// Debug state for performance monitoring and diagnostics.
#[cfg(feature = "debug-tools")]
#[derive(Default)]
struct DebugState {
    /// Whether the debug panel window is visible.
    show_panel: bool,
    /// Rolling history of frame times in milliseconds.
    frame_times: VecDeque<f32>,
    /// Total slow frame count since startup.
    slow_frame_count: u64,
    /// Duration of the last paste reload operation in milliseconds.
    last_reload_ms: Option<f32>,
    /// Duration of the last save operation in milliseconds.
    last_save_ms: Option<f32>,
    /// Duration of the last highlight recompute in milliseconds.
    last_highlight_ms: Option<f32>,
}

#[cfg(feature = "debug-tools")]
impl DebugState {
    /// Record a frame time and update slow frame counter.
    fn record_frame_time(&mut self, ms: f32) {
        if self.frame_times.len() >= FRAME_TIME_HISTORY_SIZE {
            self.frame_times.pop_front();
        }
        self.frame_times.push_back(ms);

        if ms > SLOW_FRAME_THRESHOLD_MS {
            self.slow_frame_count += 1;
            eprintln!(
                "[debug-tools] slow frame: {:.2}ms (total slow: {})",
                ms, self.slow_frame_count
            );
        }
    }

    /// Calculate average frame time from history.
    fn avg_frame_time(&self) -> f32 {
        if self.frame_times.is_empty() {
            return 0.0;
        }
        self.frame_times.iter().sum::<f32>() / self.frame_times.len() as f32
    }

    /// Calculate P95 frame time (95th percentile).
    fn p95_frame_time(&self) -> f32 {
        self.percentile_frame_time(95)
    }

    /// Calculate P99 frame time (99th percentile).
    fn p99_frame_time(&self) -> f32 {
        self.percentile_frame_time(99)
    }

    /// Calculate a percentile frame time.
    fn percentile_frame_time(&self, percentile: usize) -> f32 {
        if self.frame_times.is_empty() {
            return 0.0;
        }
        let mut sorted: Vec<f32> = self.frame_times.iter().copied().collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let index = ((percentile as f32 / 100.0) * (sorted.len() - 1) as f32).round() as usize;
        sorted.get(index).copied().unwrap_or(0.0)
    }

    /// Log operation timing to console.
    fn log_operation(&self, op: &str, ms: f32) {
        eprintln!("[debug-tools] {}: {:.2}ms", op, ms);
    }
}

/// Profiling state for the in-app profiler panel.
#[cfg(feature = "profile")]
#[derive(Default)]
struct ProfileState {
    frame_times: VecDeque<f32>,
    last_highlight_ms: Option<f32>,
    last_save_ms: Option<f32>,
}

#[cfg(feature = "profile")]
impl ProfileState {
    fn record_frame_time(&mut self, ms: f32) {
        if self.frame_times.len() >= FRAME_TIME_HISTORY_SIZE {
            self.frame_times.pop_front();
        }
        self.frame_times.push_back(ms);
    }

    fn avg_frame_time(&self) -> f32 {
        if self.frame_times.is_empty() {
            return 0.0;
        }
        self.frame_times.iter().sum::<f32>() / self.frame_times.len() as f32
    }

    fn percentile_frame_time(&self, percentile: usize) -> f32 {
        if self.frame_times.is_empty() {
            return 0.0;
        }
        let mut sorted: Vec<f32> = self.frame_times.iter().copied().collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let index = ((percentile as f32 / 100.0) * (sorted.len() - 1) as f32).round() as usize;
        sorted.get(index).copied().unwrap_or(0.0)
    }
}

/// Language detection state machine.
///
/// Once any language prediction is made (auto or manual), detection NEVER runs
/// again unless the user explicitly resets to "Auto Detect".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum LanguageState {
    /// No language set yet; detection should run.
    #[default]
    Undetected,
    /// Language was auto-detected; detection STOPS.
    AutoDetected,
    /// User explicitly chose a language; detection STOPS.
    ManuallySet,
}

fn compute_line_offsets(text: &str) -> Vec<usize> {
    let mut offsets = Vec::with_capacity(text.lines().count() + 1);
    offsets.push(0);
    let mut count = 0;
    for ch in text.chars() {
        count += 1;
        if ch == '\n' {
            offsets.push(count);
        }
    }
    offsets
}

fn key_to_ascii_letter(key: egui::Key) -> Option<char> {
    use egui::Key::*;
    let ch = match key {
        A => 'a',
        B => 'b',
        C => 'c',
        D => 'd',
        E => 'e',
        F => 'f',
        G => 'g',
        H => 'h',
        I => 'i',
        J => 'j',
        K => 'k',
        L => 'l',
        M => 'm',
        N => 'n',
        O => 'o',
        P => 'p',
        Q => 'q',
        R => 'r',
        S => 's',
        T => 't',
        U => 'u',
        V => 'v',
        W => 'w',
        X => 'x',
        Y => 'y',
        Z => 'z',
        _ => return None,
    };
    Some(ch)
}

fn sanitize_filename(name: &str) -> String {
    let mut sanitized: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    sanitized = sanitized.trim_matches('_').to_string();
    if sanitized.is_empty() {
        "paste".to_string()
    } else {
        sanitized
    }
}

fn default_export_filename(name: &str, extension: &str) -> String {
    let mut stem = sanitize_filename(name);
    if stem.len() > 64 {
        stem.truncate(64);
    }
    if stem.ends_with(&format!(".{}", extension)) {
        stem
    } else {
        format!("{stem}.{extension}")
    }
}

/// Build the application icon bitmap.
///
/// # Returns
/// The icon pixel data for egui.
///
/// # Panics
/// Does not intentionally panic.
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
    filter_counts: HashMap<String, usize>,
    filter_unfiled: usize,
    filter_query: String,
    filter_query_lower: String,
    filter_focus_requested: bool,
    editor: EditorState,
    status: Option<StatusMessage>,
    style_applied: bool,
    folder_dialog: Option<FolderDialog>,
    _server: ServerHandle,
    profile_highlight: bool,
    editor_focused: bool,
    auto_save_blocked: bool,
    #[cfg(feature = "debug-tools")]
    debug_state: DebugState,
    #[cfg(feature = "profile")]
    show_profiler: bool,
    #[cfg(feature = "profile")]
    profile_state: ProfileState,
}

impl LocalPasteApp {
    /// Construct the GUI application using shared backend components.
    ///
    /// # Returns
    /// A fully initialized [`LocalPasteApp`].
    ///
    /// # Errors
    /// Returns an error if configuration, database, or server initialization fails.
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
        let mut startup_status: Option<String> = None;
        let server = if std::env::var("LOCALPASTE_GUI_DISABLE_SERVER").is_ok() {
            info!("API background server disabled via LOCALPASTE_GUI_DISABLE_SERVER");
            ServerHandle::noop()
        } else {
            ServerHandle::start(state.clone(), allow_public)?
        };

        if server.used_fallback {
            if let Some(addr) = server.addr {
                startup_status = Some(format!(
                    "API running on http://{} (auto port because requested port is in use)",
                    addr
                ));
            }
        }

        let profile_highlight = std::env::var("LOCALPASTE_PROFILE_HIGHLIGHT")
            .map(|v| v != "0")
            .unwrap_or(false);

        #[cfg(feature = "debug-tools")]
        {
            eprintln!("[debug-tools] LocalPaste debug mode enabled");
            eprintln!("[debug-tools] Toggle debug panel: Ctrl+Shift+D");
        }

        #[cfg(feature = "profile")]
        {
            puffin::set_scopes_on(true);
            eprintln!("[profile] Puffin profiler enabled");
            eprintln!("[profile] Toggle profiler window: Ctrl+Shift+P");
        }

        let mut app = Self {
            db,
            config: config_arc,
            pastes: Vec::new(),
            folders: Vec::new(),
            paste_index: HashMap::new(),
            folder_index: HashMap::new(),
            selected_id: None,
            folder_focus: None,
            filter_counts: HashMap::new(),
            filter_unfiled: 0,
            filter_query: String::new(),
            filter_query_lower: String::new(),
            filter_focus_requested: false,
            editor: EditorState::default(),
            status: None,
            style_applied: false,
            folder_dialog: None,
            _server: server,
            profile_highlight,
            editor_focused: false,
            auto_save_blocked: false,
            #[cfg(feature = "debug-tools")]
            debug_state: DebugState::default(),
            #[cfg(feature = "profile")]
            show_profiler: false,
            #[cfg(feature = "profile")]
            profile_state: ProfileState::default(),
        };

        if let Some(message) = startup_status {
            app.push_status(StatusLevel::Info, message);
        }

        app.reload_pastes("startup");
        app.reload_folders("startup");
        app.update_filter_cache();

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
        self.style_applied = true;
    }

    fn reload_pastes(&mut self, reason: &str) {
        #[cfg(feature = "debug-tools")]
        let start = Instant::now();

        match self.db.pastes.list(512, None) {
            Ok(mut loaded) => {
                info!("refreshed {} pastes ({})", loaded.len(), reason);
                loaded.sort_by_key(|p| std::cmp::Reverse(p.updated_at));
                self.pastes = loaded;
                self.rebuild_paste_index();
                self.refresh_filter_counts();

                if let Some(selected) = self
                    .selected_id
                    .clone()
                    .or_else(|| self.pastes.first().map(|p| p.id.clone()))
                {
                    self.select_paste(selected, false);
                } else if self.pastes.is_empty() {
                    self.editor = EditorState::new_unsaved(self.folder_focus.clone());
                }
                if self.has_active_filter() {
                    self.ensure_selection_after_filter();
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

        #[cfg(feature = "debug-tools")]
        {
            let ms = start.elapsed().as_secs_f32() * 1000.0;
            self.debug_state.last_reload_ms = Some(ms);
            self.debug_state.log_operation("reload_pastes", ms);
            eprintln!(
                "[debug-tools] reload_pastes: {} pastes, {} folders",
                self.pastes.len(),
                self.folders.len()
            );
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

    fn has_active_filter(&self) -> bool {
        !self.filter_query_lower.is_empty()
    }

    fn update_filter_cache(&mut self) {
        self.filter_query_lower = self.filter_query.to_ascii_lowercase();
        self.refresh_filter_counts();
    }

    fn refresh_filter_counts(&mut self) {
        self.filter_counts.clear();
        self.filter_unfiled = 0;
        if self.filter_query_lower.is_empty() {
            return;
        }

        for paste in &self.pastes {
            if self.matches_filter(paste) {
                if let Some(folder) = &paste.folder_id {
                    *self.filter_counts.entry(folder.clone()).or_insert(0) += 1;
                } else {
                    self.filter_unfiled += 1;
                }
            }
        }
    }

    fn matches_filter(&self, paste: &Paste) -> bool {
        if self.filter_query_lower.is_empty() {
            return true;
        }
        let needle = self.filter_query_lower.as_str();
        Self::contains_case_insensitive(&paste.name, needle)
            || paste
                .tags
                .iter()
                .any(|tag| Self::contains_case_insensitive(tag, needle))
            || paste
                .language
                .as_deref()
                .map(|lang| Self::contains_case_insensitive(lang, needle))
                .unwrap_or(false)
            || Self::contains_case_insensitive(paste.id.as_str(), needle)
    }

    fn contains_case_insensitive(text: &str, needle: &str) -> bool {
        if needle.is_empty() {
            return true;
        }
        if text.len() < needle.len() {
            return text.to_ascii_lowercase().contains(needle);
        }
        text.to_ascii_lowercase().contains(needle)
    }

    fn ensure_selection_after_filter(&mut self) {
        if self.selected_id.is_some() {
            if let Some(selected) = self.selected_id.clone() {
                if self
                    .find_paste(selected.as_str())
                    .map(|paste| self.matches_filter(paste))
                    .unwrap_or(false)
                {
                    return;
                }
            }
        }

        if let Some(next) = self
            .pastes
            .iter()
            .find(|paste| self.matches_filter(paste))
            .cloned()
        {
            let next_id = next.id.clone();
            let selected_changed = self.selected_id.as_deref() != Some(next_id.as_str());
            self.select_paste(next_id, false);
            if selected_changed {
                self.editor.needs_focus = false;
            }
        } else {
            self.selected_id = None;
            self.editor = EditorState::new_unsaved(self.folder_focus.clone());
            self.editor.needs_focus = false;
        }
    }

    fn integrate_paste(&mut self, paste: Paste) {
        let paste_id = paste.id.clone();
        self.pastes.retain(|p| p.id != paste_id);
        self.pastes.push(paste);
        self.pastes.sort_by_key(|p| std::cmp::Reverse(p.updated_at));
        self.rebuild_paste_index();
        if let Some(idx) = self.paste_index.get(&paste_id).copied() {
            let editor_is_current = self.editor.paste_id.as_deref() == Some(paste_id.as_str());
            if self.selected_id.as_deref() == Some(paste_id.as_str()) && !editor_is_current {
                if let Some(updated) = self.pastes.get(idx) {
                    self.editor.apply_paste(updated.clone());
                }
            }
        }
        self.refresh_filter_counts();
        self.ensure_selection_after_filter();
    }

    fn remove_paste_by_id(&mut self, paste_id: &str) -> bool {
        let original_len = self.pastes.len();
        self.pastes.retain(|paste| paste.id != paste_id);
        if self.pastes.len() != original_len {
            self.rebuild_paste_index();
        }
        if self.selected_id.as_deref() == Some(paste_id) {
            self.selected_id = None;
        }
        self.refresh_filter_counts();
        self.ensure_selection_after_filter();
        self.selected_id.is_some()
    }

    fn focus_filter(&mut self) {
        self.filter_focus_requested = true;
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

    fn count_filtered_pastes_in(&self, folder_id: Option<&str>) -> usize {
        if self.filter_query_lower.is_empty() {
            return self.count_pastes_in(folder_id);
        }

        match folder_id {
            Some(id) => self.filter_counts.get(id).copied().unwrap_or(0),
            None => self.filter_unfiled,
        }
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

    fn mark_editor_dirty(&mut self) {
        self.editor.mark_dirty();
        // Set debounce timestamp for small-paste detection (only if still Undetected)
        if self.editor.language_state == LanguageState::Undetected {
            self.editor.language_pending_since = Some(Instant::now());
        }
        // Update line offsets for navigation
        self.editor.line_offsets = compute_line_offsets(&self.editor.content);
        self.auto_save_blocked = false;
    }

    fn render_filter_bar(&mut self, ui: &mut egui::Ui) {
        let total_width = ui.available_width().max(60.0);
        let row_height = ui.spacing().interact_size.y;
        let item_spacing = ui.spacing().item_spacing.x;
        let show_clear = !self.filter_query.is_empty();
        let reserved_for_clear = if show_clear {
            row_height + item_spacing
        } else {
            0.0
        };
        let text_width = (total_width - reserved_for_clear).max(60.0);

        ui.allocate_ui_with_layout(
            egui::vec2(total_width, row_height),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                let response = ui
                    .add_sized(
                        [text_width, row_height],
                        egui::TextEdit::singleline(&mut self.filter_query)
                            .hint_text("Filter pastes…"),
                    )
                    .on_hover_text("Type to filter pastes");

                if self.filter_focus_requested {
                    response.request_focus();
                    self.filter_focus_requested = false;
                }

                if response.changed() {
                    self.update_filter_cache();
                    self.ensure_selection_after_filter();
                }

                let remaining = total_width - text_width;
                if show_clear && remaining >= row_height + item_spacing {
                    let clear_resp = ui
                        .add_sized(
                            [row_height, row_height],
                            egui::Button::new("✕").frame(false),
                        )
                        .on_hover_text("Clear filter");
                    if clear_resp.clicked() {
                        self.filter_query.clear();
                        self.update_filter_cache();
                        self.ensure_selection_after_filter();
                    }
                }
            },
        );
    }

    /// Single-shot language detection with debouncing for typing scenarios.
    ///
    /// Detection only runs when `language_state == Undetected`. Once a language
    /// is detected (auto or manual), detection STOPS until user resets to Auto.
    fn ensure_language_selection(&mut self) {
        // RULE 1: Never detect if already detected or manually set
        if self.editor.language_state != LanguageState::Undetected {
            return;
        }

        let content_len = self.editor.content.len();

        // RULE 2: Very large content uses plain text; stop detection
        if self.is_plain_highlight_mode() {
            self.assign_plain_language();
            return;
        }

        // RULE 3: Skip if content too small to detect
        if content_len < AUTO_DETECT_MIN_CHARS {
            return;
        }

        // RULE 4: For large content (>=threshold), single-shot detect immediately
        if content_len >= AUTO_DETECT_THRESHOLD {
            self.run_detection_once(true);
            return;
        }

        // RULE 5: For small content, debounce detection (typing scenario)
        if let Some(pending_since) = self.editor.language_pending_since {
            if Instant::now().duration_since(pending_since) < AUTO_DETECT_DEBOUNCE {
                return; // Still typing, wait
            }
        }

        self.run_detection_once(false);
    }

    /// Run language detection once and transition to AutoDetected state.
    fn run_detection_once(&mut self, force_plain_on_miss: bool) {
        // Sample only the first MAX_DETECT_CHARS for detection
        let sample = if self.editor.content.len() > MAX_DETECT_CHARS {
            // Find a char boundary near MAX_DETECT_CHARS
            let mut end = MAX_DETECT_CHARS;
            while end < self.editor.content.len() && !self.editor.content.is_char_boundary(end) {
                end += 1;
            }
            &self.editor.content[..end.min(self.editor.content.len())]
        } else {
            &self.editor.content
        };

        if let Some(detected) = crate::models::paste::detect_language(sample) {
            self.editor.language = Some(detected);
            self.editor.language_state = LanguageState::AutoDetected;
            self.editor.language_pending_since = None;
        } else if force_plain_on_miss {
            self.assign_plain_language();
        }
        // If detection fails (returns None), we stay in Undetected state
        // and will try again on next debounce cycle (for small pastes)
        // or never again (for large pastes, since they pass threshold check)
    }

    fn assign_plain_language(&mut self) {
        self.editor.language = Some("plain".to_string());
        self.editor.language_state = LanguageState::AutoDetected;
        self.editor.language_pending_since = None;
    }

    fn is_plain_highlight_mode(&self) -> bool {
        self.editor.content.len() >= HIGHLIGHT_PLAIN_THRESHOLD
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
        let unfiled_filtered = self.count_filtered_pastes_in(None);
        let unfiled_caption = if self.has_active_filter() && unfiled_filtered != unfiled_count {
            format!("Unfiled ({}/{})", unfiled_filtered, unfiled_count)
        } else {
            format!("Unfiled ({})", unfiled_count)
        };
        let unfiled_selected = self.folder_focus.is_none();
        let unfiled_label = if unfiled_selected {
            RichText::new(unfiled_caption.clone()).color(COLOR_ACCENT)
        } else {
            RichText::new(unfiled_caption).color(COLOR_TEXT_PRIMARY)
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
                let filtered_count = self.count_filtered_pastes_in(Some(folder.id.as_str()));
                let label_text = if self.has_active_filter() && filtered_count != paste_count {
                    format!("{} ({}/{})", folder.name, filtered_count, paste_count)
                } else {
                    format!("{} ({})", folder.name, paste_count)
                };
                let is_selected = self.folder_focus.as_deref() == Some(folder.id.as_str());
                let label = if is_selected {
                    RichText::new(label_text.clone()).color(COLOR_ACCENT)
                } else {
                    RichText::new(label_text).color(COLOR_TEXT_PRIMARY)
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
            .filter(|paste| self.matches_filter(paste))
            .map(|paste| paste.id.clone())
            .collect();

        if entries.is_empty() {
            let message = if self.has_active_filter() {
                "No matches"
            } else if folder_id.is_some() {
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
                let label_text = if paste.name.trim().is_empty() {
                    paste.id.chars().take(8).collect()
                } else {
                    paste.name.clone()
                };
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
        if self.auto_save_blocked {
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
        #[cfg(any(feature = "debug-tools", feature = "profile"))]
        let start = Instant::now();

        if self.editor.name.trim().is_empty() {
            self.push_status(StatusLevel::Error, "Name cannot be empty".into());
            return;
        }
        if !self.validate_editor_state() {
            self.auto_save_blocked = true;
            return;
        }
        self.auto_save_blocked = false;

        if let Some(id) = &self.editor.paste_id {
            self.update_existing_paste(id.clone());
        } else {
            self.persist_new_paste();
        }

        #[cfg(feature = "debug-tools")]
        {
            let ms = start.elapsed().as_secs_f32() * 1000.0;
            self.debug_state.last_save_ms = Some(ms);
            self.debug_state.log_operation("save", ms);
        }

        #[cfg(feature = "profile")]
        {
            let ms = start.elapsed().as_secs_f32() * 1000.0;
            self.profile_state.last_save_ms = Some(ms);
        }
    }

    fn export_current_paste(&mut self) {
        if self.editor.content.is_empty() {
            self.push_status(StatusLevel::Info, "Nothing to export".into());
            return;
        }

        let language = self
            .editor
            .language
            .clone()
            .unwrap_or_else(|| "plain".to_string());
        let extension = LanguageSet::extension(language.as_str());
        let default_name = default_export_filename(&self.editor.name, extension);

        let dialog = FileDialog::new().set_file_name(default_name);
        let dialog = if let Some(label) = LanguageSet::label(language.as_str()) {
            dialog.add_filter(label, &[extension])
        } else {
            dialog.add_filter("Export", &[extension])
        };

        match dialog.save_file() {
            Some(path) => match fs::write(&path, &self.editor.content) {
                Ok(_) => {
                    self.push_status(StatusLevel::Info, format!("Exported to {}", path.display()))
                }
                Err(err) => self.push_status(
                    StatusLevel::Error,
                    format!("Export failed ({}): {}", path.display(), err),
                ),
            },
            None => {
                self.push_status(StatusLevel::Info, "Export cancelled".into());
            }
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
        paste.language_is_manual = self.editor.language_state == LanguageState::ManuallySet;
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
                self.editor.sync_after_save(&paste);
                self.selected_id = Some(paste.id.clone());
                self.folder_focus = paste.folder_id.clone();
                self.integrate_paste(paste);
            }
            Err(err) => {
                error!("failed to create paste: {}", err);
                self.auto_save_blocked = true;
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

        let folder_value = self.editor.folder_id.clone();
        let folder_changed = previous.folder_id.as_deref() != self.editor.folder_id.as_deref();
        let folder_update = match (&folder_value, folder_changed) {
            (Some(id), _) => Some(id.clone()),
            (None, true) => Some(String::new()),
            (None, false) => None,
        };
        let is_manual = self.editor.language_state == LanguageState::ManuallySet;
        let language_update = match (&self.editor.language, is_manual) {
            (Some(lang), manual) => Some((Some(lang.clone()), manual)),
            (None, true) => Some((None, true)), // explicit clear to Auto
            (None, false) => None,
        };
        let update = UpdatePasteRequest {
            content: Some(self.editor.content.clone()),
            name: Some(self.editor.name.clone()),
            language: language_update.as_ref().and_then(|(lang, _)| lang.clone()),
            language_is_manual: language_update.map(|(_, manual)| manual),
            folder_id: folder_update.clone(),
            tags: Some(self.editor.tags.clone()),
        };

        let result = if folder_changed {
            let new_folder = self.editor.folder_id.as_deref();
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
                self.editor.sync_after_save(&updated);
                self.selected_id = Some(updated.id.clone());
                self.folder_focus = updated.folder_id.clone();
                self.integrate_paste(updated);
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
                self.auto_save_blocked = true;
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
                    let has_selection = self.remove_paste_by_id(&id);
                    if !has_selection {
                        self.editor = EditorState::new_unsaved(self.folder_focus.clone());
                    }
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
    addr: Option<SocketAddr>,
    used_fallback: bool,
}

impl ServerHandle {
    fn noop() -> Self {
        Self {
            shutdown: None,
            thread: None,
            addr: None,
            used_fallback: false,
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
                let mut used_fallback = false;
                let listener = match rt.block_on(tokio::net::TcpListener::bind(bind_addr)) {
                    Ok(listener) => listener,
                    Err(err) if err.kind() == std::io::ErrorKind::AddrInUse => {
                        warn!(
                            "API bind address {} is in use; falling back to an auto port",
                            bind_addr
                        );
                        used_fallback = true;
                        let fallback_addr = SocketAddr::new(bind_addr.ip(), 0);
                        match rt.block_on(tokio::net::TcpListener::bind(fallback_addr)) {
                            Ok(listener) => listener,
                            Err(fallback_err) => {
                                let _ = ready_tx.send(Err(format!(
                                    "failed to bind server socket: {}",
                                    fallback_err
                                )));
                                return;
                            }
                        }
                    }
                    Err(err) => {
                        let _ =
                            ready_tx.send(Err(format!("failed to bind server socket: {}", err)));
                        return;
                    }
                };

                let actual_addr = listener.local_addr().unwrap_or(bind_addr);
                if used_fallback {
                    warn!(
                        "API listening on http://{} (auto port; {} was in use)",
                        actual_addr, bind_addr
                    );
                } else {
                    info!("API listening on http://{}", actual_addr);
                }
                let _ = ready_tx.send(Ok((actual_addr, used_fallback)));

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

        let mut thread_handle = Some(thread);

        match ready_rx.recv() {
            Ok(Ok((addr, used_fallback))) => {
                if !addr.ip().is_loopback() {
                    warn!("binding to non-localhost address {}", addr);
                }
                if used_fallback {
                    warn!("API listening on http://{} (auto port)", addr);
                } else {
                    info!("API listening on http://{}", addr);
                }
                Ok(Self {
                    shutdown: Some(shutdown_tx),
                    thread: thread_handle.take(),
                    addr: Some(addr),
                    used_fallback,
                })
            }
            Ok(Err(message)) => {
                let _ = shutdown_tx.send(());
                if let Some(handle) = thread_handle.take() {
                    let _ = handle.join();
                }
                Err(AppError::DatabaseError(message))
            }
            Err(_) => {
                let _ = shutdown_tx.send(());
                if let Some(handle) = thread_handle.take() {
                    let _ = handle.join();
                }
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
        // Start new profiler frame
        #[cfg(feature = "profile")]
        puffin::GlobalProfiler::lock().new_frame();

        // Record frame start time for debug-tools / profiler
        #[cfg(any(feature = "debug-tools", feature = "profile"))]
        let frame_start = Instant::now();

        #[cfg(feature = "profile")]
        puffin::profile_function!();

        self.ensure_style(ctx);
        self.ensure_language_selection();
        let editor_was_focused = self.editor_focused;

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
            if input.modifiers.command && input.key_pressed(egui::Key::F) && !editor_was_focused {
                self.focus_filter();
            }
            if input.modifiers.command && input.key_pressed(egui::Key::K) && !editor_was_focused {
                self.focus_filter();
            }
            // Ctrl+Shift+D toggles debug panel
            #[cfg(feature = "debug-tools")]
            if input.modifiers.command && input.modifiers.shift && input.key_pressed(egui::Key::D) {
                self.debug_state.show_panel = !self.debug_state.show_panel;
                eprintln!(
                    "[debug-tools] debug panel {}",
                    if self.debug_state.show_panel {
                        "opened"
                    } else {
                        "closed"
                    }
                );
            }
            // Ctrl+Shift+P toggles profiler window
            #[cfg(feature = "profile")]
            if input.modifiers.command && input.modifiers.shift && input.key_pressed(egui::Key::P) {
                self.show_profiler = !self.show_profiler;
                eprintln!(
                    "[profile] profiler window {}",
                    if self.show_profiler {
                        "opened"
                    } else {
                        "closed"
                    }
                );
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
                    self.render_filter_bar(ui);
                    ui.add_space(10.0);
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
                ui.columns(3, |columns| {
                    columns[0].vertical(|ui| {
                        if let Some(status) = &self.status {
                            ui.label(
                                RichText::new(&status.text).color(Self::status_color(status.level)),
                            );
                        } else if self.editor.dirty {
                            ui.label(RichText::new("Unsaved changes").color(COLOR_ACCENT));
                        } else {
                            ui.label(RichText::new("Ready").color(COLOR_TEXT_MUTED));
                        }
                    });

                    columns[1].with_layout(
                        Layout::centered_and_justified(egui::Direction::LeftToRight),
                        |ui| {
                            ui.vertical(|ui| {
                                let id_label =
                                    self.editor.paste_id.as_deref().unwrap_or("unsaved draft");
                                ui.label(
                                    RichText::new(id_label).monospace().color(COLOR_TEXT_MUTED),
                                );
                                if self.is_plain_highlight_mode() {
                                    ui.label(
                                        RichText::new("Highlighting trimmed for large paste")
                                            .size(11.0)
                                            .color(COLOR_TEXT_MUTED),
                                    );
                                }
                            });
                        },
                    );

                    columns[2].with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                        let char_count = self.editor.content.chars().count();
                        ui.label(
                            RichText::new(format!("{char_count} chars")).color(COLOR_TEXT_MUTED),
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
                            let combo_id = ui.id().with("language_select");
                            egui::ComboBox::from_id_salt("language_select")
                                .selected_text(current_language_label)
                                .show_ui(ui, |ui| {
                                    ui.set_min_width(160.0);
                                    let popup_open = Popup::is_id_open(ui.ctx(), combo_id);
                                    let typed_letter = if popup_open {
                                        ui.ctx().input(|input| {
                                            input.events.iter().rev().find_map(
                                                |event| match event {
                                                    egui::Event::Text(text) => text
                                                        .chars()
                                                        .rev()
                                                        .find(|c| c.is_ascii_alphabetic())
                                                        .map(|c| c.to_ascii_lowercase()),
                                                    egui::Event::Key {
                                                        key,
                                                        pressed,
                                                        repeat,
                                                        modifiers,
                                                        ..
                                                    } if *pressed
                                                        && !*repeat
                                                        && !modifiers.alt
                                                        && !modifiers.ctrl
                                                        && !modifiers.command
                                                        && !modifiers.mac_cmd =>
                                                    {
                                                        key_to_ascii_letter(*key)
                                                    }
                                                    _ => None,
                                                },
                                            )
                                        })
                                    } else {
                                        None
                                    };

                                    let mut auto_scroll_target: Option<&'static str> = None;
                                    if let Some(letter) = typed_letter {
                                        if let Some(option) =
                                            LanguageSet::options().iter().find(|opt| {
                                                opt.label
                                                    .chars()
                                                    .next()
                                                    .map(|c| c.to_ascii_lowercase())
                                                    == Some(letter)
                                            })
                                        {
                                            if self.editor.language.as_deref() != Some(option.id) {
                                                self.editor.language = Some(option.id.to_string());
                                                self.editor.language_state =
                                                    LanguageState::ManuallySet;
                                                self.editor.language_pending_since = None;
                                                self.mark_editor_dirty();
                                            } else {
                                                self.editor.language_state =
                                                    LanguageState::ManuallySet;
                                            }
                                            auto_scroll_target = Some(option.id);
                                        }
                                    }

                                    if ui
                                        .selectable_value(&mut self.editor.language, None, "Auto")
                                        .clicked()
                                    {
                                        // Reset to Undetected allows re-detection
                                        self.editor.language_state = LanguageState::Undetected;
                                        self.editor.language_pending_since = Some(Instant::now());
                                        self.mark_editor_dirty();
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
                                            self.editor.language_state = LanguageState::ManuallySet;
                                            self.editor.language_pending_since = None;
                                            self.mark_editor_dirty();
                                        }
                                        if Some(option.id) == auto_scroll_target {
                                            ui.scroll_to_cursor(Some(egui::Align::Center));
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
                                        self.mark_editor_dirty();
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
                                            self.mark_editor_dirty();
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

                            let export_btn =
                                egui::Button::new(RichText::new("Export").color(Color32::WHITE))
                                    .fill(COLOR_ACCENT)
                                    .min_size(egui::vec2(110.0, 36.0));
                            if ui.add(export_btn).clicked() {
                                self.export_current_paste();
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
                    let highlight_language = self
                        .editor
                        .language
                        .clone()
                        .unwrap_or_else(|| "plain".to_string());
                    let syntax_token = LanguageSet::highlight_token(highlight_language.as_str())
                        .unwrap_or(highlight_language.as_str())
                        .to_string();

                    // Use plain text for very large content to avoid perf issues
                    let use_plain_mode = self.is_plain_highlight_mode();
                    let theme = CodeTheme::from_memory(ui.ctx(), ui.style());

                    egui::ScrollArea::vertical()
                        .id_salt("editor_scroll")
                        .auto_shrink([false; 2])
                        .show(ui, |ui| {
                            let mut layouter =
                                |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap_width: f32| {
                                    let text = text.as_str();
                                    let mut job = if use_plain_mode {
                                        // Plain text for large files
                                        egui::text::LayoutJob::simple(
                                            text.to_owned(),
                                            egui::FontId::monospace(14.0),
                                            ui.visuals().text_color(),
                                            wrap_width,
                                        )
                                    } else {
                                        // Syntax highlighted (memoized by egui_extras)
                                        highlight(ui.ctx(), ui.style(), &theme, text, &syntax_token)
                                    };
                                    job.wrap.max_width = wrap_width;
                                    ui.fonts_mut(|f| f.layout_job(job))
                                };

                            let editor = egui::TextEdit::multiline(&mut self.editor.content)
                                .font(text_style)
                                .desired_width(f32::INFINITY)
                                .desired_rows(32)
                                .frame(false)
                                .layouter(&mut layouter);

                            let layout_start =
                                self.profile_highlight.then_some((Instant::now(), ui.id()));
                            let response = ui.add(editor);
                            self.editor_focused = response.has_focus();
                            if let Some((started, _)) = layout_start {
                                let elapsed = started.elapsed();
                                #[cfg(any(feature = "debug-tools", feature = "profile"))]
                                let ms = elapsed.as_secs_f32() * 1000.0;
                                debug!(
                                    "text_edit_layout duration_ms={:.3} chars={}",
                                    elapsed.as_secs_f64() * 1_000.0,
                                    self.editor.content.len()
                                );
                                #[cfg(feature = "debug-tools")]
                                {
                                    self.debug_state.last_highlight_ms = Some(ms);
                                }
                                #[cfg(feature = "profile")]
                                {
                                    self.profile_state.last_highlight_ms = Some(ms);
                                }
                            }
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
                                self.mark_editor_dirty();
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
        #[cfg(any(feature = "debug-tools", feature = "profile"))]
        let frame_ms = frame_start.elapsed().as_secs_f32() * 1000.0;

        // Debug panel and frame timing (debug-tools feature)
        #[cfg(feature = "debug-tools")]
        {
            self.debug_state.record_frame_time(frame_ms);
            if self.debug_state.show_panel {
                self.render_debug_panel(ctx);
            }
        }

        // Manual profiler panel (profile feature)
        #[cfg(feature = "profile")]
        {
            self.profile_state.record_frame_time(frame_ms);
            if self.show_profiler {
                self.render_profiler_panel(ctx);
            }
        }

        self.handle_auto_save(ctx);
    }
}

#[cfg(feature = "debug-tools")]
impl LocalPasteApp {
    /// Render the debug panel window with performance metrics.
    fn render_debug_panel(&mut self, ctx: &egui::Context) {
        egui::Window::new("Debug Tools")
            .default_width(320.0)
            .resizable(true)
            .collapsible(true)
            .show(ctx, |ui| {
                ui.heading("Frame Timing");
                ui.separator();

                let avg = self.debug_state.avg_frame_time();
                let p95 = self.debug_state.p95_frame_time();
                let p99 = self.debug_state.p99_frame_time();
                let slow_count = self.debug_state.slow_frame_count;
                let sample_count = self.debug_state.frame_times.len();

                ui.horizontal(|ui| {
                    ui.label("Samples:");
                    ui.monospace(format!("{}", sample_count));
                });
                ui.horizontal(|ui| {
                    ui.label("Avg:");
                    ui.monospace(format!("{:.2} ms", avg));
                });
                ui.horizontal(|ui| {
                    ui.label("P95:");
                    ui.monospace(format!("{:.2} ms", p95));
                });
                ui.horizontal(|ui| {
                    ui.label("P99:");
                    ui.monospace(format!("{:.2} ms", p99));
                });
                ui.horizontal(|ui| {
                    ui.label("Slow frames (>16ms):");
                    ui.monospace(format!("{}", slow_count));
                });

                ui.add_space(12.0);
                ui.heading("Data Counts");
                ui.separator();

                ui.horizontal(|ui| {
                    ui.label("Pastes:");
                    ui.monospace(format!("{}", self.pastes.len()));
                });
                ui.horizontal(|ui| {
                    ui.label("Folders:");
                    ui.monospace(format!("{}", self.folders.len()));
                });

                ui.add_space(12.0);
                ui.heading("Last Operation Timings");
                ui.separator();

                if let Some(ms) = self.debug_state.last_reload_ms {
                    ui.horizontal(|ui| {
                        ui.label("Reload:");
                        ui.monospace(format!("{:.2} ms", ms));
                    });
                }
                if let Some(ms) = self.debug_state.last_save_ms {
                    ui.horizontal(|ui| {
                        ui.label("Save:");
                        ui.monospace(format!("{:.2} ms", ms));
                    });
                }
                if let Some(ms) = self.debug_state.last_highlight_ms {
                    ui.horizontal(|ui| {
                        ui.label("Highlight:");
                        ui.monospace(format!("{:.2} ms", ms));
                    });
                }

                ui.add_space(12.0);
                ui.heading("Editor State");
                ui.separator();

                ui.horizontal(|ui| {
                    ui.label("Content size:");
                    ui.monospace(format!("{} bytes", self.editor.content.len()));
                });
                ui.horizontal(|ui| {
                    ui.label("Dirty:");
                    ui.monospace(format!("{}", self.editor.dirty));
                });
                ui.horizontal(|ui| {
                    ui.label("Plain highlight (>256KB):");
                    ui.monospace(format!("{}", self.is_plain_highlight_mode()));
                });
                if let Some(lang) = &self.editor.language {
                    ui.horizontal(|ui| {
                        ui.label("Language:");
                        ui.monospace(lang);
                    });
                }

                ui.add_space(12.0);
                if ui.button("Close (Ctrl+Shift+D)").clicked() {
                    self.debug_state.show_panel = false;
                }
            });
    }
}

#[cfg(feature = "profile")]
impl LocalPasteApp {
    fn render_profiler_panel(&mut self, ctx: &egui::Context) {
        egui::Window::new("Profiler")
            .default_width(320.0)
            .resizable(true)
            .collapsible(true)
            .show(ctx, |ui| {
                ui.heading("Frame Timing");
                ui.separator();

                let avg = self.profile_state.avg_frame_time();
                let p95 = self.profile_state.percentile_frame_time(95);
                let p99 = self.profile_state.percentile_frame_time(99);
                let sample_count = self.profile_state.frame_times.len();

                ui.horizontal(|ui| {
                    ui.label("Samples:");
                    ui.monospace(format!("{}", sample_count));
                });
                ui.horizontal(|ui| {
                    ui.label("Avg:");
                    ui.monospace(format!("{:.2} ms", avg));
                });
                ui.horizontal(|ui| {
                    ui.label("P95:");
                    ui.monospace(format!("{:.2} ms", p95));
                });
                ui.horizontal(|ui| {
                    ui.label("P99:");
                    ui.monospace(format!("{:.2} ms", p99));
                });

                ui.add_space(12.0);
                ui.heading("Last Operation Timings");
                ui.separator();

                if let Some(ms) = self.profile_state.last_highlight_ms {
                    ui.horizontal(|ui| {
                        ui.label("Highlight:");
                        ui.monospace(format!("{:.2} ms", ms));
                    });
                }
                if let Some(ms) = self.profile_state.last_save_ms {
                    ui.horizontal(|ui| {
                        ui.label("Save:");
                        ui.monospace(format!("{:.2} ms", ms));
                    });
                }

                ui.add_space(12.0);
                if ui.button("Close (Ctrl+Shift+P)").clicked() {
                    self.show_profiler = false;
                }
            });
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
    language_state: LanguageState,
    language_pending_since: Option<Instant>,
    folder_id: Option<String>,
    tags: Vec<String>,
    dirty: bool,
    last_modified: Option<Instant>,
    needs_focus: bool,
    line_offsets: Vec<usize>,
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
        self.language_state = match (&paste.language, paste.language_is_manual) {
            (Some(_), true) => LanguageState::ManuallySet,
            (Some(_), false) => LanguageState::AutoDetected,
            (None, _) => LanguageState::Undetected,
        };
        self.language = paste.language;
        self.language_pending_since = None;
        self.folder_id = paste.folder_id;
        self.tags = paste.tags;
        self.mark_pristine();
        self.needs_focus = true;
        self.line_offsets = compute_line_offsets(&self.content);
    }

    fn sync_after_save(&mut self, paste: &Paste) {
        self.paste_id = Some(paste.id.clone());
        self.name = paste.name.clone();
        self.folder_id = paste.folder_id.clone();
        self.tags = paste.tags.clone();
        self.language = paste.language.clone();
        self.language_state = match (&paste.language, paste.language_is_manual) {
            (Some(_), true) => LanguageState::ManuallySet,
            (Some(_), false) => LanguageState::AutoDetected,
            (None, _) => LanguageState::Undetected,
        };
        self.mark_pristine();
        self.needs_focus = false;
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
            language_state: LanguageState::default(),
            language_pending_since: None,
            folder_id: None,
            tags: Vec::new(),
            dirty: false,
            last_modified: None,
            needs_focus: false,
            line_offsets: vec![0],
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
    extension: &'static str,
}

struct LanguageSet;

impl LanguageSet {
    fn options() -> &'static [LanguageOption] {
        const OPTIONS: &[LanguageOption] = &[
            LanguageOption {
                id: "plain",
                label: "Plain Text",
                highlight: None,
                extension: "txt",
            },
            LanguageOption {
                id: "c",
                label: "C",
                highlight: Some("c"),
                extension: "c",
            },
            LanguageOption {
                id: "cpp",
                label: "C++",
                highlight: Some("cpp"),
                extension: "cpp",
            },
            LanguageOption {
                id: "csharp",
                label: "C#",
                highlight: Some("cs"),
                extension: "cs",
            },
            LanguageOption {
                id: "css",
                label: "CSS",
                highlight: Some("css"),
                extension: "css",
            },
            LanguageOption {
                id: "go",
                label: "Go",
                highlight: Some("go"),
                extension: "go",
            },
            LanguageOption {
                id: "html",
                label: "HTML",
                highlight: Some("html"),
                extension: "html",
            },
            LanguageOption {
                id: "java",
                label: "Java",
                highlight: Some("java"),
                extension: "java",
            },
            LanguageOption {
                id: "javascript",
                label: "JavaScript",
                highlight: Some("js"),
                extension: "js",
            },
            LanguageOption {
                id: "json",
                label: "JSON",
                highlight: Some("json"),
                extension: "json",
            },
            LanguageOption {
                id: "latex",
                label: "LaTeX",
                highlight: Some("tex"),
                extension: "tex",
            },
            LanguageOption {
                id: "markdown",
                label: "Markdown",
                highlight: Some("md"),
                extension: "md",
            },
            LanguageOption {
                id: "python",
                label: "Python",
                highlight: Some("py"),
                extension: "py",
            },
            LanguageOption {
                id: "rust",
                label: "Rust",
                highlight: Some("rs"),
                extension: "rs",
            },
            LanguageOption {
                id: "shell",
                label: "Shell / Bash",
                highlight: Some("sh"),
                extension: "sh",
            },
            LanguageOption {
                id: "sql",
                label: "SQL",
                highlight: Some("sql"),
                extension: "sql",
            },
            LanguageOption {
                id: "toml",
                label: "TOML",
                highlight: Some("toml"),
                extension: "toml",
            },
            LanguageOption {
                id: "typescript",
                label: "TypeScript",
                highlight: Some("ts"),
                extension: "ts",
            },
            LanguageOption {
                id: "xml",
                label: "XML",
                highlight: Some("xml"),
                extension: "xml",
            },
            LanguageOption {
                id: "yaml",
                label: "YAML",
                highlight: Some("yml"),
                extension: "yml",
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

    fn extension(id: &str) -> &'static str {
        Self::options()
            .iter()
            .find(|opt| opt.id == id)
            .map(|opt| opt.extension)
            .unwrap_or("txt")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::{App as _, Frame};
    use std::sync::Mutex;
    use tempfile::TempDir;

    static TEST_MUTEX: Mutex<()> = Mutex::new(());

    fn init_app(max_size: usize) -> (LocalPasteApp, TempDir) {
        let _env_guard = TEST_MUTEX.lock().expect("test mutex poisoned");
        let temp = TempDir::new().expect("temp dir");
        let db_path = temp.path().join("db");

        std::env::set_var("DB_PATH", db_path.to_string_lossy().to_string());
        std::env::set_var("MAX_PASTE_SIZE", max_size.to_string());
        std::env::set_var("BIND", "127.0.0.1:0");
        std::env::set_var("LOCALPASTE_GUI_DISABLE_SERVER", "1");

        let mut app = None;
        for _ in 0..3 {
            match LocalPasteApp::initialise() {
                Ok(instance) => {
                    app = Some(instance);
                    break;
                }
                Err(AppError::DatabaseError(msg)) if msg.contains("locked") => {
                    let _ = std::fs::remove_dir_all(&db_path);
                    continue;
                }
                Err(other) => panic!("app init failed: {other}"),
            }
        }
        let app = app.expect("app init");

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

    #[test]
    fn editor_sync_after_save_preserves_focus_and_content() {
        let (mut app, _guard) = init_app(1024);
        app.editor.content = "hello world".to_string();
        app.editor.dirty = true;
        app.editor.needs_focus = true;
        let mut paste = Paste::new(app.editor.content.clone(), app.editor.name.clone());
        paste.id = "existing".to_string();
        paste.name = "server-name".to_string();
        paste.folder_id = Some("folder".to_string());
        paste.tags.push("tag".to_string());
        paste.language = Some("rust".to_string());

        app.editor.sync_after_save(&paste);

        assert_eq!(app.editor.content, "hello world");
        assert_eq!(app.editor.name, "server-name");
        assert_eq!(app.editor.paste_id.as_deref(), Some("existing"));
        assert_eq!(app.editor.folder_id, paste.folder_id);
        assert_eq!(app.editor.tags, paste.tags);
        assert_eq!(app.editor.language, paste.language);
        assert!(!app.editor.dirty);
        assert!(!app.editor.needs_focus);
    }

    #[test]
    fn filter_bar_handles_tiny_width() {
        let (mut app, _guard) = init_app(1024);
        app.filter_query = "beans".to_string();
        app.filter_focus_requested = true;
        app.update_filter_cache();

        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(12.0, 120.0),
            )),
            ..Default::default()
        };
        ctx.begin_pass(input);
        egui::SidePanel::left("filter_test")
            .exact_width(12.0)
            .show(&ctx, |ui| {
                app.render_filter_bar(ui);
            });
        let _ = ctx.end_pass();
        assert!(
            !app.filter_focus_requested,
            "rendering should clear pending focus request"
        );

        app.filter_query.clear();
        app.update_filter_cache();
        let input2 = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(4.0, 120.0),
            )),
            ..Default::default()
        };
        ctx.begin_pass(input2);
        egui::SidePanel::left("filter_test_small")
            .exact_width(4.0)
            .show(&ctx, |ui| {
                app.render_filter_bar(ui);
            });
        let _ = ctx.end_pass();
    }

    #[test]
    fn gui_update_smoke_runs_once() {
        let (mut app, _guard) = init_app(1024);
        let ctx = egui::Context::default();
        ctx.begin_pass(egui::RawInput::default());
        let mut frame = Frame::_new_kittest();
        app.update(&ctx, &mut frame);
        let _ = ctx.end_pass();
    }
}
