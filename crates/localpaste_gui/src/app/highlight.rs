//! Syntax highlighting caches and worker support for the native GUI editor.

use super::util::env_flag_enabled;
use crossbeam_channel::{Receiver, Sender};
use eframe::egui::{
    self,
    text::{LayoutJob, LayoutSection, TextFormat},
    Color32, FontId, Stroke,
};
use egui_extras::syntax_highlighting::CodeTheme;
use std::ops::Range;
use std::sync::Arc;
use std::thread;
use std::time::Instant;
use syntect::highlighting::{HighlightState, Highlighter, Style, ThemeSet};
use syntect::parsing::{ParseState, ScopeStack, SyntaxSet};
use syntect::util::LinesWithEndings;
use tracing::info;

/// Cached layout state for highlighted editor content.
#[derive(Default)]
pub(super) struct EditorLayoutCache {
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
    pub(super) last_highlight_ms: Option<f32>,
}

pub(super) struct EditorLayoutRequest<'a> {
    pub(super) ui: &'a egui::Ui,
    pub(super) text: &'a dyn egui::TextBuffer,
    pub(super) text_revision: Option<u64>,
    pub(super) wrap_width: f32,
    pub(super) language_hint: &'a str,
    pub(super) use_plain: bool,
    pub(super) theme: Option<&'a CodeTheme>,
    pub(super) highlight_render: Option<&'a HighlightRender>,
    pub(super) highlight_version: u64,
    pub(super) editor_font: &'a FontId,
    pub(super) syntect: &'a SyntectSettings,
}

