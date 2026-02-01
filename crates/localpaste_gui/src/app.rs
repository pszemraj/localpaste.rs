//! Native egui app skeleton for the LocalPaste rewrite.

use crate::backend::{spawn_backend, BackendHandle, CoreCmd, CoreEvent, PasteSummary};
use eframe::egui::{
    self,
    style::WidgetVisuals,
    text::{LayoutJob, LayoutSection, TextFormat},
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, RichText, Stroke,
    TextStyle, Visuals,
};
use egui_extras::syntax_highlighting::CodeTheme;
use localpaste_core::models::paste::Paste;
use localpaste_core::{Config, Database};
use localpaste_server::{AppState, EmbeddedServer, PasteLockManager};
use std::any::TypeId;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use syntect::highlighting::{HighlightState, Highlighter, Style, ThemeSet};
use syntect::parsing::{ParseState, ScopeStack, SyntaxSet};
use syntect::util::LinesWithEndings;
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
    selected_content: EditorBuffer,
    editor_cache: EditorLayoutCache,
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
const HIGHLIGHT_SLOW_MS: f32 = 14.0;
const HIGHLIGHT_BACKOFF: Duration = Duration::from_millis(200);

#[derive(Default)]
struct EditorBuffer {
    text: String,
    revision: u64,
}

impl EditorBuffer {
    fn new(text: String) -> Self {
        Self { text, revision: 0 }
    }

    fn reset(&mut self, text: String) {
        self.text = text;
        self.revision = 0;
    }

    fn len(&self) -> usize {
        self.text.len()
    }

    fn chars_len(&self) -> usize {
        self.text.chars().count()
    }

    fn to_string(&self) -> String {
        self.text.clone()
    }
}

impl egui::TextBuffer for EditorBuffer {
    fn is_mutable(&self) -> bool {
        true
    }

    fn as_str(&self) -> &str {
        self.text.as_str()
    }

    fn insert_text(&mut self, text: &str, char_index: usize) -> usize {
        let inserted = <String as egui::TextBuffer>::insert_text(&mut self.text, text, char_index);
        if inserted > 0 {
            self.revision = self.revision.wrapping_add(1);
        }
        inserted
    }

    fn delete_char_range(&mut self, char_range: std::ops::Range<usize>) {
        if char_range.start == char_range.end {
            return;
        }
        <String as egui::TextBuffer>::delete_char_range(&mut self.text, char_range);
        self.revision = self.revision.wrapping_add(1);
    }

    fn clear(&mut self) {
        if self.text.is_empty() {
            return;
        }
        self.text.clear();
        self.revision = self.revision.wrapping_add(1);
    }

    fn replace_with(&mut self, text: &str) {
        if self.text == text {
            return;
        }
        self.text.clear();
        self.text.push_str(text);
        self.revision = self.revision.wrapping_add(1);
    }

    fn take(&mut self) -> String {
        self.revision = self.revision.wrapping_add(1);
        std::mem::take(&mut self.text)
    }

    fn type_id(&self) -> TypeId {
        TypeId::of::<Self>()
    }
}

#[derive(Default)]
struct EditorLayoutCache {
    revision: u64,
    language_hint: String,
    use_plain: bool,
    wrap_width: f32,
    font_id: Option<FontId>,
    theme: Option<CodeTheme>,
    pixels_per_point: f32,
    galley: Option<Arc<egui::Galley>>,
    highlight_cache: HighlightCache,
    last_highlight_ms: Option<f32>,
    highlight_backoff_until: Option<Instant>,
}

#[derive(Clone, PartialEq, Eq)]
struct HighlightStateSnapshot {
    parse: ParseState,
    highlight: HighlightState,
}

#[derive(Clone)]
struct HighlightLineCache {
    hash: u64,
    sections: Vec<LayoutSection>,
    end_state: HighlightStateSnapshot,
}

#[derive(Default)]
struct HighlightCache {
    language_hint: String,
    theme_key: String,
    lines: Vec<HighlightLineCache>,
}

