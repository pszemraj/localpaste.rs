use std::{
    cell::RefCell,
    collections::{hash_map::DefaultHasher, HashMap},
    fs,
    hash::{Hash, Hasher},
    net::SocketAddr,
    ops::Range,
    rc::Rc,
    sync::{mpsc, Arc},
    thread,
    time::{Duration, Instant},
};

use eframe::egui::{
    self, style::WidgetVisuals, CollapsingHeader, Color32, CornerRadius, FontFamily, FontId, Frame,
    Layout, Margin, Popup, RichText, Stroke, TextStyle, Visuals,
};
use egui_extras::syntax_highlighting::SyntectSettings;
use rfd::FileDialog;
use syntect::{
    easy::HighlightLines,
    highlighting::{FontStyle, HighlightState as SyntectHighlightState},
    parsing::{ParseState as SyntectParseState, SyntaxReference},
    util::LinesWithEndings,
};
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
const AUTO_DETECT_MIN_LINES: usize = 3;
const HIGHLIGHT_RECOMPUTE_DELAY: Duration = Duration::from_millis(75);
const HIGHLIGHT_CHUNK_SIZE: usize = 4 * 1024;
const HIGHLIGHT_PLAIN_THRESHOLD: usize = 256 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
struct SyntectState {
    highlight: SyntectHighlightState,
    parse: SyntectParseState,
}

impl SyntectState {
    fn initial(syntax: &SyntaxReference, theme: &syntect::highlighting::Theme) -> Self {
        let initial = HighlightLines::new(syntax, theme);
        let (highlight, parse) = initial.state();
        Self { highlight, parse }
    }

    fn to_highlighter<'a>(&self, theme: &'a syntect::highlighting::Theme) -> HighlightLines<'a> {
        HighlightLines::from_state(theme, self.highlight.clone(), self.parse.clone())
    }
}

#[derive(Clone)]
struct HighlightChunk {
    index: usize,
    version: u64,
    range: Range<usize>,
    text_hash: u64,
    state_before: SyntectState,
    state_after: SyntectState,
    layout_job: egui::text::LayoutJob,
}

#[derive(Clone)]
struct HighlightData {
    revision: u64,
    job: Arc<egui::text::LayoutJob>,
    chunks: Vec<HighlightChunk>,
    line_offsets: Vec<usize>,
    plain: bool,
}

struct ChunkBuildInput<'a> {
    settings: &'a SyntectSettings,
    syntect_theme: &'a syntect::highlighting::Theme,
    highlight_theme: &'a HighlightTheme,
    index: usize,
    range: Range<usize>,
    text: &'a str,
    text_hash: u64,
    state_before: SyntectState,
}

#[derive(Clone)]
struct HighlightTheme {
    font_id: egui::FontId,
    syntect_theme_key: &'static str,
    dark_mode: bool,
}

impl HighlightTheme {
    fn from_style(style: &egui::Style) -> Self {
        let font_id = style
            .override_font_id
            .clone()
            .unwrap_or_else(|| TextStyle::Monospace.resolve(style));
        let dark_mode = style.visuals.dark_mode;
        let syntect_theme_key = if dark_mode {
            "base16-mocha.dark"
        } else {
            "Solarized (light)"
        };
        Self {
            font_id,
            syntect_theme_key,
            dark_mode,
        }
    }

    fn font_id(&self) -> &egui::FontId {
        &self.font_id
    }

    fn syntect_theme_key(&self) -> &'static str {
        self.syntect_theme_key
    }

    fn is_dark(&self) -> bool {
        self.dark_mode
    }
}

struct HighlightCache {
    settings: Arc<SyntectSettings>,
    chunk_size: usize,
    revision: u64,
    next_chunk_version: u64,
    data: Option<Rc<HighlightData>>,
}

impl Default for HighlightCache {
    fn default() -> Self {
        Self {
            settings: Arc::new(SyntectSettings::default()),
            chunk_size: HIGHLIGHT_CHUNK_SIZE,
            revision: 0,
            next_chunk_version: 0,
            data: None,
        }
    }
}

impl HighlightCache {
    fn clear(&mut self) {
        self.data = None;
        self.next_chunk_version = 0;
    }

