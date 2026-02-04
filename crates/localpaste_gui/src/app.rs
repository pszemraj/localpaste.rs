//! Native egui app skeleton for the LocalPaste rewrite.

use crate::backend::{spawn_backend, BackendHandle, CoreCmd, CoreEvent, PasteSummary};
use eframe::egui::{
    self,
    style::WidgetVisuals,
    text::{CCursor, CCursorRange, LayoutJob, LayoutSection, TextFormat},
    text_edit::TextEditOutput,
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, RichText, Stroke,
    TextStyle, Visuals,
};
use egui_extras::syntax_highlighting::CodeTheme;
use localpaste_core::models::paste::Paste;
use localpaste_core::{Config, Database};
use localpaste_server::{AppState, EmbeddedServer, PasteLockManager};
use std::any::TypeId;
use std::net::SocketAddr;
use std::ops::Range;
use std::sync::Arc;
use std::thread;
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
    editor_lines: EditorLineIndex,
    editor_mode: EditorMode,
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

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum EditorMode {
    TextEdit,
    VirtualPreview,
}

impl EditorMode {
    fn from_env() -> Self {
        match std::env::var("LOCALPASTE_VIRTUAL_PREVIEW") {
            Ok(value) => {
                let lowered = value.trim().to_ascii_lowercase();
                if lowered.is_empty() || lowered == "0" || lowered == "false" {
                    Self::TextEdit
                } else {
                    Self::VirtualPreview
                }
            }
            Err(_) => Self::TextEdit,
        }
    }
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

#[derive(Default)]
struct EditorBuffer {
    text: String,
    revision: u64,
    char_len: usize,
}

impl EditorBuffer {
    fn new(text: String) -> Self {
        let char_len = text.chars().count();
        Self {
            text,
            revision: 0,
            char_len,
        }
    }

    fn reset(&mut self, text: String) {
        self.text = text;
        self.revision = 0;
        self.char_len = self.text.chars().count();
    }

    fn len(&self) -> usize {
        self.text.len()
    }

    fn revision(&self) -> u64 {
        self.revision
    }

    fn chars_len(&self) -> usize {
        self.char_len
    }

    fn as_str(&self) -> &str {
        self.text.as_str()
    }

    fn to_string(&self) -> String {
        self.text.clone()
    }
}

#[derive(Default)]
struct EditorLineIndex {
    revision: u64,
    text_len: usize,
    lines: Vec<LineEntry>,
}

#[derive(Clone, Copy)]
struct LineEntry {
    start: usize,
    len: usize,
}

impl EditorLineIndex {
    fn reset(&mut self) {
        self.revision = 0;
        self.text_len = 0;
        self.lines.clear();
    }

    fn ensure_for(&mut self, revision: u64, text: &str) {
        if !self.lines.is_empty() && self.revision == revision && self.text_len == text.len() {
            return;
        }
        self.rebuild(revision, text);
    }

    fn rebuild(&mut self, revision: u64, text: &str) {
        self.lines.clear();
        let mut start = 0usize;
        for (idx, byte) in text.as_bytes().iter().enumerate() {
            if *byte == b'\n' {
                let len = idx + 1 - start;
                self.lines.push(LineEntry { start, len });
                start = idx + 1;
            }
        }
        if start <= text.len() {
            self.lines.push(LineEntry {
                start,
                len: text.len().saturating_sub(start),
            });
        }
        if self.lines.is_empty() {
            self.lines.push(LineEntry { start: 0, len: 0 });
        }
        self.revision = revision;
        self.text_len = text.len();
    }

    fn line_count(&self) -> usize {
        self.lines.len().max(1)
    }

