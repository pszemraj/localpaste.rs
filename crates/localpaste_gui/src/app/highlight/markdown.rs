//! Owned Markdown grammar and readable mappings onto existing syntax colors.

use super::SyntectSettings;
use syntect::highlighting::{Highlighter, StyleModifier, ThemeItem, ThemeSet};
use syntect::parsing::{Scope, SyntaxDefinition, SyntaxSet};

/// Build the shared syntax/theme sets with the project Markdown grammar.
///
/// # Returns
/// The default grammars plus LocalPaste Markdown and its readable scope mappings.
///
/// # Panics
/// Panics if the checked-in grammar or constant scope selectors are invalid.
pub(super) fn settings() -> SyntectSettings {
    let mut builder = SyntaxSet::load_defaults_newlines().into_builder();
    builder.add(
        SyntaxDefinition::load_from_str(
            include_str!("../../../assets/LocalPaste-Markdown.sublime-syntax"),
            true,
            None,
        )
        .expect("checked-in Markdown grammar"),
    );
    let mut themes = ThemeSet::load_defaults();
    for theme in themes.themes.values_mut() {
        let highlighter = Highlighter::new(theme);
        let prose = highlighter.get_default().foreground;
        let code = highlighter
            .style_for_stack(&[Scope::new("string").expect("string scope")])
            .foreground;
        let marker = highlighter
            .style_for_stack(&[Scope::new("keyword").expect("keyword scope")])
            .foreground;
        let structure = highlighter
            .style_for_stack(&[Scope::new("constant.numeric").expect("numeric scope")])
            .foreground;
        for (scope, color) in [
            ("text.html.markdown.localpaste", prose),
            ("markup.raw.block.markdown.localpaste, markup.raw.inline.markdown.localpaste", code),
            ("constant.other.reference.link.markdown.localpaste, punctuation.definition.raw.markdown.localpaste", marker),
            ("markup.heading.markdown", marker),
            ("markup.underline.link.markdown", code),
            ("punctuation.definition.blockquote.markdown, punctuation.definition.list.markdown", structure),
        ] {
            theme.scopes.push(ThemeItem { scope: scope.parse().expect("Markdown selector"), style: StyleModifier { foreground: Some(color), ..Default::default() } });
        }
    }
    SyntectSettings {
        ps: builder.build(),
        ts: themes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syntect::easy::HighlightLines;
    use syntect::highlighting::{FontStyle, Style};

    fn styled_segments(
        settings: &SyntectSettings,
        lines: &mut HighlightLines<'_>,
        text: &str,
    ) -> Vec<(Style, String)> {
        lines
            .highlight_line(text, &settings.ps)
            .expect("checked-in Markdown grammar highlights")
            .into_iter()
            .map(|(style, text)| (style, text.to_string()))
            .collect()
    }

    fn non_empty_colors(
        settings: &SyntectSettings,
        lines: &mut HighlightLines<'_>,
        text: &str,
    ) -> Vec<(syntect::highlighting::Color, String)> {
        styled_segments(settings, lines, text)
            .into_iter()
            .filter(|(_, text)| !text.trim().is_empty())
            .map(|(style, text)| (style.foreground, text))
            .collect()
    }

    #[test]
    fn escaped_punctuation_stays_prose_without_disabling_real_markup() {
        let settings = settings();
        let syntax = super::super::resolve_syntax(&settings.ps, "markdown");
        let theme = &settings.ts.themes["base16-mocha.dark"];
        let prose = Highlighter::new(theme).get_default().foreground;
        let mut lines = HighlightLines::new(syntax, theme);
        for text in [
            "Use \\` to write a literal backtick.\n",
            "Escaped \\*stars\\* and \\_underscores\\_ and \\[link](url).\n",
        ] {
            assert!(styled_segments(&settings, &mut lines, text)
                .iter()
                .all(|(style, _)| style.foreground == prose && style.font_style.is_empty()));
        }
        let segments = styled_segments(
            &settings,
            &mut lines,
            "Next prose line with **bold** emphasis.\n",
        );
        assert_eq!(segments.first().unwrap().0.foreground, prose);
        assert!(segments.iter().any(|(style, text)| {
            text.contains("bold") && style.font_style.contains(FontStyle::BOLD)
        }));

        // An escaped backslash leaves the following delimiter active. Escapes
        // inside code are literal and must not prevent its closing delimiter.
        let code = Highlighter::new(theme)
            .style_for_stack(&[Scope::new("string").unwrap()])
            .foreground;
        let segments = styled_segments(&settings, &mut lines, "Use \\\\`code\\` after.\n");
        assert!(segments
            .iter()
            .any(|(style, text)| text.contains("code") && style.foreground == code));
        assert_eq!(segments.last().unwrap().0.foreground, prose);
    }

    #[test]
    fn footnotes_are_readable_and_fences_end_at_matching_delimiters() {
        let settings = settings();
        let syntax = super::super::resolve_syntax(&settings.ps, "markdown");
        assert_eq!(syntax.name, "LocalPaste Markdown");
        let theme = &settings.ts.themes["base16-mocha.dark"];
        let prose = Highlighter::new(theme).get_default().foreground;
        let code = Highlighter::new(theme)
            .style_for_stack(&[Scope::new("string").unwrap()])
            .foreground;
        assert_ne!(prose, code);
        let mut lines = HighlightLines::new(syntax, theme);
        let colors = |lines: &mut HighlightLines<'_>, text: &str| {
            lines
                .highlight_line(text, &settings.ps)
                .unwrap()
                .into_iter()
                .filter(|(_, text)| !text.trim().is_empty())
                .map(|(style, text)| (style.foreground, text.to_string()))
                .collect::<Vec<_>>()
        };
        let footnote = colors(&mut lines, "[^note]: A readable footnote body.\n");
        assert_eq!(footnote.last().unwrap().0, prose);
        for (open, short, close) in [
            ("````rust\n", "```\n", "`````\n"),
            ("~~~~\n", "~~~\n", "~~~~\n"),
            ("```\n", "~~~\n", "```\n"),
        ] {
            colors(&mut lines, open);
            for text in ["fn main() { /* **not markup** */ }\n", short, "more code\n"] {
                assert!(
                    colors(&mut lines, text)
                        .iter()
                        .all(|(color, _)| *color == code),
                    "{text}"
                );
            }
            // Fence-looking literals inside a top-level block are body text,
            // not closers, when indented four columns or prefixed as containers.
            let delimiter = if open.starts_with('`') {
                "`````"
            } else {
                "~~~~~"
            };
            for prefix in ["    ", "\t", "> ", "- ", "1. "] {
                colors(&mut lines, &format!("{prefix}{delimiter}\n"));
                assert!(
                    colors(&mut lines, "**still code**\n")
                        .iter()
                        .all(|(color, _)| *color == code),
                    "{prefix:?}{delimiter}"
                );
            }
            colors(&mut lines, close);
            assert!(colors(&mut lines, "ordinary prose after fence\n")
                .iter()
                .all(|(color, _)| *color == prose));
        }
    }

    #[test]
    fn inline_code_recovers_at_blank_paragraphs_without_breaking_multiline_spans() {
        let settings = settings();
        let syntax = super::super::resolve_syntax(&settings.ps, "markdown");
        let theme = &settings.ts.themes["base16-mocha.dark"];
        let prose = Highlighter::new(theme).get_default().foreground;
        let code = Highlighter::new(theme)
            .style_for_stack(&[Scope::new("string").unwrap()])
            .foreground;
        let colors = |lines: &mut HighlightLines<'_>, text: &str| {
            lines
                .highlight_line(text, &settings.ps)
                .unwrap()
                .into_iter()
                .filter(|(_, text)| !text.trim().is_empty())
                .map(|(style, text)| (style.foreground, text.to_string()))
                .collect::<Vec<_>>()
        };

        let mut unmatched = HighlightLines::new(syntax, theme);
        colors(&mut unmatched, "unmatched `code\n");
        assert!(colors(&mut unmatched, "still inline code\n")
            .iter()
            .all(|(color, _)| *color == code));
        colors(&mut unmatched, "\n");
        assert!(colors(&mut unmatched, "ordinary prose\n")
            .iter()
            .all(|(color, _)| *color == prose));

        let mut matched = HighlightLines::new(syntax, theme);
        colors(&mut matched, "``first line\n");
        assert!(colors(&mut matched, "`shorter delimiter stays code\n")
            .iter()
            .all(|(color, _)| *color == code));
        let closing = colors(&mut matched, "second line`` after\n");
        assert!(closing
            .iter()
            .any(|(color, text)| *color == code && text.contains("second line")));
        assert!(closing
            .iter()
            .any(|(color, text)| *color == prose && text.contains("after")));

        let heading = Highlighter::new(theme)
            .style_for_stack(&[Scope::new("keyword").unwrap()])
            .foreground;
        for (boundary, expected, closer) in [
            ("# Heading\n", heading, None),
            ("---\r\n", heading, None),
            ("```rust\n", heading, Some("```\n")),
            ("~~~rust\n", heading, Some("~~~\n")),
        ] {
            let mut lines = HighlightLines::new(syntax, theme);
            colors(&mut lines, "unmatched `code\n");
            let boundary_colors = colors(&mut lines, boundary);
            assert!(
                boundary_colors.iter().all(|(color, _)| *color == expected),
                "{boundary:?}: {boundary_colors:?} expected {expected:?}"
            );
            if let Some(closer) = closer {
                assert!(colors(&mut lines, "**literal code body**\n")
                    .iter()
                    .all(|(color, _)| *color == code));
                colors(&mut lines, closer);
            }
            assert!(
                colors(&mut lines, "ordinary prose\n")
                    .iter()
                    .all(|(color, _)| *color == prose),
                "after {boundary:?}"
            );
        }
    }

    #[test]
    fn fences_keep_indented_list_and_blockquote_bodies_as_code() {
        let settings = settings();
        let syntax = super::super::resolve_syntax(&settings.ps, "markdown");
        let theme = &settings.ts.themes["base16-mocha.dark"];
        let prose = Highlighter::new(theme).get_default().foreground;
        let code = Highlighter::new(theme)
            .style_for_stack(&[Scope::new("string").unwrap()])
            .foreground;

        for (opening, body, blank, closing) in [
            ("    ```rust\n", "    let value = 1;\n", "\n", "    ```\n"),
            ("    ~~~rust\n", "    let value = 1;\n", "\n", "    ~~~\n"),
            ("1. ```rust\n", "   let value = 1;\n", "\n", "   ```\n"),
            ("> ```rust\n", "> let value = 1;\n", ">\n", "> ```\n"),
        ] {
            let mut lines = HighlightLines::new(syntax, theme);
            non_empty_colors(&settings, &mut lines, opening);
            assert!(
                non_empty_colors(&settings, &mut lines, body)
                    .iter()
                    .all(|(color, _)| *color == code),
                "{opening:?} should start a code fence"
            );
            non_empty_colors(&settings, &mut lines, blank);
            assert!(
                non_empty_colors(&settings, &mut lines, body)
                    .iter()
                    .all(|(color, _)| *color == code),
                "blank lines must not close {opening:?}"
            );
            non_empty_colors(&settings, &mut lines, closing);
            assert!(
                non_empty_colors(&settings, &mut lines, "ordinary prose after fence\n")
                    .iter()
                    .all(|(color, _)| *color == prose),
                "{closing:?} should close the code fence"
            );
        }
    }

    #[test]
    fn quoted_fences_end_with_the_containing_blockquote() {
        let settings = settings();
        let syntax = super::super::resolve_syntax(&settings.ps, "markdown");
        let theme = &settings.ts.themes["base16-mocha.dark"];
        let highlighter = Highlighter::new(theme);
        let prose = highlighter.get_default().foreground;
        let code = highlighter
            .style_for_stack(&[Scope::new("string").unwrap()])
            .foreground;
        let heading = highlighter
            .style_for_stack(&[Scope::new("keyword").unwrap()])
            .foreground;
        for fence in ["```", "~~~"] {
            for boundary in ["", "\n", "\r\n"] {
                let mut lines = HighlightLines::new(syntax, theme);
                non_empty_colors(&settings, &mut lines, &format!("> {fence}text\n"));
                // A quoted blank line stays inside the fence, even when the
                // quote marker's indentation changes on the following line.
                non_empty_colors(&settings, &mut lines, ">\n");
                assert!(
                    non_empty_colors(&settings, &mut lines, "  > **literal code**\n")
                        .iter()
                        .all(|(color, _)| *color == code)
                );
                if !boundary.is_empty() {
                    non_empty_colors(&settings, &mut lines, boundary);
                }
                assert!(
                    non_empty_colors(&settings, &mut lines, "# Outside heading\n")
                        .iter()
                        .all(|(color, _)| *color == heading),
                    "{fence:?}, {boundary:?}"
                );
                assert!(non_empty_colors(&settings, &mut lines, "Outside prose\n")
                    .iter()
                    .all(|(color, _)| *color == prose));
            }
        }
    }

    #[test]
    fn structural_markdown_scopes_use_readable_theme_colors() {
        let settings = settings();
        let syntax = super::super::resolve_syntax(&settings.ps, "markdown");
        let theme = &settings.ts.themes["base16-mocha.dark"];
        let highlighter = Highlighter::new(theme);
        let prose = highlighter.get_default().foreground;
        let heading = highlighter
            .style_for_stack(&[Scope::new("keyword").unwrap()])
            .foreground;
        let link = highlighter
            .style_for_stack(&[Scope::new("string").unwrap()])
            .foreground;
        let structure = highlighter
            .style_for_stack(&[Scope::new("constant.numeric").unwrap()])
            .foreground;
        assert_ne!(heading, prose);
        assert_ne!(link, prose);
        assert_ne!(structure, prose);

        let mut lines = HighlightLines::new(syntax, theme);
        assert!(non_empty_colors(&settings, &mut lines, "# Heading\n")
            .iter()
            .all(|(color, _)| *color == heading));
        assert!(non_empty_colors(
            &settings,
            &mut lines,
            "[LocalPaste](https://example.test)\n"
        )
        .iter()
        .all(|(color, _)| *color == link));
        for marker in ["> quoted text\n", "- listed text\n"] {
            let colors = non_empty_colors(&settings, &mut lines, marker);
            assert_eq!(colors[0].0, structure, "{marker:?}");
            assert_eq!(colors[1].0, prose, "{marker:?}");
        }
    }

    #[test]
    fn underscore_italics_do_not_match_within_identifiers_or_urls() {
        let settings = settings();
        let syntax = super::super::resolve_syntax(&settings.ps, "markdown");
        let theme = &settings.ts.themes["base16-mocha.dark"];
        let text = "my_var_name http://x.com/a_b_c _emphasis_ *asterisk*\n";
        let ranges = [
            text.find("my_var_name").unwrap()
                ..text.find("my_var_name").unwrap() + "my_var_name".len(),
            text.find("http://x.com/a_b_c").unwrap()
                ..text.find("http://x.com/a_b_c").unwrap() + "http://x.com/a_b_c".len(),
        ];
        let mut lines = HighlightLines::new(syntax, theme);
        let segments = styled_segments(&settings, &mut lines, text);
        let mut offset = 0;
        let mut emphasis_is_italic = false;
        let mut asterisk_is_italic = false;
        for (style, segment) in segments {
            let range = offset..offset + segment.len();
            offset = range.end;
            if style.font_style.contains(FontStyle::ITALIC) {
                assert!(
                    ranges.iter().all(|protected| {
                        range.end <= protected.start || protected.end <= range.start
                    }),
                    "{segment:?} in an identifier or URL was styled as italics"
                );
                emphasis_is_italic |= segment.contains("emphasis");
                asterisk_is_italic |= segment.contains("asterisk");
            }
        }
        assert_eq!(offset, text.len());
        assert!(emphasis_is_italic);
        assert!(asterisk_is_italic);
    }
}
