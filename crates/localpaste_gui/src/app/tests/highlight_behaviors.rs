//! Highlight cache/render alignment tests for editor and staged-highlight flows.

use super::super::highlight::hash_bytes;
use super::*;

#[test]
fn highlight_cache_reuses_layout_when_unchanged() {
    let mut cache = EditorLayoutCache::default();
    let buffer = EditorBuffer::new("def foo():\n    return 1\n".to_string());
    let syntect = SyntectSettings::default();

    egui::__run_test_ctx(|ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let font = egui::FontId::monospace(14.0);
            let theme = CodeTheme::dark(14.0);
            let _ = cache.layout(EditorLayoutRequest {
                ui,
                text: &buffer,
                text_revision: Some(buffer.revision()),
                wrap_width: 400.0,
                language_hint: "py",
                use_plain: false,
                theme: Some(&theme),
                highlight_render: None,
                highlight_version: 0,
                editor_font: &font,
                syntect: &syntect,
            });
            let first_ms = cache.last_highlight_ms;
            let line_count = LinesWithEndings::from(buffer.as_str()).count();
            let _ = cache.layout(EditorLayoutRequest {
                ui,
                text: &buffer,
                text_revision: Some(buffer.revision()),
                wrap_width: 400.0,
                language_hint: "py",
                use_plain: false,
                theme: Some(&theme),
                highlight_render: None,
                highlight_version: 0,
                editor_font: &font,
                syntect: &syntect,
            });

            assert_eq!(cache.last_highlight_ms, first_ms);
            assert_eq!(cache.highlight_line_count(), line_count);
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
            let font = egui::FontId::monospace(14.0);
            let theme = CodeTheme::dark(14.0);
            let _ = cache.layout(EditorLayoutRequest {
                ui,
                text: &buffer,
                text_revision: Some(buffer.revision()),
                wrap_width: 400.0,
                language_hint: "py",
                use_plain: false,
                theme: Some(&theme),
                highlight_render: None,
                highlight_version: 0,
                editor_font: &font,
                syntect: &syntect,
            });

            buffer.insert_text("x", 0);

            let _ = cache.layout(EditorLayoutRequest {
                ui,
                text: &buffer,
                text_revision: Some(buffer.revision()),
                wrap_width: 400.0,
                language_hint: "py",
                use_plain: false,
                theme: Some(&theme),
                highlight_render: None,
                highlight_version: 0,
                editor_font: &font,
                syntect: &syntect,
            });
            let line_count = LinesWithEndings::from(buffer.as_str()).count();
            assert_eq!(cache.highlight_line_count(), line_count);
        });
    });
}