    fn line_slice<'a>(&self, text: &'a str, index: usize) -> &'a str {
        let Some(line) = self.lines.get(index) else {
            return "";
        };
        let end = line.start.saturating_add(line.len).min(text.len());
        &text[line.start..end]
    }

    fn line_without_newline<'a>(&self, text: &'a str, index: usize) -> &'a str {
        let mut line = self.line_slice(text, index);
        if let Some(trimmed) = line.strip_suffix('\n') {
            line = trimmed;
        }
        if let Some(trimmed) = line.strip_suffix('\r') {
            line = trimmed;
        }
        line
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
            self.char_len = self.char_len.saturating_add(inserted);
        }
        inserted
    }

    fn delete_char_range(&mut self, char_range: std::ops::Range<usize>) {
        if char_range.start == char_range.end {
            return;
        }
        let removed = char_range.end.saturating_sub(char_range.start);
        <String as egui::TextBuffer>::delete_char_range(&mut self.text, char_range);
        self.revision = self.revision.wrapping_add(1);
        self.char_len = self.char_len.saturating_sub(removed);
    }

    fn clear(&mut self) {
        if self.text.is_empty() {
            return;
        }
        self.text.clear();
        self.revision = self.revision.wrapping_add(1);
        self.char_len = 0;
    }

    fn replace_with(&mut self, text: &str) {
        if self.text == text {
            return;
        }
        self.text.clear();
        self.text.push_str(text);
        self.revision = self.revision.wrapping_add(1);
        self.char_len = text.chars().count();
    }

    fn take(&mut self) -> String {
        self.revision = self.revision.wrapping_add(1);
        self.char_len = 0;
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
    highlight_version: u64,
    galley: Option<Arc<egui::Galley>>,
    highlight_cache: HighlightCache,
    last_highlight_ms: Option<f32>,
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
        highlight_render: Option<&HighlightRender>,
        highlight_version: u64,
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
                highlight_render,
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
            && self.theme == theme_value
            && self.highlight_version == highlight_version;

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
            highlight_render,
            editor_font,
            syntect,
        );
        if !use_plain {
            let elapsed_ms = started.elapsed().as_secs_f32() * 1000.0;
            self.last_highlight_ms = Some(elapsed_ms);
        }

        self.revision = revision;
        self.use_plain = use_plain;
        self.wrap_width = wrap_width;
        self.pixels_per_point = pixels_per_point;
        self.language_hint = language_hint.to_string();
        self.font_id = Some(editor_font.clone());
        self.theme = theme_value;
        self.highlight_version = highlight_version;
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
        highlight_render: Option<&HighlightRender>,
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
        } else if let Some(render) = highlight_render {
            self.build_render_job(ui, text, render, editor_font)
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

    fn build_render_job(
        &self,
        ui: &egui::Ui,
        text: &str,
        render: &HighlightRender,
        editor_font: &FontId,
    ) -> LayoutJob {
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

        for line in &render.lines {
            if line_start >= text.len() {
                break;
            }
            if line.spans.is_empty() && line.len > 0 {
                job.sections.push(LayoutSection {
                    leading_space: 0.0,
                    byte_range: line_start..(line_start + line.len).min(text.len()),
                    format: default_format.clone(),
                });
            } else {
                for span in &line.spans {
                    let start = line_start.saturating_add(span.range.start);
                    let end = line_start.saturating_add(span.range.end);
                    if start >= text.len() || end > text.len() || start >= end {
                        continue;
                    }
                    job.sections.push(LayoutSection {
                        leading_space: 0.0,
                        byte_range: start..end,
                        format: render_span_to_format(span, editor_font),
                    });
                }
            }
            line_start = line_start.saturating_add(line.len);
        }
        if line_start < text.len() {
            job.sections.push(LayoutSection {
                leading_space: 0.0,
                byte_range: line_start..text.len(),
                format: default_format,
            });
        }

        job
    }
}

