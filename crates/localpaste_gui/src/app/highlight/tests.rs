//! Tests for highlight layout section coverage under stale/context renders.

use super::*;

fn test_style() -> HighlightStyle {
    HighlightStyle {
        color: [120, 180, 240, 255],
        italics: false,
        underline: false,
    }
}

fn test_span(range: Range<usize>) -> HighlightSpan {
    HighlightSpan {
        range,
        style: test_style(),
    }
}

fn assert_sections_cover(job: &LayoutJob, len: usize) {
    let mut ranges: Vec<Range<usize>> = job
        .sections
        .iter()
        .map(|section| section.byte_range.clone())
        .collect();
    ranges.sort_unstable_by(|a, b| a.start.cmp(&b.start).then_with(|| a.end.cmp(&b.end)));
    let mut cursor = 0usize;
    for range in ranges {
        assert!(
            range.start <= cursor,
            "layout job has gap before {}",
            range.start
        );
        cursor = cursor.max(range.end);
    }
    assert_eq!(cursor, len);
}

fn assert_has_section(job: &LayoutJob, expected: Range<usize>) {
    assert!(
        job.sections.iter().any(|section| {
            section.byte_range.start == expected.start && section.byte_range.end == expected.end
        }),
        "expected section {:?} not found",
        expected
    );
}

#[test]
fn virtual_line_job_fills_gaps_for_partial_stale_spans() {
    egui::__run_test_ctx(|ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let font = egui::FontId::monospace(14.0);
            let render_line = HighlightRenderLine {
                len: 6,
                spans: vec![test_span(0..2), test_span(4..5)],
            };
            let job = build_virtual_line_job(ui, "abcdef", &font, Some(&render_line), false);

            assert_sections_cover(&job, 6);
            assert_has_section(&job, 2..4);
            assert_has_section(&job, 5..6);
        });
    });
}

#[test]
fn virtual_line_segment_job_fills_prefix_and_suffix_gaps() {
    egui::__run_test_ctx(|ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let font = egui::FontId::monospace(14.0);
            let render_line = HighlightRenderLine {
                len: 6,
                spans: vec![test_span(2..3)],
            };
            let job = build_virtual_line_segment_job_owned(
                ui,
                "bcde".to_string(),
                &font,
                Some(&render_line),
                false,
                1..5,
            );

            assert_sections_cover(&job, 4);
            assert_has_section(&job, 0..1);
            assert_has_section(&job, 2..4);
        });
    });
}

#[test]
fn render_job_fills_unstyled_gaps_with_default_format() {
    egui::__run_test_ctx(|ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let font = egui::FontId::monospace(14.0);
            let text = "abcdef\n";
            let render = HighlightRender {
                paste_id: "alpha".to_string(),
                revision: 1,
                text_len: text.len(),
                base_revision: None,
                base_text_len: None,
                language_hint: "rust".to_string(),
                theme_key: "base16-mocha.dark".to_string(),
                changed_line_range: None,
                lines: vec![HighlightRenderLine {
                    len: text.len(),
                    spans: vec![test_span(0..2), test_span(4..5)],
                }],
            };
            let cache = EditorLayoutCache::default();
            let job = cache.build_render_job(ui, text, &render, &font);

            assert_sections_cover(&job, text.len());
            assert_has_section(&job, 2..4);
            assert_has_section(&job, 5..text.len());
        });
    });
}