    fn current(&self) -> Option<Rc<HighlightData>> {
        self.data.clone()
    }

    fn recompute(
        &mut self,
        theme: &HighlightTheme,
        language: &str,
        text: &str,
    ) -> Rc<HighlightData> {
        let settings = Arc::clone(&self.settings);
        let syntax = settings
            .ps
            .find_syntax_by_name(language)
            .or_else(|| settings.ps.find_syntax_by_extension(language))
            .unwrap_or_else(|| settings.ps.find_syntax_plain_text());
        let syntect_theme = settings
            .ts
            .themes
            .get(theme.syntect_theme_key())
            .cloned()
            .unwrap_or_else(|| {
                settings
                    .ts
                    .themes
                    .values()
                    .next()
                    .cloned()
                    .expect("syntect theme set must contain at least one entry")
            });
        let line_offsets = compute_line_offsets(text);
        let chunk_ranges = compute_chunk_ranges(text, self.chunk_size);
        let mut current_state = SyntectState::initial(syntax, &syntect_theme);

        let previous = self.data.clone();
        let mut aggregates = Vec::new();
        let mut chunks = Vec::with_capacity(chunk_ranges.len());

        for (index, range) in chunk_ranges.into_iter().enumerate() {
            let chunk_text = &text[range.clone()];
            let chunk_hash = hash_str(chunk_text);

            let reused = previous
                .as_ref()
                .and_then(|data| data.chunks.get(index))
                .filter(|chunk| {
                    chunk.text_hash == chunk_hash && chunk.state_before == current_state
                })
                .cloned();

            let mut chunk = if let Some(mut existing) = reused {
                existing.range = range.clone();
                existing.state_before = current_state.clone();
                existing
            } else {
                self.build_chunk(ChunkBuildInput {
                    settings: settings.as_ref(),
                    syntect_theme: &syntect_theme,
                    highlight_theme: theme,
                    index,
                    range: range.clone(),
                    text: chunk_text,
                    text_hash: chunk_hash,
                    state_before: current_state.clone(),
                })
            };

            current_state = chunk.state_after.clone();

            aggregates.extend(
                chunk
                    .layout_job
                    .sections
                    .iter()
                    .cloned()
                    .map(|mut section| {
                        section.byte_range.start += chunk.range.start;
                        section.byte_range.end += chunk.range.start;
                        section
                    }),
            );

            chunk.index = index;
            chunk.range = range;
            chunk.text_hash = chunk_hash;
            chunks.push(chunk);
        }

        let mut job = egui::text::LayoutJob {
            text: text.to_owned(),
            sections: aggregates,
            ..Default::default()
        };
        job.wrap.max_width = f32::INFINITY;

        self.revision = self.revision.wrapping_add(1);
        let data = HighlightData {
            revision: self.revision,
            job: Arc::new(job),
            chunks,
            line_offsets,
            plain: false,
        };
        let rc = Rc::new(data);
        self.data = Some(rc.clone());
        rc
    }