struct BuildGalleyRequest<'a> {
    ui: &'a egui::Ui,
    text: &'a str,
    wrap_width: f32,
    language_hint: &'a str,
    use_plain: bool,
    theme: Option<&'a CodeTheme>,
    highlight_render: Option<&'a HighlightRender>,
    editor_font: &'a FontId,
    syntect: &'a SyntectSettings,
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
    /// Returns the number of cached highlight lines for tests and profiling.
    #[cfg(test)]
    pub(super) fn highlight_line_count(&self) -> usize {
        self.highlight_cache.lines.len()
    }

    pub(super) fn layout(&mut self, request: EditorLayoutRequest<'_>) -> Arc<egui::Galley> {
        let Some(revision) = request.text_revision else {
            return self.build_galley(BuildGalleyRequest {
                ui: request.ui,
                text: request.text.as_str(),
                wrap_width: request.wrap_width,
                language_hint: request.language_hint,
                use_plain: request.use_plain,
                theme: request.theme,
                highlight_render: request.highlight_render,
                editor_font: request.editor_font,
                syntect: request.syntect,
            });
        };

        let pixels_per_point = request.ui.ctx().pixels_per_point();
        let wrap_width = request.wrap_width.max(0.0).round();
        let theme_value = if request.use_plain {
            None
        } else {
            request.theme.cloned()
        };

        let cache_hit = self.galley.is_some()
            && self.revision == revision
            && self.use_plain == request.use_plain
            && self.wrap_width == wrap_width
            && self.pixels_per_point == pixels_per_point
            && self.language_hint == request.language_hint
            && self.font_id.as_ref() == Some(request.editor_font)
            && self.theme == theme_value
            && self.highlight_version == request.highlight_version;

        if cache_hit {
            if let Some(galley) = self.galley.as_ref() {
                return galley.clone();
            }
        }

        let started = Instant::now();
        let galley = self.build_galley(BuildGalleyRequest {
            ui: request.ui,
            text: request.text.as_str(),
            wrap_width,
            language_hint: request.language_hint,
            use_plain: request.use_plain,
            theme: request.theme,
            highlight_render: request.highlight_render,
            editor_font: request.editor_font,
            syntect: request.syntect,
        });
        if !request.use_plain {
            let elapsed_ms = started.elapsed().as_secs_f32() * 1000.0;
            self.last_highlight_ms = Some(elapsed_ms);
        }

        self.revision = revision;
        self.use_plain = request.use_plain;
        self.wrap_width = wrap_width;
        self.pixels_per_point = pixels_per_point;
        self.language_hint = request.language_hint.to_string();
        self.font_id = Some(request.editor_font.clone());
        self.theme = theme_value;
        self.highlight_version = request.highlight_version;
        self.galley = Some(galley.clone());

        galley
    }

    fn build_galley(&mut self, request: BuildGalleyRequest<'_>) -> Arc<egui::Galley> {
        let mut job = if request.use_plain {
            plain_layout_job(
                request.ui,
                request.text,
                request.editor_font,
                request.wrap_width,
            )
        } else if let Some(render) = request.highlight_render {
            self.build_render_job(request.ui, request.text, render, request.editor_font)
        } else if let Some(theme) = request.theme {
            self.build_highlight_job(
                request.ui,
                request.text,
                request.language_hint,
                theme,
                request.editor_font,
                request.syntect,
            )
        } else {
            plain_layout_job(
                request.ui,
                request.text,
                request.editor_font,
                request.wrap_width,
            )
        };
        job.wrap.max_width = request.wrap_width;
        request.ui.fonts_mut(|f| f.layout_job(job))
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

        let syntax = resolve_syntax(&settings.ps, language_hint);
        let theme = settings
            .ts
            .themes
            .get(theme_key)
            .or_else(|| settings.ts.themes.values().next());
        let Some(theme) = theme else {
            return plain_layout_job(ui, text, editor_font, f32::INFINITY);
        };

        let highlighter = Highlighter::new(theme);
        let mut parse_state = ParseState::new(syntax);
        let mut highlight_state = HighlightState::new(&highlighter, ScopeStack::new());
        let default_state = HighlightStateSnapshot {
            parse: parse_state.clone(),
            highlight: highlight_state.clone(),
        };
        let lines: Vec<&str> = LinesWithEndings::from(text).collect();
        let new_hashes: Vec<u64> = lines
            .iter()
            .map(|line| hash_bytes(line.as_bytes()))
            .collect();
        let mut old_lines = align_old_lines_by_hash(
            std::mem::take(&mut self.highlight_cache.lines),
            &new_hashes,
            |line| line.hash,
        );
        let mut new_lines = Vec::with_capacity(lines.len().max(1));
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
        let mut prev_line_reused = false;

        for (idx, line) in lines.iter().enumerate() {
            let line_hash = new_hashes[idx];
            if line_start_state_matches(
                idx,
                prev_line_reused,
                &old_lines,
                &parse_state,
                &highlight_state,
                &default_state,
                |line: &HighlightLineCache| &line.end_state,
            ) && line_hash_matches(&old_lines, idx, line_hash, |line: &HighlightLineCache| {
                line.hash
            }) {
                let old_line = old_lines[idx].take().expect("checked Some");
                append_sections(&mut job, &old_line.sections, line_start);
                parse_state = old_line.end_state.parse.clone();
                highlight_state = old_line.end_state.highlight.clone();
                new_lines.push(old_line);
                line_start += line.len();
                prev_line_reused = true;
                continue;
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
                    job = plain_layout_job(ui, text, editor_font, f32::INFINITY);
                    new_lines.clear();
                    break;
                }
            }

            line_start += line.len();
            prev_line_reused = false;
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

/// Builds a layout job for a single rendered line in the virtual preview.
pub(super) fn build_virtual_line_job(
    ui: &egui::Ui,
    line: &str,
    editor_font: &FontId,
    render_line: Option<&HighlightRenderLine>,
    use_plain: bool,
) -> LayoutJob {
    if use_plain || render_line.is_none() {
        return plain_layout_job(ui, line, editor_font, f32::INFINITY);
    }
    let render_line = render_line.expect("render line checked above");
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

fn normalized_syntax_key(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .map(|ch| ch.to_ascii_lowercase())
        .collect()
}

fn try_resolve_syntax_candidate<'a>(
    ps: &'a syntect::parsing::SyntaxSet,
    candidate: &str,
) -> Option<&'a syntect::parsing::SyntaxReference> {
    let trimmed = candidate.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(syntax) = ps.find_syntax_by_name(trimmed) {
        return Some(syntax);
    }
    if let Some(syntax) = ps.find_syntax_by_extension(trimmed) {
        return Some(syntax);
    }

    for syntax in ps.syntaxes() {
        if syntax.name.eq_ignore_ascii_case(trimmed) {
            return Some(syntax);
        }
    }

    let normalized = normalized_syntax_key(trimmed);
    if !normalized.is_empty() {
        for syntax in ps.syntaxes() {
            if normalized_syntax_key(syntax.name.as_str()) == normalized {
                return Some(syntax);
            }
        }
    }

    ps.syntaxes().iter().find(|syntax| {
        syntax
            .file_extensions
            .iter()
            .any(|ext| ext.eq_ignore_ascii_case(trimmed))
    })
}