impl HighlightCache {
    fn clear_if_mismatch(&mut self, language_hint: &str, theme_key: &str) {
        if self.language_hint != language_hint || self.theme_key != theme_key {
            self.language_hint = language_hint.to_string();
            self.theme_key = theme_key.to_string();
            self.lines.clear();
        }
    }
}

impl EditorLayoutCache {
    fn layout(
        &mut self,
        ui: &egui::Ui,
        text: &dyn egui::TextBuffer,
        wrap_width: f32,
        language_hint: &str,
        use_plain: bool,
        theme: Option<&CodeTheme>,
        editor_font: &FontId,
        syntect: &SyntectSettings,
    ) -> Arc<egui::Galley> {
        let Some(revision) = editor_buffer_revision(text) else {
            return self.build_galley(
                ui,
                text.as_str(),
                wrap_width,
                language_hint,
                use_plain,
                theme,
                editor_font,
                syntect,
            );
        };

        let pixels_per_point = ui.ctx().pixels_per_point();
        let wrap_width = wrap_width.max(0.0).round();
        let theme_value = if use_plain { None } else { theme.cloned() };

        let cache_hit = self.galley.is_some()
            && self.revision == revision
            && self.use_plain == use_plain
            && self.wrap_width == wrap_width
            && self.pixels_per_point == pixels_per_point
            && self.language_hint == language_hint
            && self.font_id.as_ref() == Some(editor_font)
            && self.theme == theme_value;

        if cache_hit {
            return self.galley.as_ref().expect("cached galley").clone();
        }

        let started = Instant::now();
        let galley = self.build_galley(
            ui,
            text.as_str(),
            wrap_width,
            language_hint,
            use_plain,
            theme,
            editor_font,
            syntect,
        );
        if use_plain {
            self.last_highlight_ms = None;
        } else {
            let elapsed_ms = started.elapsed().as_secs_f32() * 1000.0;
            self.last_highlight_ms = Some(elapsed_ms);
            if elapsed_ms > HIGHLIGHT_SLOW_MS && text.as_str().len() >= HIGHLIGHT_DEBOUNCE_MIN_BYTES
            {
                self.highlight_backoff_until = Some(Instant::now() + HIGHLIGHT_BACKOFF);
            } else {
                self.highlight_backoff_until = None;
            }
        }

        self.revision = revision;
        self.use_plain = use_plain;
        self.wrap_width = wrap_width;
        self.pixels_per_point = pixels_per_point;
        self.language_hint = language_hint.to_string();
        self.font_id = Some(editor_font.clone());
        self.theme = theme_value;
        self.galley = Some(galley.clone());

        galley
    }

    fn build_galley(
        &mut self,
        ui: &egui::Ui,
        text: &str,
        wrap_width: f32,
        language_hint: &str,
        use_plain: bool,
        theme: Option<&CodeTheme>,
        editor_font: &FontId,
        syntect: &SyntectSettings,
    ) -> Arc<egui::Galley> {
        let mut job = if use_plain {
            LayoutJob::simple(
                text.to_owned(),
                editor_font.clone(),
                ui.visuals().text_color(),
                wrap_width,
            )
        } else {
            let theme = theme.expect("theme required for highlighted layout");
            self.build_highlight_job(ui, text, language_hint, theme, editor_font, syntect)
        };
        job.wrap.max_width = wrap_width;
        ui.fonts_mut(|f| f.layout_job(job))
    }

