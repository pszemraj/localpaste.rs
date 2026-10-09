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

fn assert_sections_use_char_boundaries(job: &LayoutJob) {
    for section in &job.sections {
        assert!(job.text.is_char_boundary(section.byte_range.start));
        assert!(job.text.is_char_boundary(section.byte_range.end));
    }
}

fn assert_virtual_line_segment_gaps(
    text: &str,
    render_line: HighlightRenderLine,
    visible_range: Range<usize>,
    expected_gaps: &[Range<usize>],
) {
    egui::__run_test_ctx(|ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let font = egui::FontId::monospace(14.0);
            let job = build_virtual_line_segment_job_owned(
                ui,
                text.to_string(),
                &font,
                Some(&render_line),
                false,
                visible_range.clone(),
            );

            assert_sections_cover(&job, text.len());
            for expected in expected_gaps {
                assert_has_section(&job, expected.clone());
            }
        });
    });
}

#[test]
fn virtual_line_segment_job_fills_unstyled_gaps() {
    let cases = [
        (
            "abcdef",
            HighlightRenderLine {
                len: 6,
                spans: vec![test_span(0..2), test_span(4..5)],
            },
            0..6,
            vec![2..4, 5..6],
        ),
        (
            "bcde",
            HighlightRenderLine {
                len: 6,
                spans: vec![test_span(2..3)],
            },
            1..5,
            vec![0..1, 2..4],
        ),
    ];

    for (text, render_line, visible_range, expected_gaps) in cases {
        assert_virtual_line_segment_gaps(text, render_line, visible_range, &expected_gaps);
    }
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

#[test]
fn virtual_line_segment_job_clamps_non_boundary_spans() {
    egui::__run_test_ctx(|ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let font = egui::FontId::monospace(14.0);
            let line = "🔥Title".to_string();
            let render_line = HighlightRenderLine {
                len: line.len(),
                spans: vec![test_span(1..4)],
            };
            let job = build_virtual_line_segment_job_owned(
                ui,
                line.clone(),
                &font,
                Some(&render_line),
                false,
                0..line.len(),
            );

            assert_sections_cover(&job, line.len());
            assert_sections_use_char_boundaries(&job);
        });
    });
}

#[test]
fn render_job_clamps_stale_line_offsets_with_emoji_boundaries() {
    egui::__run_test_ctx(|ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let font = egui::FontId::monospace(14.0);
            let text = "🔥Title\nok\n";
            let render = HighlightRender {
                paste_id: "alpha".to_string(),
                revision: 1,
                text_len: text.len().saturating_sub(3),
                base_revision: Some(0),
                base_text_len: Some(text.len().saturating_sub(4)),
                language_hint: "markdown".to_string(),
                theme_key: "base16-mocha.dark".to_string(),
                changed_line_range: Some(0..2),
                lines: vec![
                    HighlightRenderLine {
                        // Deliberately stale/non-boundary byte length.
                        len: 1,
                        spans: vec![test_span(0..3)],
                    },
                    HighlightRenderLine {
                        len: text.len(),
                        spans: vec![test_span(1..text.len())],
                    },
                ],
            };
            let cache = EditorLayoutCache::default();
            let job = cache.build_render_job(ui, text, &render, &font);

            assert_sections_use_char_boundaries(&job);
            assert_sections_cover(&job, text.len());
        });
    });
}

mod markdown_block_boundaries {
    use super::super::markdown::{
        settings,
        tests::{non_empty_colors, styled_segments},
    };
    use super::super::resolve_syntax;
    use syntect::easy::HighlightLines;
    use syntect::highlighting::Highlighter;
    use syntect::parsing::Scope;