fn syntax_fallback_candidates(hint_lower: &str) -> &'static [&'static str] {
    match hint_lower {
        "cs" => &["C#", "cs"],
        "shell" => &["Bourne Again Shell (bash)", "bash", "sh"],
        "cpp" => &["C++", "cpp", "cc"],
        "objectivec" => &["Objective-C", "m"],
        "dockerfile" => &["Dockerfile", "bash", "sh"],
        "makefile" => &["Makefile", "make"],
        "latex" => &["LaTeX", "tex"],
        // Syntect defaults used by egui do not ship native grammars for these in all bundles.
        // Keep explicit fallback only for high-priority labels to avoid hiding unsupported
        // language gaps behind misleading tokenization.
        "typescript" => &["ts", "JavaScript", "js"],
        "toml" => &["yaml", "YAML", "properties"],
        "swift" => &["Objective-C", "C++", "c"],
        "powershell" => &["ps1", "Bourne Again Shell (bash)", "bash", "sh"],
        "sass" => &["sass", "Ruby Haml", "css"],
        _ => &[],
    }
}

fn resolve_syntax<'a>(
    ps: &'a syntect::parsing::SyntaxSet,
    hint: &str,
) -> &'a syntect::parsing::SyntaxReference {
    let hint_trimmed = hint.trim();
    if hint_trimmed.is_empty() {
        return ps.find_syntax_plain_text();
    }

    let hint_lower = hint_trimmed.to_ascii_lowercase();
    if matches!(hint_lower.as_str(), "text" | "txt" | "plain" | "plaintext") {
        return ps.find_syntax_plain_text();
    }

    if let Some(syntax) = try_resolve_syntax_candidate(ps, hint_trimmed) {
        return syntax;
    }

    for candidate in syntax_fallback_candidates(hint_lower.as_str()) {
        if let Some(syntax) = try_resolve_syntax_candidate(ps, candidate) {
            return syntax;
        }
    }

    ps.find_syntax_plain_text()
}

/// Provides reusable syntect sets for worker and UI layouts.
pub(super) struct SyntectSettings {
    pub(super) ps: SyntaxSet,
    pub(super) ts: ThemeSet,
}

impl Default for SyntectSettings {
    fn default() -> Self {
        Self {
            ps: SyntaxSet::load_defaults_newlines(),
            ts: ThemeSet::load_defaults(),
        }
    }
}

/// Maps an egui code theme to a syntect theme key.
pub(super) fn syntect_theme_key(theme: &CodeTheme) -> &'static str {
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

fn plain_layout_job(ui: &egui::Ui, text: &str, editor_font: &FontId, wrap_width: f32) -> LayoutJob {
    LayoutJob::simple(
        text.to_owned(),
        editor_font.clone(),
        ui.visuals().text_color(),
        wrap_width,
    )
}

fn line_start_state_matches<T, F>(
    idx: usize,
    prev_line_reused: bool,
    old_lines: &[Option<T>],
    parse_state: &ParseState,
    highlight_state: &HighlightState,
    default_state: &HighlightStateSnapshot,
    end_state_for: F,
) -> bool
where
    F: Fn(&T) -> &HighlightStateSnapshot,
{
    if idx == 0 {
        return default_state.parse == *parse_state && default_state.highlight == *highlight_state;
    }
    if prev_line_reused {
        return true;
    }
    old_lines
        .get(idx - 1)
        .and_then(|line| line.as_ref())
        .map(|line| {
            let end_state = end_state_for(line);
            end_state.parse == *parse_state && end_state.highlight == *highlight_state
        })
        .unwrap_or(false)
}