fn build_virtual_line_job(
    ui: &egui::Ui,
    line: &str,
    editor_font: &FontId,
    render_line: Option<&HighlightRenderLine>,
    use_plain: bool,
) -> LayoutJob {
    if use_plain || render_line.is_none() {
        return LayoutJob::simple(
            line.to_owned(),
            editor_font.clone(),
            ui.visuals().text_color(),
            f32::INFINITY,
        );
    }

    let render_line = render_line.expect("render line");
    let mut job = LayoutJob {
        text: line.to_owned(),
        ..Default::default()
    };
    let default_format = TextFormat {
        font_id: editor_font.clone(),
        color: ui.visuals().text_color(),
        ..Default::default()
    };

    if render_line.spans.is_empty() && !line.is_empty() {
        job.sections.push(LayoutSection {
            leading_space: 0.0,
            byte_range: 0..line.len(),
            format: default_format,
        });
    } else {
        for span in &render_line.spans {
            let start = span.range.start.min(line.len());
            let end = span.range.end.min(line.len());
            if start >= end {
                continue;
            }
            job.sections.push(LayoutSection {
                leading_space: 0.0,
                byte_range: start..end,
                format: render_span_to_format(span, editor_font),
            });
        }
    }

    job.wrap.max_width = f32::INFINITY;
    job
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

fn render_span_to_format(span: &HighlightSpan, editor_font: &FontId) -> TextFormat {
    let color = Color32::from_rgba_unmultiplied(
        span.style.color[0],
        span.style.color[1],
        span.style.color[2],
        span.style.color[3],
    );
    TextFormat {
        font_id: editor_font.clone(),
        color,
        italics: span.style.italics,
        underline: if span.style.underline {
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

#[derive(Clone)]
struct HighlightSpan {
    range: Range<usize>,
    style: HighlightStyle,
}

#[derive(Clone, Copy)]
struct HighlightStyle {
    color: [u8; 4],
    italics: bool,
    underline: bool,
}

#[derive(Clone)]
struct HighlightRenderLine {
    len: usize,
    spans: Vec<HighlightSpan>,
}

#[derive(Clone)]
struct HighlightRender {
    paste_id: String,
    revision: u64,
    text_len: usize,
    language_hint: String,
    theme_key: String,
    lines: Vec<HighlightRenderLine>,
}

impl HighlightRender {
    fn matches_context(&self, paste_id: &str, language_hint: &str, theme_key: &str) -> bool {
        self.paste_id == paste_id
            && self.language_hint == language_hint
            && self.theme_key == theme_key
    }

    fn matches_exact(
        &self,
        revision: u64,
        text_len: usize,
        language_hint: &str,
        theme_key: &str,
        paste_id: &str,
    ) -> bool {
        self.revision == revision
            && self.text_len == text_len
            && self.matches_context(paste_id, language_hint, theme_key)
    }
}

#[derive(Clone)]
struct HighlightRequest {
    paste_id: String,
    revision: u64,
    text: String,
    language_hint: String,
    theme_key: String,
}

struct HighlightRequestMeta {
    paste_id: String,
    revision: u64,
    text_len: usize,
    language_hint: String,
    theme_key: String,
}

impl HighlightRequestMeta {
    fn matches(
        &self,
        revision: u64,
        text_len: usize,
        language_hint: &str,
        theme_key: &str,
        paste_id: &str,
    ) -> bool {
        self.revision == revision
            && self.text_len == text_len
            && self.language_hint == language_hint
            && self.theme_key == theme_key
            && self.paste_id == paste_id
    }

    fn matches_render(&self, render: &HighlightRender) -> bool {
        self.revision == render.revision
            && self.text_len == render.text_len
            && self.language_hint == render.language_hint
            && self.theme_key == render.theme_key
            && self.paste_id == render.paste_id
    }
}

struct HighlightWorker {
    tx: crossbeam_channel::Sender<HighlightRequest>,
    rx: crossbeam_channel::Receiver<HighlightRender>,
}

#[derive(Default)]
struct HighlightWorkerCache {
    language_hint: String,
    theme_key: String,
    lines: Vec<HighlightWorkerLine>,
}

#[derive(Clone)]
struct HighlightWorkerLine {
    hash: u64,
    len: usize,
    spans: Vec<HighlightSpan>,
    end_state: HighlightStateSnapshot,
}

fn spawn_highlight_worker() -> HighlightWorker {
    let (tx, rx_cmd) = crossbeam_channel::unbounded();
    let (tx_evt, rx_evt) = crossbeam_channel::unbounded();

    thread::Builder::new()
        .name("localpaste-gui-highlight".to_string())
        .spawn(move || {
            let settings = SyntectSettings::default();
            let mut cache = HighlightWorkerCache::default();
            for req in rx_cmd.iter() {
                let mut latest = req;
                while let Ok(next) = rx_cmd.try_recv() {
                    latest = next;
                }
                let render = highlight_in_worker(&settings, &mut cache, latest);
                let _ = tx_evt.send(render);
            }
        })
        .expect("spawn highlight worker");

    HighlightWorker { tx, rx: rx_evt }
}

fn highlight_in_worker(
    settings: &SyntectSettings,
    cache: &mut HighlightWorkerCache,
    req: HighlightRequest,
) -> HighlightRender {
    if cache.language_hint != req.language_hint || cache.theme_key != req.theme_key {
        cache.language_hint = req.language_hint.clone();
        cache.theme_key = req.theme_key.clone();
        cache.lines.clear();
    }

    let syntax = settings
        .ps
        .find_syntax_by_name(&req.language_hint)
        .or_else(|| settings.ps.find_syntax_by_extension(&req.language_hint))
        .unwrap_or_else(|| settings.ps.find_syntax_plain_text());
    let theme = settings
        .ts
        .themes
        .get(req.theme_key.as_str())
        .unwrap_or_else(|| settings.ts.themes.values().next().expect("theme"));

    let highlighter = Highlighter::new(theme);
    let mut parse_state = ParseState::new(syntax);
    let mut highlight_state = HighlightState::new(&highlighter, ScopeStack::new());
    let default_state = HighlightStateSnapshot {
        parse: parse_state.clone(),
        highlight: highlight_state.clone(),
    };

    let old_lines = std::mem::take(&mut cache.lines);
    let mut new_lines = Vec::with_capacity(old_lines.len().max(1));
    let mut render_lines = Vec::with_capacity(old_lines.len().max(1));

    for (idx, line) in LinesWithEndings::from(req.text.as_str()).enumerate() {
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
                parse_state = old_line.end_state.parse.clone();
                highlight_state = old_line.end_state.highlight.clone();
                new_lines.push(old_line.clone());
                render_lines.push(HighlightRenderLine {
                    len: old_line.len,
                    spans: old_line.spans.clone(),
                });
                continue;
            }
        }

        let mut spans = Vec::new();
        if let Ok(ops) = parse_state.parse_line(line, &settings.ps) {
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
                spans.push(HighlightSpan {
                    range,
                    style: HighlightStyle {
                        color: [
                            style.foreground.r,
                            style.foreground.g,
                            style.foreground.b,
                            style.foreground.a,
                        ],
                        italics: style
                            .font_style
                            .contains(syntect::highlighting::FontStyle::ITALIC),
                        underline: style
                            .font_style
                            .contains(syntect::highlighting::FontStyle::UNDERLINE),
                    },
                });
            }
        }

        let end_state = HighlightStateSnapshot {
            parse: parse_state.clone(),
            highlight: highlight_state.clone(),
        };
        let line_len = line.len();
        new_lines.push(HighlightWorkerLine {
            hash: line_hash,
            len: line_len,
            spans: spans.clone(),
            end_state,
        });
        render_lines.push(HighlightRenderLine {
            len: line_len,
            spans,
        });
    }

    cache.lines = new_lines;

    HighlightRender {
        paste_id: req.paste_id,
        revision: req.revision,
        text_len: req.text.len(),
        language_hint: req.language_hint,
        theme_key: req.theme_key,
        lines: render_lines,
    }
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

fn is_word_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn word_range_at(text: &str, char_index: usize) -> Option<(usize, usize)> {
    if text.is_empty() {
        return None;
    }
    let total_chars = text.chars().count();
    let target_char_index = if char_index >= total_chars {
        total_chars.saturating_sub(1)
    } else {
        char_index
    };
    let byte_index = text
        .char_indices()
        .nth(target_char_index)
        .map(|(idx, _)| idx)?;
    let mut iter = text[byte_index..].chars();
    let current = iter.next()?;
    let current_is_word = is_word_char(current);
    let mut start_byte = byte_index;
    let mut end_byte = byte_index + current.len_utf8();

    for (idx, ch) in text[..byte_index].char_indices().rev() {
        if is_word_char(ch) == current_is_word {
            start_byte = idx;
        } else {
            break;
        }
    }

    let mut tail = text[end_byte..].chars();
    while let Some(ch) = tail.next() {
        if is_word_char(ch) == current_is_word {
            end_byte = end_byte.saturating_add(ch.len_utf8());
        } else {
            break;
        }
    }

    let start_char = text[..start_byte].chars().count();
    let selected_chars = text[start_byte..end_byte].chars().count();
    Some((start_char, start_char + selected_chars))
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
        });

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
                            ui.add(egui::Label::new(job));
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
                            let text_snapshot = self.selected_content.text.clone();
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
                assert_eq!(cache.highlight_cache.lines.len(), line_count);
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