    #[test]
    fn indentation_cannot_interrupt_paragraphs_or_lazy_list_continuations() {
        let settings = settings();
        let syntax = resolve_syntax(&settings.ps, "markdown");
        let theme = &settings.ts.themes["base16-mocha.dark"];
        let prose = Highlighter::new(theme).get_default().foreground;
        for seed in [
            "paragraph\n",
            "`inline-only paragraph`\n",
            "**bold-only paragraph**\n",
            "- item\nlazy continuation\n",
            "1. item\nlazy continuation\n",
            "---\nparent:\n",
        ] {
            for indent in ["    ", "\t", "  \t"] {
                let mut lines = HighlightLines::new(syntax, theme);
                for line in seed.split_inclusive('\n') {
                    styled_segments(&settings, &mut lines, line);
                }
                let result = non_empty_colors(
                    &settings,
                    &mut lines,
                    &format!("{indent}continued prose: value\n"),
                );
                assert!(
                    result.iter().all(|(color, _)| *color == prose),
                    "{seed:?} {indent:?}: {result:?}"
                );
            }
        }
    }

    #[test]
    fn blank_lines_allow_indented_code_after_paragraphs() {
        let settings = settings();
        let syntax = resolve_syntax(&settings.ps, "markdown");
        let theme = &settings.ts.themes["base16-mocha.dark"];
        let highlighter = Highlighter::new(theme);
        let prose = highlighter.get_default().foreground;
        let code = highlighter
            .style_for_stack(&[Scope::new("string").unwrap()])
            .foreground;
        for separator in ["\n", " \n", "\r\n", "\t\n"] {
            for indent in ["    ", "\t", "  \t"] {
                let mut lines = HighlightLines::new(syntax, theme);
                styled_segments(&settings, &mut lines, "paragraph\n");
                styled_segments(&settings, &mut lines, separator);
                let result = non_empty_colors(
                    &settings,
                    &mut lines,
                    &format!("{indent}**literal code**\n"),
                );
                assert!(result.iter().all(|(color, _)| *color == code));
                let result = non_empty_colors(&settings, &mut lines, "dedented prose\n");
                assert!(result.iter().all(|(color, _)| *color == prose));
            }
        }
    }

    #[test]
    fn quoted_inline_spans_recover_at_blank_lines_and_sibling_items() {
        let settings = settings();
        let syntax = resolve_syntax(&settings.ps, "markdown");
        let theme = &settings.ts.themes["base16-mocha.dark"];
        let highlighter = Highlighter::new(theme);
        let prose = highlighter.get_default().foreground;
        let code = highlighter
            .style_for_stack(&[Scope::new("string").unwrap()])
            .foreground;
        for quote in [">", "  >", "> >"] {
            for boundary in [
                "",
                " ",
                "\t",
                "- sibling item",
                "1. sibling item",
                "# Heading",
            ] {
                let mut lines = HighlightLines::new(syntax, theme);
                styled_segments(&settings, &mut lines, &format!("{quote} unmatched `code\n"));
                styled_segments(&settings, &mut lines, &format!("{quote} {boundary}\n"));
                let result =
                    non_empty_colors(&settings, &mut lines, &format!("{quote} ordinary prose\n"));
                assert_eq!(result.last().unwrap().0, prose, "{quote:?} {boundary:?}");
            }
            let mut lines = HighlightLines::new(syntax, theme);
            styled_segments(&settings, &mut lines, &format!("{quote} ``first line\n"));
            let result = non_empty_colors(
                &settings,
                &mut lines,
                &format!("{quote} second line`` after\n"),
            );
            assert!(result
                .iter()
                .any(|(color, text)| { *color == code && text.contains("second line") }));
            assert_eq!(result.last().unwrap().0, prose);
        }
    }