#[test]
fn align_old_lines_handles_insert_delete_and_replace_cases() {
    let cases = [
        (
            vec![
                FakeHighlightLine { hash: 1, name: "a" },
                FakeHighlightLine { hash: 2, name: "b" },
                FakeHighlightLine { hash: 3, name: "c" },
                FakeHighlightLine { hash: 4, name: "d" },
            ],
            vec![1, 2, 99, 3, 4],
            vec![Some("a"), Some("b"), None, Some("c"), Some("d")],
        ),
        (
            vec![
                FakeHighlightLine { hash: 1, name: "a" },
                FakeHighlightLine { hash: 2, name: "b" },
                FakeHighlightLine { hash: 3, name: "c" },
                FakeHighlightLine { hash: 4, name: "d" },
            ],
            vec![1, 3, 4],
            vec![Some("a"), Some("c"), Some("d")],
        ),
        (
            vec![
                FakeHighlightLine { hash: 1, name: "a" },
                FakeHighlightLine { hash: 2, name: "b" },
                FakeHighlightLine { hash: 4, name: "d" },
            ],
            vec![1, 77, 4],
            vec![Some("a"), None, Some("d")],
        ),
    ];

    for (old, hashes, expected) in cases {
        let aligned = align_old_lines_by_hash(old, hashes.as_slice(), |line| line.hash);
        assert_eq!(aligned_names(&aligned), expected);
    }
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

fn assert_virtual_selection_text(
    content: &str,
    start: VirtualCursor,
    end: VirtualCursor,
    expected: &str,
) {
    let mut harness = make_app();
    harness.app.selected_content.reset(content.to_string());
    harness.app.virtual_selection.select_range(start, end);
    let copied = harness.app.virtual_selection_text().expect("copied text");
    assert_eq!(copied, expected);
}

fn assert_virtual_select_line_text(content: &str, line: usize, expected: &str) {
    let mut harness = make_app();
    harness.app.reset_virtual_editor(content);
    harness.app.virtual_select_line(line);
    let copied = harness.app.virtual_selected_text().expect("copied text");
    assert_eq!(copied, expected);
}

#[test]
fn virtual_selection_text_matrix_preserves_line_boundaries() {
    let cases = [
        (
            "alpha\nbeta\ngamma",
            VirtualCursor { line: 0, column: 2 },
            VirtualCursor { line: 2, column: 3 },
            "pha\nbeta\ngam",
        ),
        (
            "a\n\nb",
            VirtualCursor { line: 0, column: 1 },
            VirtualCursor { line: 2, column: 0 },
            "\n\n",
        ),
    ];

    for (content, start, end, expected) in cases {
        assert_virtual_selection_text(content, start, end, expected);
    }
}

#[test]
fn virtual_select_line_matrix_handles_terminal_and_non_terminal_lines() {
    let cases = [("one\ntwo\nthree", 1, "two\n"), ("one\ntwo", 1, "two")];
    for (content, line, expected) in cases {
        assert_virtual_select_line_text(content, line, expected);
    }
}

#[test]
fn staged_highlight_waits_for_idle() {
    let mut harness = make_app();
    harness.app.selected_content.insert_text("x", 0);
    let active_revision = harness.app.selected_content.revision();
    let active_len = harness.app.selected_content.len();
    harness.app.highlight_render = Some(HighlightRender {
        paste_id: "alpha".to_string(),
        revision: active_revision.saturating_sub(1),
        text_len: active_len,
        content_hash: hash_bytes(harness.app.selected_content.as_str().as_bytes()),
        language_hint: "py".to_string(),
        theme_key: "base16-mocha.dark".to_string(),
        lines: Vec::new(),
    });
    let render = HighlightRender {
        paste_id: "alpha".to_string(),
        revision: active_revision,
        text_len: active_len,
        content_hash: hash_bytes(harness.app.selected_content.as_str().as_bytes()),
        language_hint: "py".to_string(),
        theme_key: "base16-mocha.dark".to_string(),
        lines: Vec::new(),
    };
    harness.app.highlight_staged = Some(render.clone());
    let now = Instant::now();
    harness.app.last_interaction_at = Some(now);
    harness.app.maybe_apply_staged_highlight(now);
    assert_eq!(
        harness
            .app
            .highlight_render
            .as_ref()
            .map(|render| render.revision),
        Some(0)
    );
    assert!(harness.app.highlight_staged.is_some());

    let idle_now = now + HIGHLIGHT_APPLY_IDLE + Duration::from_millis(10);
    harness.app.maybe_apply_staged_highlight(idle_now);
    assert_eq!(
        harness
            .app
            .highlight_render
            .as_ref()
            .map(|render| render.revision),
        Some(1)
    );
    assert!(harness.app.highlight_staged.is_none());
}

#[test]
fn staged_highlight_applies_immediately_without_current_render() {
    let mut harness = make_app();
    let active_revision = harness.app.selected_content.revision();
    let active_len = harness.app.selected_content.len();
    harness.app.highlight_staged = Some(HighlightRender {
        paste_id: "alpha".to_string(),
        revision: active_revision,
        text_len: active_len,
        content_hash: hash_bytes(harness.app.selected_content.as_str().as_bytes()),
        language_hint: "py".to_string(),
        theme_key: "base16-mocha.dark".to_string(),
        lines: Vec::new(),
    });
    let now = Instant::now();
    harness.app.last_interaction_at = Some(now);
    harness.app.maybe_apply_staged_highlight(now);
    assert!(harness.app.highlight_render.is_some());
}

#[test]
fn staged_highlight_stale_matrix_drops_mismatched_revision_length_or_hash_without_version_bump() {
    #[derive(Clone, Copy)]
    enum StaleKind {
        Revision,
        TextLen,
        ContentHash,
    }

    let cases = [
        (StaleKind::Revision, 19_u64),
        (StaleKind::TextLen, 23_u64),
        (StaleKind::ContentHash, 29_u64),
    ];

    for (kind, expected_version) in cases {
        let mut harness = make_app();
        harness.app.highlight_version = expected_version;
        let active_revision = harness.app.selected_content.revision();
        let active_len = harness.app.selected_content.len();
        let active_hash = hash_bytes(harness.app.selected_content.as_str().as_bytes());

        let mut staged = HighlightRender {
            paste_id: "alpha".to_string(),
            revision: active_revision,
            text_len: active_len,
            content_hash: active_hash,
            language_hint: "py".to_string(),
            theme_key: "base16-mocha.dark".to_string(),
            lines: Vec::new(),
        };
        match kind {
            StaleKind::Revision => staged.revision = active_revision.saturating_add(1),
            StaleKind::TextLen => staged.text_len = active_len.saturating_add(1),
            StaleKind::ContentHash => staged.content_hash = active_hash.wrapping_add(1),
        }
        harness.app.highlight_staged = Some(staged);

        harness.app.maybe_apply_staged_highlight(Instant::now());

        assert!(harness.app.highlight_staged.is_none());
        assert!(harness.app.highlight_render.is_none());
        assert_eq!(harness.app.highlight_version, expected_version);
    }
}

#[test]
fn highlight_request_skips_when_staged_matches() {
    let mut harness = make_app();
    let render = HighlightRender {
        paste_id: "alpha".to_string(),
        revision: 0,
        text_len: harness.app.selected_content.len(),
        content_hash: hash_bytes(harness.app.selected_content.as_str().as_bytes()),
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
fn queue_highlight_render_ignores_older_revision_when_current_exists() {
    let mut harness = make_app();
    harness.app.highlight_render = Some(HighlightRender {
        paste_id: "alpha".to_string(),
        revision: 9,
        text_len: harness.app.selected_content.len(),
        content_hash: hash_bytes(harness.app.selected_content.as_str().as_bytes()),
        language_hint: "py".to_string(),
        theme_key: "base16-mocha.dark".to_string(),
        lines: Vec::new(),
    });
    harness.app.queue_highlight_render(HighlightRender {
        paste_id: "alpha".to_string(),
        revision: 4,
        text_len: harness.app.selected_content.len(),
        content_hash: hash_bytes(harness.app.selected_content.as_str().as_bytes()),
        language_hint: "py".to_string(),
        theme_key: "base16-mocha.dark".to_string(),
        lines: Vec::new(),
    });

    assert!(harness.app.highlight_staged.is_none());
    assert_eq!(
        harness
            .app
            .highlight_render
            .as_ref()
            .map(|render| render.revision),
        Some(9)
    );
}

#[test]
fn paste_saved_keeps_existing_highlight_render() {
    let mut harness = make_app();
    harness.app.highlight_version = 7;
    harness.app.highlight_render = Some(HighlightRender {
        paste_id: "alpha".to_string(),
        revision: 42,
        text_len: harness.app.selected_content.len(),
        content_hash: hash_bytes(harness.app.selected_content.as_str().as_bytes()),
        language_hint: "py".to_string(),
        theme_key: "base16-mocha.dark".to_string(),
        lines: Vec::new(),
    });

    let mut paste = Paste::new("content".to_string(), "Alpha".to_string());
    paste.id = "alpha".to_string();
    harness.app.apply_event(CoreEvent::PasteSaved { paste });

    assert!(harness.app.highlight_render.is_some());
    assert_eq!(harness.app.highlight_version, 7);
}