    fn build_highlight_job(
        &mut self,
        ui: &egui::Ui,
        text: &str,
        language_hint: &str,
        theme: &CodeTheme,
        editor_font: &FontId,
        settings: &SyntectSettings,
    ) -> LayoutJob {
        let theme_key = syntect_theme_key(theme);
        self.highlight_cache
            .clear_if_mismatch(language_hint, theme_key);

        let syntax = settings
            .ps
            .find_syntax_by_name(language_hint)
            .or_else(|| settings.ps.find_syntax_by_extension(language_hint))
            .unwrap_or_else(|| settings.ps.find_syntax_plain_text());
        let theme = settings
            .ts
            .themes
            .get(theme_key)
            .unwrap_or_else(|| settings.ts.themes.values().next().expect("theme"));

        let highlighter = Highlighter::new(theme);
        let mut parse_state = ParseState::new(syntax);
        let mut highlight_state = HighlightState::new(&highlighter, ScopeStack::new());
        let default_state = HighlightStateSnapshot {
            parse: parse_state.clone(),
            highlight: highlight_state.clone(),
        };
        let old_lines = std::mem::take(&mut self.highlight_cache.lines);
        let mut new_lines = Vec::with_capacity(old_lines.len().max(1));
        let mut job = LayoutJob {
            text: text.to_owned(),
            ..Default::default()
        };
        let default_format = TextFormat {
            font_id: editor_font.clone(),
            color: ui.visuals().text_color(),
            ..Default::default()
        };
        let mut line_start = 0usize;

        for (idx, line) in LinesWithEndings::from(text).enumerate() {
            let line_hash = hash_bytes(line.as_bytes());
            let start_state_matches = if idx == 0 {
                default_state.parse == parse_state && default_state.highlight == highlight_state
            } else {
                old_lines
                    .get(idx - 1)
                    .map(|line| {
                        line.end_state.parse == parse_state
                            && line.end_state.highlight == highlight_state
                    })
                    .unwrap_or(false)
            };
            if let Some(old_line) = old_lines.get(idx) {
                if old_line.hash == line_hash && start_state_matches {
                    append_sections(&mut job, &old_line.sections, line_start);
                    parse_state = old_line.end_state.parse.clone();
                    highlight_state = old_line.end_state.highlight.clone();
                    new_lines.push(old_line.clone());
                    line_start += line.len();
                    continue;
                }
            }

            match parse_state.parse_line(line, &settings.ps) {
                Ok(ops) => {
                    let mut sections = Vec::new();
                    let iter = syntect::highlighting::RangedHighlightIterator::new(
                        &mut highlight_state,
                        &ops[..],
                        line,
                        &highlighter,
                    );
                    for (style, _token, range) in iter {
                        if range.is_empty() {
                            continue;
                        }
                        sections.push(LayoutSection {
                            leading_space: 0.0,
                            byte_range: range,
                            format: syntect_style_to_format(style, editor_font),
                        });
                    }
                    if sections.is_empty() && !line.is_empty() {
                        sections.push(LayoutSection {
                            leading_space: 0.0,
                            byte_range: 0..line.len(),
                            format: default_format.clone(),
                        });
                    }

                    let end_state = HighlightStateSnapshot {
                        parse: parse_state.clone(),
                        highlight: highlight_state.clone(),
                    };
                    append_sections(&mut job, &sections, line_start);
                    new_lines.push(HighlightLineCache {
                        hash: line_hash,
                        sections,
                        end_state,
                    });
                }
                Err(_) => {
                    // Fallback to plain layout on parse errors.
                    job = LayoutJob::simple(
                        text.to_owned(),
                        editor_font.clone(),
                        ui.visuals().text_color(),
                        f32::INFINITY,
                    );
                    new_lines.clear();
                    break;
                }
            }

            line_start += line.len();
        }

        self.highlight_cache.lines = new_lines;
        job
    }
}

fn editor_buffer_revision(text: &dyn egui::TextBuffer) -> Option<u64> {
    if text.type_id() == TypeId::of::<EditorBuffer>() {
        let ptr = text as *const dyn egui::TextBuffer as *const EditorBuffer;
        // Safety: we only cast when the type id matches.
        let buffer = unsafe { &*ptr };
        Some(buffer.revision)
    } else {
        None
    }
}

struct SyntectSettings {
    ps: SyntaxSet,
    ts: ThemeSet,
}

impl Default for SyntectSettings {
    fn default() -> Self {
        Self {
            ps: SyntaxSet::load_defaults_newlines(),
            ts: ThemeSet::load_defaults(),
        }
    }
}

fn syntect_theme_key(theme: &CodeTheme) -> &'static str {
    if theme.is_dark() {
        "base16-mocha.dark"
    } else {
        "Solarized (light)"
    }
}

