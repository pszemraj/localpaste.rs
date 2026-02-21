//! Syntax highlighting caches and worker support for the native GUI editor.

mod reuse;
mod syntax;
#[cfg(test)]
mod tests;
mod worker;

use eframe::egui::{
    self,
    text::{LayoutJob, LayoutSection, TextFormat},
    Color32, FontId, Stroke,
};
use egui_extras::syntax_highlighting::CodeTheme;
use ropey::Rope;
use std::ops::Range;
use std::sync::Arc;
use std::time::Instant;
use syntect::highlighting::{HighlightState, Highlighter, Style, ThemeSet};
use syntect::parsing::{ParseState, ScopeStack, SyntaxSet};
use syntect::util::LinesWithEndings;

pub(super) use reuse::{
    align_old_lines_by_hash, hash_bytes, line_hash_matches, line_start_state_matches,
};
pub(super) use syntax::{resolve_syntax, syntect_language_hint};
pub(super) use worker::{spawn_highlight_worker, HighlightWorker};

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

/// Input bundle used to build a highlighted editor galley.
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
    ///
    /// # Returns
    /// Cached line count currently stored in the highlight cache.
    pub(super) fn highlight_line_count(&self) -> usize {
        self.highlight_cache.lines.len()
    }

    /// Builds (or reuses) the editor galley for the requested render snapshot.
    ///
    /// # Returns
    /// Shared galley ready for painting in the editor panel.
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
                (&default_state.parse, &default_state.highlight),
                |line: &HighlightLineCache| (&line.end_state.parse, &line.end_state.highlight),
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
                        let Some(range) = clamp_byte_range_to_char_boundaries(line, range.clone())
                        else {
                            continue;
                        };
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
            let line_end = line_start.saturating_add(line.len).min(text.len());
            let line_len = line_end.saturating_sub(line_start);
            let mut styled_sections = Vec::with_capacity(line.spans.len());
            for span in &line.spans {
                let start = span.range.start.min(line_len);
                let end = span.range.end.min(line_len);
                if start >= end {
                    continue;
                }
                styled_sections.push((start..end, render_span_to_format(span, editor_font)));
            }
            push_sections_with_default_gaps(
                &mut job,
                line_start,
                line_len,
                &default_format,
                styled_sections,
            );
            line_start = line_start.saturating_add(line.len);
        }
        if line_start < text.len() {
            push_sections_with_default_gaps(
                &mut job,
                line_start,
                text.len().saturating_sub(line_start),
                &default_format,
                Vec::new(),
            );
        }

        job
    }
}

/// Builds a layout job for a single rendered line in the virtual preview.
///
/// # Arguments
/// - `ui`: UI context used for default text styling.
/// - `line`: Line text to shape.
/// - `editor_font`: Font id used by editor rendering.
/// - `render_line`: Optional highlight spans for this line.
/// - `use_plain`: When `true`, bypass highlight spans.
///
/// # Returns
/// Layout job representing one unwrapped virtual line.
pub(super) fn build_virtual_line_job(
    ui: &egui::Ui,
    line: &str,
    editor_font: &FontId,
    render_line: Option<&HighlightRenderLine>,
    use_plain: bool,
) -> LayoutJob {
    build_virtual_line_job_owned(ui, line.to_owned(), editor_font, render_line, use_plain)
}

/// Builds a layout job for a single rendered line in the virtual preview/editor
/// when the caller already owns the line text buffer.
fn build_virtual_line_job_owned(
    ui: &egui::Ui,
    line: String,
    editor_font: &FontId,
    render_line: Option<&HighlightRenderLine>,
    use_plain: bool,
) -> LayoutJob {
    if use_plain || render_line.is_none() {
        return plain_layout_job_owned(ui, line, editor_font, f32::INFINITY);
    }
    let render_line = render_line.expect("render line checked above");
    let mut job = LayoutJob {
        text: line,
        ..Default::default()
    };
    let line_len = job.text.len();
    let default_format = TextFormat {
        font_id: editor_font.clone(),
        color: ui.visuals().text_color(),
        ..Default::default()
    };

    let mut styled_sections = Vec::with_capacity(render_line.spans.len());
    for span in &render_line.spans {
        let start = span.range.start.min(line_len);
        let end = span.range.end.min(line_len);
        if start >= end {
            continue;
        }
        let Some(range) = clamp_byte_range_to_char_boundaries(job.text.as_str(), start..end) else {
            continue;
        };
        styled_sections.push((range, render_span_to_format(span, editor_font)));
    }
    push_sections_with_default_gaps(&mut job, 0, line_len, &default_format, styled_sections);

    job.wrap.max_width = f32::INFINITY;
    job
}

