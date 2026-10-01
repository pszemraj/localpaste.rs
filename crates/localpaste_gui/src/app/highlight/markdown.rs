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
        for (scope, color) in [
            ("text.html.markdown.localpaste", prose),
            ("markup.raw.block.markdown.localpaste, markup.raw.inline.markdown.localpaste", code),
            ("constant.other.reference.link.markdown.localpaste, punctuation.definition.raw.markdown.localpaste", marker),
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
    }
}