fn line_hash_matches<T, F>(
    old_lines: &[Option<T>],
    idx: usize,
    expected_hash: u64,
    hash_for: F,
) -> bool
where
    F: Fn(&T) -> u64,
{
    old_lines
        .get(idx)
        .and_then(|line| line.as_ref())
        .map(|line| hash_for(line) == expected_hash)
        .unwrap_or(false)
}

const FNV_OFFSET: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x00000100000001B3;

fn hash_bytes_step(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

pub(super) fn hash_bytes(bytes: &[u8]) -> u64 {
    hash_bytes_step(FNV_OFFSET, bytes)
}

pub(super) fn hash_text_chunks<'a, I>(chunks: I) -> u64
where
    I: IntoIterator<Item = &'a str>,
{
    let mut hash = FNV_OFFSET;
    for chunk in chunks {
        hash = hash_bytes_step(hash, chunk.as_bytes());
    }
    hash
}

pub(super) fn align_old_lines_by_hash<T, F>(
    old_lines: Vec<T>,
    new_hashes: &[u64],
    hash_for: F,
) -> Vec<Option<T>>
where
    F: Fn(&T) -> u64,
{
    let old_len = old_lines.len();
    let new_len = new_hashes.len();
    if new_len == 0 {
        return Vec::new();
    }
    if old_len == 0 {
        let mut out = Vec::with_capacity(new_len);
        out.resize_with(new_len, || None);
        return out;
    }

    let mut old: Vec<Option<T>> = old_lines.into_iter().map(Some).collect();

    let mut prefix = 0usize;
    while prefix < old_len && prefix < new_len {
        let Some(ref line) = old[prefix] else {
            break;
        };
        if hash_for(line) == new_hashes[prefix] {
            prefix += 1;
        } else {
            break;
        }
    }

    let mut suffix = 0usize;
    while suffix < (old_len - prefix) && suffix < (new_len - prefix) {
        let old_idx = old_len - 1 - suffix;
        let new_idx = new_len - 1 - suffix;
        let Some(ref line) = old[old_idx] else {
            break;
        };
        if hash_for(line) == new_hashes[new_idx] {
            suffix += 1;
        } else {
            break;
        }
    }

    let mut aligned = Vec::with_capacity(new_len);
    aligned.resize_with(new_len, || None);
    for i in 0..prefix {
        aligned[i] = old[i].take();
    }
    for j in 0..suffix {
        let new_idx = new_len - suffix + j;
        let old_idx = old_len - suffix + j;
        aligned[new_idx] = old[old_idx].take();
    }
    aligned
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

/// Highlight spans for a single rendered line.
#[derive(Clone)]
pub(super) struct HighlightRenderLine {
    len: usize,
    spans: Vec<HighlightSpan>,
}

/// Highlight rendering output for an entire buffer snapshot.
#[derive(Clone)]
pub(super) struct HighlightRender {
    pub(super) paste_id: String,
    pub(super) revision: u64,
    pub(super) text_len: usize,
    pub(super) content_hash: u64,
    pub(super) language_hint: String,
    pub(super) theme_key: String,
    pub(super) lines: Vec<HighlightRenderLine>,
}

impl HighlightRender {
    pub(super) fn matches_context(
        &self,
        paste_id: &str,
        language_hint: &str,
        theme_key: &str,
    ) -> bool {
        self.paste_id == paste_id
            && self.language_hint == language_hint
            && self.theme_key == theme_key
    }

    pub(super) fn matches_exact(
        &self,
        revision: u64,
        text_len: usize,
        content_hash: u64,
        language_hint: &str,
        theme_key: &str,
        paste_id: &str,
    ) -> bool {
        self.revision == revision
            && self.text_len == text_len
            && self.content_hash == content_hash
            && self.matches_context(paste_id, language_hint, theme_key)
    }
}

/// Highlight request payload sent to the worker thread.
#[derive(Clone)]
pub(super) struct HighlightRequest {
    pub(super) paste_id: String,
    pub(super) revision: u64,
    pub(super) text: String,
    pub(super) content_hash: u64,
    pub(super) language_hint: String,
    pub(super) theme_key: String,
}

/// Metadata used to coalesce highlight requests while typing.
pub(super) struct HighlightRequestMeta {
    pub(super) paste_id: String,
    pub(super) revision: u64,
    pub(super) text_len: usize,
    pub(super) content_hash: u64,
    pub(super) language_hint: String,
    pub(super) theme_key: String,
}

impl HighlightRequestMeta {
    pub(super) fn matches(
        &self,
        revision: u64,
        text_len: usize,
        content_hash: u64,
        language_hint: &str,
        theme_key: &str,
        paste_id: &str,
    ) -> bool {
        self.revision == revision
            && self.text_len == text_len
            && self.content_hash == content_hash
            && self.language_hint == language_hint
            && self.theme_key == theme_key
            && self.paste_id == paste_id
    }

    pub(super) fn matches_render(&self, render: &HighlightRender) -> bool {
        self.revision == render.revision
            && self.text_len == render.text_len
            && self.content_hash == render.content_hash
            && self.language_hint == render.language_hint
            && self.theme_key == render.theme_key
            && self.paste_id == render.paste_id
    }
}

/// Background worker handles syntect highlighting off the UI thread.
pub(super) struct HighlightWorker {
    pub(super) tx: Sender<HighlightRequest>,
    pub(super) rx: Receiver<HighlightRender>,
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

/// Spawns the syntect worker thread and returns its channel endpoints.
pub(super) fn spawn_highlight_worker() -> HighlightWorker {
    let (tx, rx_cmd) = crossbeam_channel::unbounded();
    let (tx_evt, rx_evt) = crossbeam_channel::unbounded();
    let trace_enabled = env_flag_enabled("LOCALPASTE_HIGHLIGHT_TRACE");

    thread::Builder::new()
        .name("localpaste-gui-highlight".to_string())
        .spawn(move || {
            let settings = SyntectSettings::default();
            let mut cache = HighlightWorkerCache::default();
            for req in rx_cmd.iter() {
                let mut latest: HighlightRequest = req;
                // Coalesce backlog bursts so stale highlight work is skipped.
                while let Ok(next) = rx_cmd.try_recv() {
                    latest = next;
                }
                let started = Instant::now();
                let trace_paste_id = latest.paste_id.clone();
                let trace_revision = latest.revision;
                let trace_len = latest.text.len();
                let render = highlight_in_worker(&settings, &mut cache, latest);
                let _ = tx_evt.send(render);
                if trace_enabled {
                    let elapsed_ms = started.elapsed().as_secs_f32() * 1000.0;
                    info!(
                        target: "localpaste_gui::highlight",
                        event = "worker_done",
                        paste_id = trace_paste_id.as_str(),
                        revision = trace_revision,
                        text_len = trace_len,
                        elapsed_ms = elapsed_ms,
                        "highlight worker pass"
                    );
                }
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

    let syntax = resolve_syntax(&settings.ps, &req.language_hint);
    let theme = settings
        .ts
        .themes
        .get(req.theme_key.as_str())
        .or_else(|| settings.ts.themes.values().next());
    let Some(theme) = theme else {
        let lines = LinesWithEndings::from(req.text.as_str())
            .map(|line| HighlightRenderLine {
                len: line.len(),
                spans: Vec::new(),
            })
            .collect();
        return HighlightRender {
            paste_id: req.paste_id,
            revision: req.revision,
            text_len: req.text.len(),
            content_hash: req.content_hash,
            language_hint: req.language_hint,
            theme_key: req.theme_key,
            lines,
        };
    };

    let lines: Vec<&str> = LinesWithEndings::from(req.text.as_str()).collect();
    let new_hashes: Vec<u64> = lines
        .iter()
        .map(|line| hash_bytes(line.as_bytes()))
        .collect();
    let mut old_lines =
        align_old_lines_by_hash(std::mem::take(&mut cache.lines), &new_hashes, |line| {
            line.hash
        });

    let highlighter = Highlighter::new(theme);
    let mut parse_state = ParseState::new(syntax);
    let mut highlight_state = HighlightState::new(&highlighter, ScopeStack::new());
    let default_state = HighlightStateSnapshot {
        parse: parse_state.clone(),
        highlight: highlight_state.clone(),
    };

    let mut new_lines = Vec::with_capacity(lines.len().max(1));
    let mut render_lines = Vec::with_capacity(lines.len().max(1));
    let mut prev_line_reused = false;

    for (idx, line) in lines.iter().enumerate() {
        let line_hash = new_hashes[idx];
        if line_start_state_matches(
            idx,
            prev_line_reused,
            &old_lines,
            &parse_state,
            &highlight_state,
            &default_state,
            |line: &HighlightWorkerLine| &line.end_state,
        ) && line_hash_matches(&old_lines, idx, line_hash, |line: &HighlightWorkerLine| {
            line.hash
        }) {
            let old_line = old_lines[idx].take().expect("checked Some");
            parse_state = old_line.end_state.parse.clone();
            highlight_state = old_line.end_state.highlight.clone();
            render_lines.push(HighlightRenderLine {
                len: old_line.len,
                spans: old_line.spans.clone(),
            });
            new_lines.push(old_line);
            prev_line_reused = true;
            continue;
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
        prev_line_reused = false;
    }

    cache.lines = new_lines;

    HighlightRender {
        paste_id: req.paste_id,
        revision: req.revision,
        text_len: req.text.len(),
        content_hash: req.content_hash,
        language_hint: req.language_hint,
        theme_key: req.theme_key,
        lines: render_lines,
    }
}

/// Normalizes user-facing language names into syntect-compatible hints.
pub(super) fn syntect_language_hint(language: &str) -> String {
    let canonical = localpaste_core::detection::canonical::canonicalize(language);
    if canonical.is_empty() {
        "text".to_string()
    } else {
        canonical
    }
}

#[cfg(test)]
mod resolver_tests {
    use super::{resolve_syntax, syntect_language_hint, SyntectSettings};

    #[test]
    fn resolve_syntax_handles_common_canonical_labels() {
        let settings = SyntectSettings::default();
        let cases = [
            "rust",
            "python",
            "javascript",
            "go",
            "java",
            "html",
            "css",
            "json",
            "yaml",
            "sql",
        ];
        for label in cases {
            let syntax = resolve_syntax(&settings.ps, label);
            assert_ne!(syntax.name, "Plain Text", "label: {label}");
        }
    }

    #[test]
    fn resolve_syntax_handles_alias_labels() {
        let settings = SyntectSettings::default();
        for label in ["cs", "shell", "cpp", "powershell"] {
            let syntax = resolve_syntax(&settings.ps, label);
            assert_ne!(syntax.name, "Plain Text", "label: {label}");
        }
    }

    #[test]
    fn resolve_syntax_falls_back_to_plain_for_unknown_or_text() {
        let settings = SyntectSettings::default();
        assert_eq!(
            resolve_syntax(&settings.ps, "somethingtotallyunknown").name,
            "Plain Text"
        );
        assert_eq!(resolve_syntax(&settings.ps, "").name, "Plain Text");
        assert_eq!(resolve_syntax(&settings.ps, "text").name, "Plain Text");
        assert_eq!(resolve_syntax(&settings.ps, "txt").name, "Plain Text");
    }

    #[test]
    fn syntect_hint_uses_canonical_labels() {
        assert_eq!(syntect_language_hint(" csharp "), "cs");
        assert_eq!(syntect_language_hint("bash"), "shell");
        assert_eq!(syntect_language_hint(""), "text");
    }

    #[test]
    fn resolve_syntax_high_priority_and_new_language_matrix() {
        let settings = SyntectSettings::default();
        let labels = ["typescript", "toml", "swift", "powershell"];
        let mut plain_labels = Vec::new();
        for label in labels {
            let syntax = resolve_syntax(&settings.ps, label);
            if syntax.name == "Plain Text" {
                plain_labels.push(label);
            }
        }
        assert!(
            plain_labels.is_empty(),
            "labels that still resolve to plain text: {:?}",
            plain_labels
        );
    }

    #[test]
    fn resolve_syntax_leaves_known_unsupported_labels_as_plain_text() {
        let settings = SyntectSettings::default();
        for label in ["zig", "scss", "kotlin", "elixir", "dart"] {
            let syntax = resolve_syntax(&settings.ps, label);
            assert_eq!(syntax.name, "Plain Text", "label: {label}");
        }
    }
}
