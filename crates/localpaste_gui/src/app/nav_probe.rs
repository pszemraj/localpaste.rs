//! Environment-gated navigation probe for real keyboard focus and caret behavior.

use super::{
    commands_from_events, LocalPasteApp, VirtualInputCommand, COMMAND_PALETTE_INPUT_ID,
    DIFF_QUERY_INPUT_ID, NAV_PROBE_PASTE_ID, PROPERTIES_NAME_INPUT_ID, PROPERTIES_TAGS_INPUT_ID,
    SEARCH_INPUT_ID, TITLE_INPUT_ID, VIRTUAL_EDITOR_ID,
};
use eframe::egui;
use localpaste_core::models::paste::Paste;
use serde::Serialize;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;
use tracing::warn;

#[derive(Debug)]
/// File-backed recorder for local navigation probe frames.
pub(super) struct NavProbe {
    file: File,
    frame_index: u64,
    scenario: Option<String>,
    started_at: Instant,
    current_raw_events: Vec<ProbeEvent>,
    current_candidate_commands_if_editor_focused: Vec<String>,
}

impl NavProbe {
    /// Creates a navigation probe from `LOCALPASTE_NAV_PROBE_*` environment variables when logging is enabled.
    ///
    /// # Returns
    /// `Some(NavProbe)` when `LOCALPASTE_NAV_PROBE_LOG` names a writable file, otherwise `None`.
    pub(super) fn from_env() -> Option<Self> {
        let raw_path = std::env::var("LOCALPASTE_NAV_PROBE_LOG").ok()?;
        let trimmed = raw_path.trim();
        if trimmed.is_empty() {
            return None;
        }
        let path = PathBuf::from(trimmed);
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            if let Err(err) = std::fs::create_dir_all(parent) {
                warn!("failed to create LOCALPASTE_NAV_PROBE_LOG parent: {err}");
                return None;
            }
        }
        let file = match OpenOptions::new().create(true).append(true).open(&path) {
            Ok(file) => file,
            Err(err) => {
                warn!(
                    "failed to open LOCALPASTE_NAV_PROBE_LOG at {}: {err}",
                    path.display()
                );
                return None;
            }
        };
        Some(Self {
            file,
            frame_index: 0,
            scenario: std::env::var("LOCALPASTE_NAV_PROBE_SCENARIO")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            started_at: Instant::now(),
            current_raw_events: Vec::new(),
            current_candidate_commands_if_editor_focused: Vec::new(),
        })
    }

    fn capture_begin_frame(&mut self, ctx: &egui::Context) {
        let (raw_events, candidate_commands_if_editor_focused) = ctx.input(|input| {
            let raw_events = input.events.iter().map(describe_event).collect::<Vec<_>>();
            let candidate = commands_from_events(&input.events, true)
                .iter()
                .map(|command| format!("{command:?}"))
                .collect::<Vec<_>>();
            (raw_events, candidate)
        });
        self.current_raw_events = raw_events;
        self.current_candidate_commands_if_editor_focused = candidate_commands_if_editor_focused;
    }

    fn write_frame(&mut self, frame: &NavProbeFrame) {
        if let Err(err) = serde_json::to_writer(&mut self.file, frame) {
            warn!("failed to serialize navigation probe frame: {err}");
            return;
        }
        if let Err(err) = self.file.write_all(b"\n").and_then(|_| self.file.flush()) {
            warn!("failed to write navigation probe frame: {err}");
        }
        self.frame_index = self.frame_index.saturating_add(1);
    }
}

#[derive(Debug, Serialize)]
struct NavProbeFrame {
    event: &'static str,
    scenario: Option<String>,
    platform: &'static str,
    frame_index: u64,
    elapsed_ms: u128,
    raw_events: Vec<ProbeEvent>,
    candidate_commands_if_editor_focused: Vec<String>,
    applied_commands: Vec<String>,
    focus: FocusSnapshot,
    cursor: CursorSnapshot,
    selection: Option<RangeSnapshot>,
    editor: EditorSnapshot,
    app: AppSnapshot,
}

#[derive(Clone, Debug, Serialize)]
struct ProbeEvent {
    kind: String,
    key: Option<String>,
    physical_key: Option<String>,
    pressed: Option<bool>,
    repeat: Option<bool>,
    modifiers: ModifierSnapshot,
    text_chars: Option<usize>,
}

#[derive(Clone, Debug, Default, Serialize)]
struct ModifierSnapshot {
    alt: bool,
    ctrl: bool,
    shift: bool,
    command: bool,
    mac_cmd: bool,
}

#[derive(Debug, Serialize)]
struct FocusSnapshot {
    virtual_editor: bool,
    sidebar_search: bool,
    editor_title: bool,
    command_palette_query: bool,
    properties_name: bool,
    properties_tags: bool,
    diff_query: bool,
    wants_keyboard_input: bool,
}