    fn plain_text(&mut self, theme: &HighlightTheme, text: &str) -> Rc<HighlightData> {
        let settings = Arc::clone(&self.settings);
        let syntect_theme = settings
            .ts
            .themes
            .get(theme.syntect_theme_key())
            .cloned()
            .unwrap_or_else(|| {
                settings
                    .ts
                    .themes
                    .values()
                    .next()
                    .cloned()
                    .expect("syntect theme set must contain at least one entry")
            });
        let syntax = settings.ps.find_syntax_plain_text();
        let state = SyntectState::initial(syntax, &syntect_theme);
        let color = if theme.is_dark() {
            Color32::LIGHT_GRAY
        } else {
            Color32::DARK_GRAY
        };
        let mut layout_job = egui::text::LayoutJob::simple(
            text.to_owned(),
            theme.font_id().clone(),
            color,
            f32::INFINITY,
        );
        layout_job.wrap.max_width = f32::INFINITY;
        self.next_chunk_version = self.next_chunk_version.wrapping_add(1);
        let chunk = HighlightChunk {
            index: 0,
            version: self.next_chunk_version,
            range: 0..text.len(),
            text_hash: hash_str(text),
            state_before: state.clone(),
            state_after: state,
            layout_job: layout_job.clone(),
        };
        self.revision = self.revision.wrapping_add(1);
        let data = HighlightData {
            revision: self.revision,
            job: Arc::new(layout_job),
            chunks: vec![chunk],
            line_offsets: compute_line_offsets(text),
            plain: true,
        };
        let rc = Rc::new(data);
        self.data = Some(rc.clone());
        rc
    }
    fn build_chunk(&mut self, input: ChunkBuildInput<'_>) -> HighlightChunk {
        let mut highlighter = input.state_before.to_highlighter(input.syntect_theme);
        let mut sections = Vec::new();
        let mut failed = false;

        for line in LinesWithEndings::from(input.text) {
            match highlighter.highlight_line(line, &input.settings.ps) {
                Ok(spans) => {
                    for (style, span) in spans {
                        if span.is_empty() {
                            continue;
                        }
                        sections.push(layout_section_from_style(
                            input.highlight_theme,
                            input.text,
                            span,
                            &style,
                        ));
                    }
                }
                Err(_) => {
                    failed = true;
                    break;
                }
            }
        }

        let (highlight_state, parse_state) = highlighter.state();
        let state_after = SyntectState {
            highlight: highlight_state,
            parse: parse_state,
        };

        let layout_job = if failed {
            egui::text::LayoutJob::simple(
                input.text.to_owned(),
                input.highlight_theme.font_id().clone(),
                if input.highlight_theme.is_dark() {
                    Color32::LIGHT_GRAY
                } else {
                    Color32::DARK_GRAY
                },
                f32::INFINITY,
            )
        } else {
            egui::text::LayoutJob {
                text: input.text.to_owned(),
                sections,
                wrap: egui::text::TextWrapping {
                    max_width: f32::INFINITY,
                    ..Default::default()
                },
                ..Default::default()
            }
        };

        self.next_chunk_version = self.next_chunk_version.wrapping_add(1);
        HighlightChunk {
            index: input.index,
            version: self.next_chunk_version,
            range: input.range,
            text_hash: input.text_hash,
            state_before: input.state_before,
            state_after,
            layout_job,
        }
    }
}

fn layout_section_from_style(
    theme: &HighlightTheme,
    chunk_text: &str,
    span: &str,
    style: &syntect::highlighting::Style,
) -> egui::text::LayoutSection {
    let fg = style.foreground;
    let color = Color32::from_rgb(fg.r, fg.g, fg.b);
    let italics = style.font_style.contains(FontStyle::ITALIC);
    let underline = style.font_style.contains(FontStyle::ITALIC);
    egui::text::LayoutSection {
        leading_space: 0.0,
        byte_range: byte_range_in(chunk_text, span),
        format: egui::text::TextFormat {
            font_id: theme.font_id().clone(),
            color,
            italics,
            underline: if underline {
                Stroke::new(1.0, color)
            } else {
                Stroke::NONE
            },
            ..Default::default()
        },
    }
}