    #[test]
    fn direct_list_fences_end_at_dedent_or_a_sibling_item() {
        let settings = settings();
        let syntax = resolve_syntax(&settings.ps, "markdown");
        let theme = &settings.ts.themes["base16-mocha.dark"];
        let highlighter = Highlighter::new(theme);
        let prose = highlighter.get_default().foreground;
        let code = highlighter
            .style_for_stack(&[Scope::new("string").unwrap()])
            .foreground;
        for fence in ["```", "~~~", "````", "~~~~"] {
            for (items, marker, indent) in [
                ("", "- ", "  "),
                ("", "1. ", "   "),
                ("", "123456789. ", "           "),
                ("- outer\n", "  - ", "    "),
                ("", "-\t", "\t"),
                ("", "1.\t", "    "),
                ("", "12.\t", "\t"),
                ("", "1234.\t", "        "),
                ("", "123456789.\t", "\t\t\t"),
                ("", " -\t", "    "),
                ("", "  -\t", "\t"),
                ("", "   -\t", "        "),
                ("", "- \t ", "     "),
                ("- outer\n", "  -\t", "    "),
            ] {
                for boundary in [
                    "ordinary prose\n",
                    "- ordinary prose\n",
                    "1. ordinary prose\n",
                ] {
                    let mut lines = HighlightLines::new(syntax, theme);
                    for line in items.split_inclusive('\n') {
                        styled_segments(&settings, &mut lines, line);
                    }
                    styled_segments(&settings, &mut lines, &format!("{marker}{fence}rust\n"));
                    if fence.len() > 3 {
                        styled_segments(
                            &settings,
                            &mut lines,
                            &format!("{indent}{}\n", &fence[..3]),
                        );
                    }
                    assert!(non_empty_colors(
                        &settings,
                        &mut lines,
                        &format!("{indent}**literal body**\n")
                    )
                    .iter()
                    .all(|(color, _)| *color == code));
                    let result = non_empty_colors(&settings, &mut lines, boundary);
                    assert_eq!(
                        result.last().unwrap().0,
                        prose,
                        "{items:?}{marker}{fence}, {boundary:?}: {result:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn quoted_list_fences_end_with_the_item_or_quote() {
        let settings = settings();
        let syntax = resolve_syntax(&settings.ps, "markdown");
        let theme = &settings.ts.themes["base16-mocha.dark"];
        let highlighter = Highlighter::new(theme);
        let prose = highlighter.get_default().foreground;
        let code = highlighter
            .style_for_stack(&[Scope::new("string").unwrap()])
            .foreground;
        for fence in ["```", "~~~", "````", "~~~~"] {
            for (marker, indent) in [
                ("> - ", ">   "),
                ("> 1. ", "  >    "),
                ("> 123456789. ", ">            "),
                ("> -\t", "> \t"),
                (">- \t", ">    "),
                (">1.\t", "> \t "),
                ("  > -\t", ">     "),
                ("> - ", "  > \t"),
            ] {
                for boundary in [
                    "> ordinary prose\n",
                    "> - ordinary prose\n",
                    "> 1. ordinary prose\n",
                    "ordinary prose\n",
                ] {
                    let mut lines = HighlightLines::new(syntax, theme);
                    styled_segments(&settings, &mut lines, &format!("{marker}{fence}rust\n"));
                    styled_segments(&settings, &mut lines, ">\n");
                    styled_segments(&settings, &mut lines, &format!("{indent}    {fence}\n"));
                    if fence.len() > 3 {
                        styled_segments(
                            &settings,
                            &mut lines,
                            &format!("{indent}{}\n", &fence[..3]),
                        );
                    }
                    assert!(non_empty_colors(
                        &settings,
                        &mut lines,
                        &format!("{indent}**literal body**\n")
                    )
                    .iter()
                    .all(|(color, _)| *color == code));
                    let result = non_empty_colors(&settings, &mut lines, boundary);
                    assert_eq!(
                        result.last().unwrap().0,
                        prose,
                        "{marker:?}{fence}: {result:?}"
                    );
                }
                let mut lines = HighlightLines::new(syntax, theme);
                styled_segments(&settings, &mut lines, &format!("{marker}{fence}rust\n"));
                styled_segments(&settings, &mut lines, &format!("{indent}{fence}\n"));
                let result =
                    non_empty_colors(&settings, &mut lines, &format!("{indent}prose after\n"));
                assert_eq!(
                    result.last().unwrap().0,
                    prose,
                    "{marker:?}{fence}: {result:?}"
                );
            }
        }
    }
}