#[derive(Debug, Serialize)]
struct CursorSnapshot {
    char_index: usize,
    line: usize,
    col: usize,
    buffer_len_chars: usize,
}

#[derive(Debug, Serialize)]
struct RangeSnapshot {
    start: usize,
    end: usize,
}

#[derive(Debug, Serialize)]
struct EditorSnapshot {
    buffer_hash: String,
    buffer_len_chars: usize,
    buffer_revision: u64,
    viewport_height: f32,
    line_height: f32,
    wrap_width: f32,
    pending_scroll_offset_y: Option<f32>,
    follow_cursor_next_frame: bool,
}

#[derive(Debug, Serialize)]
struct AppSnapshot {
    selected_id: Option<String>,
    search_query_len: usize,
    search_query_hash: String,
    edit_name_len: usize,
    edit_name_hash: String,
    edit_tags_len: usize,
    edit_tags_hash: String,
    command_palette_query_len: usize,
    command_palette_query_hash: String,
    command_palette_open: bool,
    properties_drawer_open: bool,
    shortcut_help_open: bool,
    history_modal_open: bool,
    diff_modal_open: bool,
}

impl LocalPasteApp {
    /// Captures raw input and clears per-frame command state before UI code can consume keyboard events.
    pub(super) fn nav_probe_begin_frame(&mut self, ctx: &egui::Context) {
        self.nav_probe_applied_commands.clear();
        if let Some(probe) = self.nav_probe.as_mut() {
            probe.capture_begin_frame(ctx);
        }
    }

    /// Records virtual-editor commands extracted for the focused editor in the current frame.
    pub(super) fn nav_probe_record_applied_commands(&mut self, commands: &[VirtualInputCommand]) {
        if self.nav_probe.is_none() {
            return;
        }
        self.nav_probe_applied_commands = commands
            .iter()
            .map(|command| format!("{command:?}"))
            .collect();
    }

    /// Writes the final focus, caret, selection, and app snapshot for the current frame.
    pub(super) fn nav_probe_write_frame(&mut self, ctx: &egui::Context) {
        if self.nav_probe.is_none() {
            return;
        }
        let frame = self.build_nav_probe_frame(ctx);
        if let Some(probe) = self.nav_probe.as_mut() {
            probe.write_frame(&frame);
        }
    }

    /// Seeds an in-memory probe paste from environment variables and returns whether a seed was applied.
    ///
    /// # Returns
    /// `true` when `LOCALPASTE_NAV_PROBE_SEED_TEXT` was present and app state was seeded, otherwise `false`.
    pub(super) fn apply_nav_probe_seed_from_env(&mut self) -> bool {
        let Ok(text) = std::env::var("LOCALPASTE_NAV_PROBE_SEED_TEXT") else {
            return false;
        };
        let name = std::env::var("LOCALPASTE_NAV_PROBE_SEED_NAME")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "Navigation probe paste".to_string());

        let mut paste = Paste::new(text.clone(), name.clone());
        paste.id = NAV_PROBE_PASTE_ID.to_string();
        paste.language = None;
        paste.language_is_manual = false;
        paste.tags.clear();

        self.selected_id = Some(NAV_PROBE_PASTE_ID.to_string());
        self.selected_paste = Some(paste);
        self.edit_name = name;
        self.edit_language = None;
        self.edit_language_is_manual = false;
        self.edit_tags.clear();
        self.metadata_dirty = false;
        self.metadata_save_in_flight = false;
        self.metadata_save_request = None;
        self.search_query.clear();
        self.command_palette_query.clear();
        self.command_palette_open = false;
        self.properties_drawer_open = false;
        self.version_ui.history_modal_open = false;
        self.version_ui.history_reset_confirm_open = false;
        self.version_ui.history_reset_confirm_target = None;
        self.version_ui.diff_modal_open = false;
        self.version_ui.diff_query.clear();
        self.version_ui.diff_target_id = None;
        self.version_ui.diff_target_paste = None;
        self.version_ui.diff_loading_target = false;
        self.reset_virtual_editor(text.as_str());