fn syntect_style_to_format(style: Style, editor_font: &FontId) -> TextFormat {
    let color = Color32::from_rgb(style.foreground.r, style.foreground.g, style.foreground.b);
    let italics = style
        .font_style
        .contains(syntect::highlighting::FontStyle::ITALIC);
    let underline = style
        .font_style
        .contains(syntect::highlighting::FontStyle::UNDERLINE);
    TextFormat {
        font_id: editor_font.clone(),
        color,
        italics,
        underline: if underline {
            Stroke::new(1.0, color)
        } else {
            Stroke::NONE
        },
        ..Default::default()
    }
}

fn append_sections(job: &mut LayoutJob, sections: &[LayoutSection], offset: usize) {
    for section in sections {
        let mut section = section.clone();
        section.byte_range = (section.byte_range.start + offset)..(section.byte_range.end + offset);
        job.sections.push(section);
    }
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x00000100000001B3;
    let mut hash = FNV_OFFSET;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

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

fn display_language_label(language: Option<&str>, is_large: bool) -> String {
    if is_large {
        return "plain".to_string();
    }
    let Some(raw) = language else {
        return "auto".to_string();
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "auto".to_string();
    }
    let lowered = trimmed.to_ascii_lowercase();
    match lowered.as_str() {
        "plaintext" | "plain" | "text" | "txt" => "plain".to_string(),
        _ => trimmed.to_string(),
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
            selected_content: EditorBuffer::new(String::new()),
            editor_cache: EditorLayoutCache::default(),
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

        ctx.input(|input| {
            if input.modifiers.command && input.key_pressed(egui::Key::N) {
                self.create_new_paste();
            }
            if input.modifiers.command && input.key_pressed(egui::Key::Delete) {
                self.delete_selected();
            }
        });

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
                        ui.set_min_size(egui::vec2(ui.available_width(), editor_height));
                        let editor_style = TextStyle::Name(EDITOR_TEXT_STYLE.into());
                        let editor_font = ui
                            .style()
                            .text_styles
                            .get(&editor_style)
                            .cloned()
                            .unwrap_or_else(|| TextStyle::Monospace.resolve(ui.style()));
                        let language_hint =
                            syntect_language_hint(language.as_deref().unwrap_or("text"));
                        let debounce_active = self
                            .last_edit_at
                            .map(|last| {
                                self.selected_content.len() >= HIGHLIGHT_DEBOUNCE_MIN_BYTES
                                    && last.elapsed() < HIGHLIGHT_DEBOUNCE
                            })
                            .unwrap_or(false);
                        let backoff_active = self
                            .editor_cache
                            .highlight_backoff_until
                            .map(|until| Instant::now() < until)
                            .unwrap_or(false);
                        let use_plain = is_large || debounce_active || backoff_active;
                        let theme =
                            (!use_plain).then(|| CodeTheme::from_memory(ui.ctx(), ui.style()));
                        let row_height = ui.text_style_height(&editor_style);
                        let rows_that_fit = ((editor_height / row_height).ceil() as usize).max(1);

                        let edit = egui::TextEdit::multiline(&mut self.selected_content)
                            .font(editor_style)
                            .desired_width(f32::INFINITY)
                            .desired_rows(rows_that_fit)
                            .lock_focus(true)
                            .hint_text("Start typing...");

                        let mut editor_cache = std::mem::take(&mut self.editor_cache);
                        let syntect = &self.syntect;
                        let mut layouter =
                            |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap_width: f32| {
                                editor_cache.layout(
                                    ui,
                                    text,
                                    wrap_width,
                                    language_hint.as_str(),
                                    use_plain,
                                    theme.as_ref(),
                                    &editor_font,
                                    syntect,
                                )
                            };
                        let edit = ui.add(edit.layouter(&mut layouter));
                        self.editor_cache = editor_cache;
                        if self.focus_editor_next || edit.clicked() {
                            edit.request_focus();
                            self.focus_editor_next = false;
                        }
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
                    &font,
                    &syntect,
                );

                assert_eq!(cache.last_highlight_ms, first_ms);
                assert_eq!(cache.highlight_cache.lines.len(), line_count);
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
                    &font,
                    &syntect,
                );
                let line_count = LinesWithEndings::from(buffer.as_str()).count();
                assert_eq!(cache.highlight_cache.lines.len(), line_count);
            });
        });
    }
}