/// Builds a layout job for a wrapped visual-row segment from a physical line.
///
/// `line_byte_range` is relative to the original physical line bytes and is used
/// to intersect highlight spans onto this segment.
///
/// # Arguments
/// - `ui`: UI context used for default text styling.
/// - `segment`: Owned text segment for the target visual row.
/// - `editor_font`: Font id used by editor rendering.
/// - `render_line`: Optional highlight spans for the full physical line.
/// - `use_plain`: When `true`, bypass highlight spans.
/// - `line_byte_range`: Segment byte range within the physical line.
///
/// # Returns
/// Layout job representing one unwrapped visual-row segment.
///
/// # Panics
/// Panics if supplied highlight ranges violate expected segment invariants.
pub(super) fn build_virtual_line_segment_job_owned(
    ui: &egui::Ui,
    segment: String,
    editor_font: &FontId,
    render_line: Option<&HighlightRenderLine>,
    use_plain: bool,
    line_byte_range: Range<usize>,
) -> LayoutJob {
    if use_plain || render_line.is_none() {
        return plain_layout_job_owned(ui, segment, editor_font, f32::INFINITY);
    }
    let render_line = render_line.expect("render line checked above");
    let mut job = LayoutJob {
        text: segment,
        ..Default::default()
    };
    let line_len = job.text.len();
    let default_format = TextFormat {
        font_id: editor_font.clone(),
        color: ui.visuals().text_color(),
        ..Default::default()
    };

    if line_len == 0 {
        job.wrap.max_width = f32::INFINITY;
        return job;
    }
    let mut styled_sections = Vec::with_capacity(render_line.spans.len());
    for span in &render_line.spans {
        let start = span.range.start.max(line_byte_range.start);
        let end = span.range.end.min(line_byte_range.end);
        if start >= end {
            continue;
        }
        let local_start = start.saturating_sub(line_byte_range.start).min(line_len);
        let local_end = end.saturating_sub(line_byte_range.start).min(line_len);
        if local_start >= local_end {
            continue;
        }
        let Some(range) =
            clamp_byte_range_to_char_boundaries(job.text.as_str(), local_start..local_end)
        else {
            continue;
        };
        styled_sections.push((range, render_span_to_format(span, editor_font)));
    }
    push_sections_with_default_gaps(&mut job, 0, line_len, &default_format, styled_sections);

    job.wrap.max_width = f32::INFINITY;
    job
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
///
/// # Returns
/// Syntect theme identifier matching the current light/dark mode.
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

fn floor_char_boundary(text: &str, idx: usize) -> usize {
    let mut idx = idx.min(text.len());
    while idx > 0 && !text.is_char_boundary(idx) {
        idx = idx.saturating_sub(1);
    }
    idx
}

fn ceil_char_boundary(text: &str, idx: usize) -> usize {
    let mut idx = idx.min(text.len());
    while idx < text.len() && !text.is_char_boundary(idx) {
        idx = idx.saturating_add(1);
    }
    idx.min(text.len())
}

fn clamp_byte_range_to_char_boundaries(text: &str, range: Range<usize>) -> Option<Range<usize>> {
    let start = floor_char_boundary(text, range.start);
    let end = ceil_char_boundary(text, range.end);
    if start >= end {
        return None;
    }
    Some(start..end)
}

fn append_sections(job: &mut LayoutJob, sections: &[LayoutSection], offset: usize) {
    for section in sections {
        let mut section = section.clone();
        let start = section.byte_range.start.saturating_add(offset);
        let end = section.byte_range.end.saturating_add(offset);
        if let Some(range) = clamp_byte_range_to_char_boundaries(job.text.as_str(), start..end) {
            section.byte_range = range;
            job.sections.push(section);
        }
    }
}

fn push_sections_with_default_gaps(
    job: &mut LayoutJob,
    offset: usize,
    line_len: usize,
    default_format: &TextFormat,
    mut styled_sections: Vec<(Range<usize>, TextFormat)>,
) {
    if line_len == 0 {
        return;
    }
    if styled_sections.is_empty() {
        if let Some(range) = clamp_byte_range_to_char_boundaries(
            job.text.as_str(),
            offset..(offset.saturating_add(line_len)),
        ) {
            job.sections.push(LayoutSection {
                leading_space: 0.0,
                byte_range: range,
                format: default_format.clone(),
            });
        }
        return;
    }

    styled_sections.sort_unstable_by(|a, b| {
        a.0.start
            .cmp(&b.0.start)
            .then_with(|| a.0.end.cmp(&b.0.end))
    });

    let mut cursor = 0usize;
    for (range, format) in styled_sections {
        let start = range.start.min(line_len);
        let end = range.end.min(line_len);
        if start >= end {
            continue;
        }

        if start > cursor {
            if let Some(range) = clamp_byte_range_to_char_boundaries(
                job.text.as_str(),
                (offset.saturating_add(cursor))..(offset.saturating_add(start)),
            ) {
                job.sections.push(LayoutSection {
                    leading_space: 0.0,
                    byte_range: range,
                    format: default_format.clone(),
                });
            }
        }

        let styled_start = start.max(cursor);
        if styled_start < end {
            if let Some(range) = clamp_byte_range_to_char_boundaries(
                job.text.as_str(),
                (offset.saturating_add(styled_start))..(offset.saturating_add(end)),
            ) {
                job.sections.push(LayoutSection {
                    leading_space: 0.0,
                    byte_range: range,
                    format,
                });
            }
            cursor = end;
        }
    }

    if cursor < line_len {
        if let Some(range) = clamp_byte_range_to_char_boundaries(
            job.text.as_str(),
            (offset.saturating_add(cursor))..(offset.saturating_add(line_len)),
        ) {
            job.sections.push(LayoutSection {
                leading_space: 0.0,
                byte_range: range,
                format: default_format.clone(),
            });
        }
    }
}

fn plain_layout_job(ui: &egui::Ui, text: &str, editor_font: &FontId, wrap_width: f32) -> LayoutJob {
    plain_layout_job_owned(ui, text.to_owned(), editor_font, wrap_width)
}

fn plain_layout_job_owned(
    ui: &egui::Ui,
    text: String,
    editor_font: &FontId,
    wrap_width: f32,
) -> LayoutJob {
    LayoutJob::simple(
        text,
        editor_font.clone(),
        ui.visuals().text_color(),
        wrap_width,
    )
}

#[derive(Clone, PartialEq, Eq)]
struct HighlightSpan {
    range: Range<usize>,
    style: HighlightStyle,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct HighlightStyle {
    color: [u8; 4],
    italics: bool,
    underline: bool,
}

/// Highlight spans for a single rendered line.
#[derive(Clone, PartialEq, Eq)]
pub(super) struct HighlightRenderLine {
    len: usize,
    spans: Vec<HighlightSpan>,
}

impl HighlightRenderLine {
    #[cfg(test)]
    /// Creates a plain (unhighlighted) render line for tests.
    ///
    /// # Returns
    /// Render line with no highlight spans and provided byte length.
    pub(super) fn plain(len: usize) -> Self {
        Self {
            len,
            spans: Vec::new(),
        }
    }

    #[cfg(test)]
    /// Returns the stored byte length for this render line.
    ///
    /// # Returns
    /// Line byte length used by highlighting/layout tests.
    pub(super) fn len_for_test(&self) -> usize {
        self.len
    }
}

/// Highlight rendering output for an entire buffer snapshot.
#[derive(Clone)]
pub(super) struct HighlightRender {
    pub(super) paste_id: String,
    pub(super) revision: u64,
    pub(super) text_len: usize,
    /// Worker base snapshot used to compute `changed_line_range`.
    pub(super) base_revision: Option<u64>,
    pub(super) base_text_len: Option<usize>,
    pub(super) language_hint: String,
    pub(super) theme_key: String,
    /// Best-effort changed line range versus the previous worker snapshot.
    /// `None` means unknown and callers should fall back to structural diffing.
    pub(super) changed_line_range: Option<Range<usize>>,
    pub(super) lines: Vec<HighlightRenderLine>,
}

/// Highlight patch output for a changed line range within a buffer snapshot.
#[derive(Clone)]
pub(super) struct HighlightPatch {
    pub(super) paste_id: String,
    pub(super) revision: u64,
    pub(super) text_len: usize,
    pub(super) base_revision: u64,
    pub(super) base_text_len: usize,
    pub(super) language_hint: String,
    pub(super) theme_key: String,
    pub(super) total_lines: usize,
    pub(super) line_range: Range<usize>,
    pub(super) lines: Vec<HighlightRenderLine>,
}

/// Worker output event carrying either full-highlight render or range patch.
#[derive(Clone)]
pub(super) enum HighlightWorkerResult {
    Render(HighlightRender),
    Patch(HighlightPatch),
}

impl HighlightRender {
    /// Checks whether render context matches paste/language/theme identifiers.
    ///
    /// # Arguments
    /// - `paste_id`: Target paste id.
    /// - `language_hint`: Canonical language hint.
    /// - `theme_key`: Syntect theme key.
    ///
    /// # Returns
    /// `true` when context identifiers match.
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

    /// Checks whether render snapshot exactly matches revision + context.
    ///
    /// # Arguments
    /// - `revision`: Expected revision.
    /// - `text_len`: Expected text byte length.
    /// - `language_hint`: Canonical language hint.
    /// - `theme_key`: Syntect theme key.
    /// - `paste_id`: Target paste id.
    ///
    /// # Returns
    /// `true` when revision, length, and context all match.
    pub(super) fn matches_exact(
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

/// Highlight request payload sent to the worker thread.
#[derive(Clone)]
pub(super) struct HighlightRequest {
    pub(super) paste_id: String,
    pub(super) revision: u64,
    pub(super) text: HighlightRequestText,
    pub(super) language_hint: String,
    pub(super) theme_key: String,
    pub(super) edit_hint: Option<VirtualEditHint>,
    pub(super) patch_base_revision: Option<u64>,
    pub(super) patch_base_text_len: Option<usize>,
}

/// Snapshot payload transported to the highlight worker.
#[derive(Clone)]
pub(super) enum HighlightRequestText {
    Owned(String),
    Rope(Rope),
}

impl HighlightRequestText {
    /// Returns request text length in bytes.
    ///
    /// # Returns
    /// UTF-8 byte length of owned string or rope payload.
    pub(super) fn len_bytes(&self) -> usize {
        match self {
            Self::Owned(text) => text.len(),
            Self::Rope(rope) => rope.len_bytes(),
        }
    }

    /// Converts request text payload into an owned [`String`].
    ///
    /// # Returns
    /// Owned string representation of this request payload.
    pub(super) fn into_string(self) -> String {
        match self {
            Self::Owned(text) => text,
            Self::Rope(rope) => rope.to_string(),
        }
    }
}

/// Lightweight edit metadata captured from virtual-editor operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct VirtualEditHint {
    pub(super) start_line: usize,
    pub(super) touched_lines: usize,
    pub(super) inserted_chars: usize,
    pub(super) deleted_chars: usize,
}

/// Metadata used to coalesce highlight requests while typing.
pub(super) struct HighlightRequestMeta {
    pub(super) paste_id: String,
    pub(super) revision: u64,
    pub(super) text_len: usize,
    pub(super) language_hint: String,
    pub(super) theme_key: String,
}

impl HighlightRequestMeta {
    /// Checks whether metadata matches a target revision/context tuple.
    ///
    /// # Arguments
    /// - `revision`: Expected revision.
    /// - `text_len`: Expected text byte length.
    /// - `language_hint`: Expected language hint.
    /// - `theme_key`: Expected theme key.
    /// - `paste_id`: Expected paste id.
    ///
    /// # Returns
    /// `true` when all metadata fields match.
    pub(super) fn matches(
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

    /// Checks whether metadata matches a full render snapshot.
    ///
    /// # Returns
    /// `true` when request metadata identifies the same render output.
    pub(super) fn matches_render(&self, render: &HighlightRender) -> bool {
        self.revision == render.revision
            && self.text_len == render.text_len
            && self.language_hint == render.language_hint
            && self.theme_key == render.theme_key
            && self.paste_id == render.paste_id
    }
}