        if env_truthy("LOCALPASTE_NAV_PROBE_FOCUS_EDITOR") {
            self.focus_editor_next = true;
        }
        true
    }

    fn build_nav_probe_frame(&self, ctx: &egui::Context) -> NavProbeFrame {
        let probe = self.nav_probe.as_ref().expect("checked by caller");

        let wants_keyboard_input = ctx.wants_keyboard_input();
        let focus = ctx.memory(|memory| FocusSnapshot {
            virtual_editor: memory.has_focus(egui::Id::new(VIRTUAL_EDITOR_ID)),
            sidebar_search: memory.has_focus(egui::Id::new(SEARCH_INPUT_ID)),
            editor_title: memory.has_focus(egui::Id::new(TITLE_INPUT_ID)),
            command_palette_query: memory.has_focus(egui::Id::new(COMMAND_PALETTE_INPUT_ID)),
            properties_name: memory.has_focus(egui::Id::new(PROPERTIES_NAME_INPUT_ID)),
            properties_tags: memory.has_focus(egui::Id::new(PROPERTIES_TAGS_INPUT_ID)),
            diff_query: memory.has_focus(egui::Id::new(DIFF_QUERY_INPUT_ID)),
            wants_keyboard_input,
        });

        let buffer_len_chars = self.virtual_editor_buffer.len_chars();
        let cursor_char = self.virtual_editor_state.cursor().min(buffer_len_chars);
        let (line, col) = self.virtual_editor_buffer.char_to_line_col(cursor_char);
        let selection = self
            .virtual_editor_state
            .selection_range()
            .map(|range| RangeSnapshot {
                start: range.start,
                end: range.end,
            });
        let buffer_snapshot = self.virtual_editor_buffer.to_string();

        NavProbeFrame {
            event: "nav_probe_frame",
            scenario: probe.scenario.clone(),
            platform: platform_name(),
            frame_index: probe.frame_index,
            elapsed_ms: probe.started_at.elapsed().as_millis(),
            raw_events: probe.current_raw_events.clone(),
            candidate_commands_if_editor_focused: probe
                .current_candidate_commands_if_editor_focused
                .clone(),
            applied_commands: self.nav_probe_applied_commands.clone(),
            focus,
            cursor: CursorSnapshot {
                char_index: cursor_char,
                line,
                col,
                buffer_len_chars,
            },
            selection,
            editor: EditorSnapshot {
                buffer_hash: stable_hash(buffer_snapshot.as_str()),
                buffer_len_chars,
                buffer_revision: self.virtual_editor_buffer.revision(),
                viewport_height: self.virtual_viewport_height,
                line_height: self.virtual_line_height,
                wrap_width: self.virtual_wrap_width,
                pending_scroll_offset_y: self.virtual_pending_scroll_offset_y,
                follow_cursor_next_frame: self.virtual_follow_cursor_next_frame,
            },
            app: AppSnapshot {
                selected_id: self.selected_id.clone(),
                search_query_len: self.search_query.chars().count(),
                search_query_hash: stable_hash(self.search_query.as_str()),
                edit_name_len: self.edit_name.chars().count(),
                edit_name_hash: stable_hash(self.edit_name.as_str()),
                edit_tags_len: self.edit_tags.chars().count(),
                edit_tags_hash: stable_hash(self.edit_tags.as_str()),
                command_palette_query_len: self.command_palette_query.chars().count(),
                command_palette_query_hash: stable_hash(self.command_palette_query.as_str()),
                command_palette_open: self.command_palette_open,
                properties_drawer_open: self.properties_drawer_open,
                shortcut_help_open: self.shortcut_help_open,
                history_modal_open: self.version_ui.history_modal_open,
                diff_modal_open: self.version_ui.diff_modal_open,
            },
        }
    }
}

fn describe_event(event: &egui::Event) -> ProbeEvent {
    match event {
        egui::Event::Key {
            key,
            physical_key,
            pressed,
            repeat,
            modifiers,
        } => ProbeEvent {
            kind: "key".to_string(),
            key: Some(format!("{key:?}")),
            physical_key: physical_key.map(|key| format!("{key:?}")),
            pressed: Some(*pressed),
            repeat: Some(*repeat),
            modifiers: describe_modifiers(*modifiers),
            text_chars: None,
        },
        egui::Event::Text(text) => ProbeEvent {
            kind: "text".to_string(),
            key: None,
            physical_key: None,
            pressed: None,
            repeat: None,
            modifiers: ModifierSnapshot::default(),
            text_chars: Some(text.chars().count()),
        },
        egui::Event::Paste(text) => ProbeEvent {
            kind: "paste".to_string(),
            key: None,
            physical_key: None,
            pressed: None,
            repeat: None,
            modifiers: ModifierSnapshot::default(),
            text_chars: Some(text.chars().count()),
        },
        other => ProbeEvent {
            kind: format!("{other:?}"),
            key: None,
            physical_key: None,
            pressed: None,
            repeat: None,
            modifiers: ModifierSnapshot::default(),
            text_chars: None,
        },
    }
}

fn describe_modifiers(modifiers: egui::Modifiers) -> ModifierSnapshot {
    ModifierSnapshot {
        alt: modifiers.alt,
        ctrl: modifiers.ctrl,
        shift: modifiers.shift,
        command: modifiers.command,
        mac_cmd: modifiers.mac_cmd,
    }
}

fn env_truthy(name: &str) -> bool {
    std::env::var(name)
        .map(|value| {
            let trimmed = value.trim();
            trimmed == "1" || trimmed.eq_ignore_ascii_case("true")
        })
        .unwrap_or(false)
}

fn stable_hash(value: &str) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn platform_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "other"
    }
}