fn compute_chunk_ranges(text: &str, chunk_size: usize) -> Vec<Range<usize>> {
    if text.is_empty() {
        return Vec::new();
    }

    let total = text.len();
    let mut ranges = Vec::new();
    let mut start = 0;

    while start < total {
        let mut end = (start + chunk_size).min(total);

        if end < total {
            if let Some(last_newline) = text[start..end].rfind('\n') {
                end = start + last_newline + 1;
            } else if let Some(next_newline) = text[end..].find('\n') {
                end += next_newline + 1;
            } else {
                end = total;
            }
        }

        if end == start {
            end = (start + chunk_size).min(total);
            if end == start {
                end = total;
            }
        }

        ranges.push(start..end);
        start = end;
    }

    ranges
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

fn byte_range_in(whole: &str, part: &str) -> Range<usize> {
    let start = part.as_ptr() as usize - whole.as_ptr() as usize;
    start..start + part.len()
}

fn hash_str(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
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

#[derive(Clone, Default)]
struct LayoutCache {
    inner: Rc<RefCell<LayoutCacheInner>>,
}

impl LayoutCache {
    fn reset(&self) {
        self.inner.borrow_mut().reset();
    }

    fn layout(
        &self,
        ui: &egui::Ui,
        wrap_width: f32,
        highlight: &HighlightData,
    ) -> Arc<egui::text::Galley> {
        let pixels_per_point = ui.ctx().pixels_per_point();
        let key = LayoutCacheKey::new(wrap_width, pixels_per_point);
        let mut inner = self.inner.borrow_mut();

        inner.sync_chunks(highlight);
        inner
            .combined
            .retain(|_, cached| cached.revision == highlight.revision);

        if let Some(galley) = inner
            .combined
            .get(&key)
            .filter(|cached| cached.revision == highlight.revision)
            .map(|cached| cached.galley.clone())
        {
            return galley;
        }

        let chunk_galleys = highlight
            .chunks
            .iter()
            .enumerate()
            .map(|(idx, chunk)| inner.chunk_layout(ui, idx, chunk, key).galley)
            .collect::<Vec<_>>();

        let mut job = (*highlight.job).clone();
        job.wrap.max_width = if wrap_width.is_finite() {
            wrap_width
        } else {
            f32::INFINITY
        };
        let job = Arc::new(job);
        let galley = Arc::new(egui::text::Galley::concat(
            job,
            &chunk_galleys,
            pixels_per_point,
        ));
        inner.combined.insert(
            key,
            CachedCombinedGalley {
                revision: highlight.revision,
                galley: galley.clone(),
            },
        );
        galley
    }
}

#[derive(Clone)]
struct CachedChunkLayout {
    galley: Arc<egui::text::Galley>,
    height: f32,
}

#[derive(Default)]
struct LayoutCacheInner {
    last_chunk_count: usize,
    chunks: Vec<ChunkLayoutEntry>,
    combined: HashMap<LayoutCacheKey, CachedCombinedGalley>,
}

impl LayoutCacheInner {
    fn reset(&mut self) {
        self.last_chunk_count = 0;
        self.chunks.clear();
        self.combined.clear();
    }

    fn sync_chunks(&mut self, highlight: &HighlightData) {
        if self.last_chunk_count != highlight.chunks.len() {
            self.chunks
                .resize_with(highlight.chunks.len(), ChunkLayoutEntry::default);
            self.last_chunk_count = highlight.chunks.len();
        }

        for (entry, chunk) in self.chunks.iter_mut().zip(highlight.chunks.iter()) {
            if entry.version != chunk.version {
                entry.version = chunk.version;
                entry.layouts.clear();
            }
        }
    }

    fn chunk_layout(
        &mut self,
        ui: &egui::Ui,
        index: usize,
        chunk: &HighlightChunk,
        key: LayoutCacheKey,
    ) -> CachedChunkLayout {
        let entry = &mut self.chunks[index];
        if let Some(layout) = entry.layouts.get(&key) {
            return layout.clone();
        }

        let mut job = chunk.layout_job.clone();
        job.wrap.max_width = if key.wrap_width().is_finite() {
            key.wrap_width()
        } else {
            f32::INFINITY
        };
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        let layout = CachedChunkLayout {
            galley: galley.clone(),
            height: galley.rect.height(),
        };
        entry.layouts.insert(key, layout.clone());
        layout
    }
}

#[derive(Default)]
struct ChunkLayoutEntry {
    version: u64,
    layouts: HashMap<LayoutCacheKey, CachedChunkLayout>,
}

#[derive(Clone)]
struct CachedCombinedGalley {
    revision: u64,
    galley: Arc<egui::text::Galley>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct LayoutCacheKey {
    wrap_bits: u32,
    pixels_per_point_bits: u32,
}

impl LayoutCacheKey {
    fn new(wrap_width: f32, pixels_per_point: f32) -> Self {
        let wrap = if wrap_width.is_finite() {
            wrap_width
        } else {
            f32::INFINITY
        };
        let ppp = if pixels_per_point.is_finite() {
            pixels_per_point
        } else {
            1.0
        };
        Self {
            wrap_bits: wrap.to_bits(),
            pixels_per_point_bits: ppp.to_bits(),
        }
    }

    fn wrap_width(self) -> f32 {
        f32::from_bits(self.wrap_bits)
    }
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
    highlight_theme: HighlightTheme,
    virtual_preview_enabled: bool,
    style_applied: bool,
    folder_dialog: Option<FolderDialog>,
    _server: ServerHandle,
    profile_highlight: bool,
    editor_focused: bool,
    auto_save_blocked: bool,
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

        let profile_highlight = std::env::var("LOCALPASTE_PROFILE_HIGHLIGHT")
            .map(|v| v != "0")
            .unwrap_or(false);
        let virtual_preview_enabled = std::env::var("LOCALPASTE_VIRTUAL_PREVIEW")
            .map(|v| v != "0")
            .unwrap_or(false);

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
            highlight_theme: HighlightTheme::from_style(&egui::Style::default()),
            virtual_preview_enabled,
            style_applied: false,
            folder_dialog: None,
            _server: server,
            profile_highlight,
            editor_focused: false,
            auto_save_blocked: false,
        };

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
        self.highlight_theme = HighlightTheme::from_style(&style);
        self.style_applied = true;
    }

    fn reload_pastes(&mut self, reason: &str) {
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
        self.editor.auto_detect_cache = None;
        self.editor.request_highlight_update();
        self.editor.highlight_pending_since = Some(Instant::now());
        self.auto_save_blocked = false;
    }

    fn ensure_highlight_data(
        &mut self,
        theme: &HighlightTheme,
        language: &str,
    ) -> Rc<HighlightData> {
        let now = Instant::now();
        let content_len = self.editor.content.len();

        if content_len >= HIGHLIGHT_PLAIN_THRESHOLD {
            if !self.editor.plain_highlight_mode {
                self.editor.plain_highlight_mode = true;
                self.editor.highlight_cache.clear();
            }
            if let Some(existing) = self
                .editor
                .highlight_cache
                .current()
                .filter(|data| data.plain)
            {
                // Plain-mode needs manual invalidation from mark_editor_dirty.
                if self.editor.highlight_pending_since.is_none() {
                    self.editor.line_offsets = existing.line_offsets.clone();
                    return existing;
                }
            }
            let data = self
                .editor
                .highlight_cache
                .plain_text(theme, self.editor.content.as_str());
            self.editor.layout_cache.reset();
            self.editor.highlight_pending_since = None;
            self.editor.highlight_last_recompute = Some(now);
            self.editor.line_offsets = data.line_offsets.clone();
            return data;
        }

        if self.editor.plain_highlight_mode {
            self.editor.plain_highlight_mode = false;
            self.editor.highlight_cache.clear();
        }

        let mut should_recompute = self.editor.highlight_cache.current().is_none();

        if !should_recompute {
            if let Some(pending_since) = self.editor.highlight_pending_since {
                if now.duration_since(pending_since) >= HIGHLIGHT_RECOMPUTE_DELAY {
                    should_recompute = true;
                }
            }
        }

        if should_recompute {
            let started = self
                .profile_highlight
                .then_some((Instant::now(), content_len));
            let data = self.editor.highlight_cache.recompute(
                theme,
                language,
                self.editor.content.as_str(),
            );
            self.editor.layout_cache.reset();
            if let Some((began, chars)) = started {
                let elapsed = began.elapsed();
                debug!(
                    "highlight_job duration_ms={:.3} chars={} lang={} paste_id={} chunks={}",
                    elapsed.as_secs_f64() * 1_000.0,
                    chars,
                    language,
                    self.editor.paste_id.as_deref().unwrap_or("unsaved"),
                    data.chunks.len(),
                );
            }
            self.editor.highlight_pending_since = None;
            self.editor.highlight_last_recompute = Some(now);
            self.editor.line_offsets = data.line_offsets.clone();
            data
        } else {
            let data = self
                .editor
                .highlight_cache
                .current()
                .expect("highlight data available when clean");
            self.editor.line_offsets = data.line_offsets.clone();
            data
        }
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

    fn render_virtual_preview(
        &self,
        ui: &mut egui::Ui,
        highlight_data: Rc<HighlightData>,
        layout_cache: LayoutCache,
    ) {
        egui::ScrollArea::vertical()
            .id_salt("virtual_preview_scroll")
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                let available_width = ui.available_width();
                let pixels_per_point = ui.ctx().pixels_per_point();
                let key = LayoutCacheKey::new(available_width, pixels_per_point);

                let mut inner = layout_cache.inner.borrow_mut();
                inner.sync_chunks(highlight_data.as_ref());

                let mut heights = Vec::with_capacity(highlight_data.chunks.len());
                let mut total_height = 0.0f32;

                for (idx, chunk) in highlight_data.chunks.iter().enumerate() {
                    let layout = inner.chunk_layout(ui, idx, chunk, key);
                    heights.push(layout.height);
                    total_height += layout.height;
                }

                let total_height = total_height.max(1.0);
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(available_width, total_height),
                    egui::Sense::hover(),
                );
                let clip = ui.clip_rect();
                let painter = ui.painter_at(rect);

                let mut top_offset = 0.0f32;
                for (idx, chunk) in highlight_data.chunks.iter().enumerate() {
                    let height = heights[idx];
                    let abs_top = rect.min.y + top_offset;
                    let abs_bottom = abs_top + height;

                    if abs_bottom >= clip.min.y && abs_top <= clip.max.y {
                        let layout = inner.chunk_layout(ui, idx, chunk, key);
                        painter.galley(
                            egui::pos2(rect.min.x, abs_top),
                            layout.galley.clone(),
                            Color32::WHITE,
                        );

                        if self.profile_highlight {
                            let label = format!("chunk {} • {:.1}px", idx, height);
                            painter.text(
                                egui::pos2(rect.min.x + 8.0, abs_top + 4.0),
                                egui::Align2::LEFT_TOP,
                                label,
                                FontId::new(10.0, FontFamily::Monospace),
                                COLOR_TEXT_MUTED,
                            );
                        }

                        if idx + 1 != highlight_data.chunks.len() {
                            painter.line_segment(
                                [
                                    egui::pos2(rect.min.x, abs_bottom),
                                    egui::pos2(rect.max.x, abs_bottom),
                                ],
                                Stroke::new(0.5, COLOR_BORDER),
                            );
                        }
                    }

                    top_offset += height;
                }
            });
    }

    fn ensure_language_selection(&mut self) {
        if self.editor.manual_language_override {
            return;
        }

        let trimmed = self.editor.content.trim();
        let char_count = trimmed.chars().count();
        let line_count = trimmed.lines().count();

        if char_count < AUTO_DETECT_MIN_CHARS && line_count < AUTO_DETECT_MIN_LINES {
            if self.editor.language.is_some() {
                self.editor.language = None;
                self.editor.auto_detect_cache = None;
                self.editor.reset_highlight_state();
            }
            return;
        }

        let mut hasher = DefaultHasher::new();
        self.editor.content.hash(&mut hasher);
        let content_hash = hasher.finish();

        if let Some((cached_hash, cached_lang)) = &self.editor.auto_detect_cache {
            if *cached_hash == content_hash {
                if self.editor.language.as_deref() != Some(cached_lang) {
                    self.editor.language = Some(cached_lang.clone());
                    self.editor.reset_highlight_state();
                }
                return;
            }
        }

        let detected = crate::models::paste::detect_language(&self.editor.content)
            .unwrap_or_else(|| "plain".to_string());

        self.editor.auto_detect_cache = Some((content_hash, detected.clone()));
        self.editor.manual_language_override = false;

        if self.editor.language.as_deref() != Some(detected.as_str()) {
            self.editor.language = Some(detected);
            self.editor.reset_highlight_state();
        }
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

        let mut thread_handle = Some(thread);

        match ready_rx.recv() {
            Ok(Ok(addr)) => {
                if !addr.ip().is_loopback() {
                    warn!("binding to non-localhost address {}", addr);
                }
                info!("API listening on http://{}", addr);
                Ok(Self {
                    shutdown: Some(shutdown_tx),
                    thread: thread_handle.take(),
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
        self.ensure_style(ctx);
        self.ensure_language_selection();
        self.editor_focused = false;

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
            if input.modifiers.command && input.key_pressed(egui::Key::F) && !self.editor_focused {
                self.focus_filter();
            }
            if input.modifiers.command && input.key_pressed(egui::Key::K) && !self.editor_focused {
                self.focus_filter();
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
                                if self.editor.plain_highlight_mode {
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
                            egui::ComboBox::from_id_salt("language_select")
                                .selected_text(current_language_label)
                                .show_ui(ui, |ui| {
                                    ui.set_min_width(160.0);
                                    let popup_open = Popup::is_any_open(ui.ctx());
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
                                                self.editor.manual_language_override = true;
                                                self.editor.reset_highlight_state();
                                                self.mark_editor_dirty();
                                            } else {
                                                self.editor.manual_language_override = true;
                                            }
                                            auto_scroll_target = Some(option.id);
                                        }
                                    }

                                    if ui
                                        .selectable_value(&mut self.editor.language, None, "Auto")
                                        .clicked()
                                    {
                                        self.editor.manual_language_override = false;
                                        self.editor.reset_highlight_state();
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
                                            self.editor.manual_language_override = true;
                                            self.editor.reset_highlight_state();
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
                    let highlight_theme = self.highlight_theme.clone();
                    let highlight_data =
                        self.ensure_highlight_data(&highlight_theme, syntax_token.as_str());
                    let layout_cache = self.editor.layout_cache.clone();
                    let highlight_for_layout = highlight_data.clone();
                    let layout_cache_for_layout = layout_cache.clone();
                    egui::ScrollArea::vertical()
                        .id_salt("editor_scroll")
                        .auto_shrink([false; 2])
                        .show(ui, |ui| {
                            let mut layouter =
                                move |ui: &egui::Ui,
                                      _text: &dyn egui::TextBuffer,
                                      wrap_width: f32| {
                                    layout_cache_for_layout.layout(
                                        ui,
                                        wrap_width,
                                        highlight_for_layout.as_ref(),
                                    )
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
                                debug!(
                                    "text_edit_layout duration_ms={:.3} chars={} chunks={}",
                                    elapsed.as_secs_f64() * 1_000.0,
                                    self.editor.content.len(),
                                    highlight_data.chunks.len()
                                );
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

                    if self.virtual_preview_enabled {
                        ui.add_space(16.0);
                        ui.label(
                            RichText::new("Virtualized Preview (read-only)")
                                .color(COLOR_TEXT_MUTED)
                                .size(12.0),
                        );
                        ui.add_space(4.0);
                        self.render_virtual_preview(ui, highlight_data, layout_cache);
                    }
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
    auto_detect_cache: Option<(u64, String)>,
    manual_language_override: bool,
    highlight_cache: HighlightCache,
    layout_cache: LayoutCache,
    highlight_pending_since: Option<Instant>,
    highlight_last_recompute: Option<Instant>,
    line_offsets: Vec<usize>,
    plain_highlight_mode: bool,
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
        self.manual_language_override = self.language.is_some();
        self.mark_pristine();
        self.needs_focus = true;
        self.auto_detect_cache = None;
        self.plain_highlight_mode = false;
        self.reset_highlight_state();
        self.line_offsets = compute_line_offsets(&self.content);
    }

    fn sync_after_save(&mut self, paste: &Paste) {
        self.paste_id = Some(paste.id.clone());
        self.name = paste.name.clone();
        self.folder_id = paste.folder_id.clone();
        self.tags = paste.tags.clone();
        self.language = paste.language.clone();
        self.manual_language_override = self.language.is_some();
        self.auto_detect_cache = None;
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

    fn request_highlight_update(&mut self) {
        self.highlight_pending_since = Some(Instant::now());
    }

    fn reset_highlight_state(&mut self) {
        self.highlight_cache.clear();
        self.layout_cache.reset();
        self.highlight_pending_since = Some(Instant::now());
        self.highlight_last_recompute = None;
        self.line_offsets.clear();
        self.line_offsets.push(0);
        self.plain_highlight_mode = false;
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
            auto_detect_cache: None,
            manual_language_override: false,
            highlight_cache: HighlightCache::default(),
            layout_cache: LayoutCache::default(),
            highlight_pending_since: Some(Instant::now()),
            highlight_last_recompute: None,
            line_offsets: vec![0],
            plain_highlight_mode: false,
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
