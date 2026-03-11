//! Input-event reducer for the virtual editor.
//!
//! This module is intentionally the *single* place where platform-specific
//! modifier semantics (Cmd vs Ctrl vs Option/Alt) are translated into a small,
//! editor-centric command enum. The rest of the editor should only reason
//! about `VirtualInputCommand` and never branch on OS/modifier details.

use eframe::egui;

/// Normalized commands consumed by the virtual editor state machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum VirtualInputCommand {
    // Cursor movement
    MoveLeft { select: bool, word: bool },
    MoveRight { select: bool, word: bool },
    MoveUp { select: bool },
    MoveDown { select: bool },
    // Logical-line boundaries.
    MoveLineHome { select: bool },
    MoveLineEnd { select: bool },
    MoveDocHome { select: bool },
    MoveDocEnd { select: bool },
    PageUp { select: bool },
    PageDown { select: bool },

    // Deletion
    Backspace { word: bool },
    DeleteForward { word: bool },
    DeleteToLineStart,
    DeleteToLineEnd,

    // Insertion
    InsertText(String),
    InsertNewline,
    InsertTab,

    // Clipboard + history
    SelectAll,
    Copy,
    Cut,
    Paste(String),
    Undo,
    Redo,

    // IME
    ImeEnabled,
    ImePreedit(String),
    ImeCommit(String),
    ImeDisabled,
}

/// Routing bucket for virtual editor input handling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VirtualCommandRoute {
    CopyOnly,
    FocusRequired,
}

impl VirtualInputCommand {
    /// Returns the routing bucket used by app-level input gating.
    ///
    /// # Returns
    /// Which execution path the command should take in the app loop.
    pub(crate) fn route(&self) -> VirtualCommandRoute {
        match self {
            Self::Copy => VirtualCommandRoute::CopyOnly,
            _ => VirtualCommandRoute::FocusRequired,
        }
    }

    /// Returns true when the command should only run after post-UI focus is finalized.
    ///
    /// # Returns
    /// `true` for commands that depend on finalized widget focus state.
    pub(crate) fn requires_post_focus(&self) -> bool {
        matches!(self, Self::Cut | Self::Paste(_))
    }

    /// Returns whether the command should keep keyboard ownership on the editor.
    ///
    /// # Returns
    /// `true` when the command is part of native editor interaction flow.
    pub(crate) fn should_retain_editor_focus(&self) -> bool {
        !matches!(self, Self::InsertTab)
    }
}

/// Coarse platform flavor used for keyboard shortcut mapping.
///
/// We keep this extremely small so the translation logic stays auditable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PlatformFlavor {
    Mac,
    Other,
}

impl PlatformFlavor {
    const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Mac
        } else {
            Self::Other
        }
    }
}

fn is_word_modifier(platform: PlatformFlavor, modifiers: egui::Modifiers) -> bool {
    match platform {
        // macOS: word movement/deletion is Option (Alt).
        PlatformFlavor::Mac => modifiers.alt,
        // Windows/Linux/etc: word movement/deletion is Ctrl.
        PlatformFlavor::Other => modifiers.ctrl,
    }
}

fn is_mac_ctrl_chord(platform: PlatformFlavor, modifiers: egui::Modifiers) -> bool {
    platform == PlatformFlavor::Mac && modifiers.ctrl && !modifiers.command && !modifiers.alt
}

fn map_mac_ctrl_editing(key: egui::Key, modifiers: egui::Modifiers) -> Option<VirtualInputCommand> {
    // Cocoa text system Emacs-style bindings.
    // See Apple's shortcut table for text navigation.
    let select = modifiers.shift;

    match key {
        egui::Key::A => Some(VirtualInputCommand::MoveLineHome { select }),
        egui::Key::E => Some(VirtualInputCommand::MoveLineEnd { select }),
        egui::Key::B => Some(VirtualInputCommand::MoveLeft {
            select,
            word: false,
        }),
        egui::Key::F => Some(VirtualInputCommand::MoveRight {
            select,
            word: false,
        }),
        egui::Key::P => Some(VirtualInputCommand::MoveUp { select }),
        egui::Key::N => Some(VirtualInputCommand::MoveDown { select }),
        egui::Key::K => Some(VirtualInputCommand::DeleteToLineEnd),
        _ => None,
    }
}

fn is_primary_command_base(platform: PlatformFlavor, modifiers: egui::Modifiers) -> bool {
    if !modifiers.command || modifiers.alt {
        return false;
    }

    match platform {
        PlatformFlavor::Mac => !modifiers.ctrl,
        // On Win/Linux egui reports both `ctrl` and `command` for Ctrl chords.
        PlatformFlavor::Other => modifiers.ctrl,
    }
}

/// Maps primary command shortcuts (`Cmd`/`Ctrl`) to editor commands.
/// Shared by event extraction and fallback routing.
///
/// # Arguments
/// - `platform`: Active platform flavor.
/// - `key`: Pressed key.
/// - `modifiers`: Active modifier state for the key event.
///
/// # Returns
/// `Some(VirtualInputCommand)` when the chord maps to a primary editor
/// shortcut, otherwise `None`.
pub(crate) fn map_primary_command_shortcut(
    key: egui::Key,
    modifiers: egui::Modifiers,
) -> Option<VirtualInputCommand> {
    map_primary_command_shortcut_for_platform(PlatformFlavor::current(), key, modifiers)
}

fn map_primary_command_shortcut_for_platform(
    platform: PlatformFlavor,
    key: egui::Key,
    modifiers: egui::Modifiers,
) -> Option<VirtualInputCommand> {
    if !is_primary_command_base(platform, modifiers) {
        return None;
    }

    match key {
        egui::Key::A if !modifiers.shift => Some(VirtualInputCommand::SelectAll),
        egui::Key::C if !modifiers.shift => Some(VirtualInputCommand::Copy),
        egui::Key::X if !modifiers.shift => Some(VirtualInputCommand::Cut),
        egui::Key::Z if modifiers.shift => Some(VirtualInputCommand::Redo),
        egui::Key::Z => Some(VirtualInputCommand::Undo),
        egui::Key::Y if !modifiers.shift => Some(VirtualInputCommand::Redo),
        _ => None,
    }
}

fn map_navigation_key(
    platform: PlatformFlavor,
    key: egui::Key,
    modifiers: egui::Modifiers,
) -> Option<VirtualInputCommand> {
    let select = modifiers.shift;

    match key {
        // --- Horizontal arrows ---
        egui::Key::ArrowLeft => match platform {
            PlatformFlavor::Mac => {
                if modifiers.command {
                    Some(VirtualInputCommand::MoveLineHome { select })
                } else {
                    Some(VirtualInputCommand::MoveLeft {
                        select,
                        word: is_word_modifier(platform, modifiers),
                    })
                }
            }
            PlatformFlavor::Other => Some(VirtualInputCommand::MoveLeft {
                select,
                word: is_word_modifier(platform, modifiers),
            }),
        },
        egui::Key::ArrowRight => match platform {
            PlatformFlavor::Mac => {
                if modifiers.command {
                    Some(VirtualInputCommand::MoveLineEnd { select })
                } else {
                    Some(VirtualInputCommand::MoveRight {
                        select,
                        word: is_word_modifier(platform, modifiers),
                    })
                }
            }
            PlatformFlavor::Other => Some(VirtualInputCommand::MoveRight {
                select,
                word: is_word_modifier(platform, modifiers),
            }),
        },

        // --- Vertical arrows ---
        egui::Key::ArrowUp => match platform {
            PlatformFlavor::Mac => {
                if modifiers.command {
                    Some(VirtualInputCommand::MoveDocHome { select })
                } else {
                    Some(VirtualInputCommand::MoveUp { select })
                }
            }
            PlatformFlavor::Other => Some(VirtualInputCommand::MoveUp { select }),
        },
        egui::Key::ArrowDown => match platform {
            PlatformFlavor::Mac => {
                if modifiers.command {
                    Some(VirtualInputCommand::MoveDocEnd { select })
                } else {
                    Some(VirtualInputCommand::MoveDown { select })
                }
            }
            PlatformFlavor::Other => Some(VirtualInputCommand::MoveDown { select }),
        },

        // --- Line/document boundaries ---
        egui::Key::Home => match platform {
            // On macOS, physical Home/End (including Fn+Left/Fn+Right on compact
            // keyboards) are document-boundary keys. Line-boundary movement stays
            // on Cmd+Left/Cmd+Right.
            PlatformFlavor::Mac => Some(VirtualInputCommand::MoveDocHome { select }),
            PlatformFlavor::Other => {
                if modifiers.ctrl {
                    Some(VirtualInputCommand::MoveDocHome { select })
                } else {
                    Some(VirtualInputCommand::MoveLineHome { select })
                }
            }
        },
        egui::Key::End => match platform {
            PlatformFlavor::Mac => Some(VirtualInputCommand::MoveDocEnd { select }),
            PlatformFlavor::Other => {
                if modifiers.ctrl {
                    Some(VirtualInputCommand::MoveDocEnd { select })
                } else {
                    Some(VirtualInputCommand::MoveLineEnd { select })
                }
            }
        },

        // --- Paging ---
        egui::Key::PageUp => Some(VirtualInputCommand::PageUp { select }),
        egui::Key::PageDown => Some(VirtualInputCommand::PageDown { select }),

        // --- Deletion ---
        egui::Key::Backspace => match platform {
            PlatformFlavor::Mac => {
                if modifiers.command {
                    Some(VirtualInputCommand::DeleteToLineStart)
                } else {
                    Some(VirtualInputCommand::Backspace {
                        word: is_word_modifier(platform, modifiers),
                    })
                }
            }
            PlatformFlavor::Other => Some(VirtualInputCommand::Backspace {
                word: is_word_modifier(platform, modifiers),
            }),
        },
        egui::Key::Delete => match platform {
            PlatformFlavor::Mac => {
                if modifiers.command {
                    Some(VirtualInputCommand::DeleteToLineEnd)
                } else {
                    Some(VirtualInputCommand::DeleteForward {
                        word: is_word_modifier(platform, modifiers),
                    })
                }
            }
            PlatformFlavor::Other => Some(VirtualInputCommand::DeleteForward {
                word: is_word_modifier(platform, modifiers),
            }),
        },

        // --- Insertion ---
        egui::Key::Enter => Some(VirtualInputCommand::InsertNewline),
        egui::Key::Tab => Some(VirtualInputCommand::InsertTab),

        _ => None,
    }
}

fn should_emit_when_unfocused(command: &VirtualInputCommand) -> bool {
    matches!(command.route(), VirtualCommandRoute::CopyOnly)
}

/// Convert egui input events into virtual-editor commands.
///
/// # Arguments
/// - `events`: Raw egui events captured this frame.
/// - `focused`: Whether the virtual editor may claim this frame's shortcut/input commands.
///
/// # Returns
/// Ordered command list derived from `events`.
pub(crate) fn commands_from_events(
    events: &[egui::Event],
    focused: bool,
) -> Vec<VirtualInputCommand> {
    commands_from_events_for_platform(events, focused, PlatformFlavor::current())
}

/// Returns whether this frame contains editor-owned commands that should keep focus.
///
/// # Arguments
/// - `events`: Raw egui events captured for the frame.
///
/// # Returns
/// `true` when any event maps to a focus-retaining virtual-editor command.
pub(crate) fn frame_contains_focus_retaining_editor_command(events: &[egui::Event]) -> bool {
    commands_from_events_for_platform(events, true, PlatformFlavor::current())
        .into_iter()
        .any(|command| command.should_retain_editor_focus())
}

fn commands_from_events_for_platform(
    events: &[egui::Event],
    focused: bool,
    platform: PlatformFlavor,
) -> Vec<VirtualInputCommand> {
    let mut out: Vec<VirtualInputCommand> = Vec::new();

    // Some integrations can deliver both a `Key` event and a high-level
    // `Copy`/`Cut` event for the same physical key chord. Dedup within a frame.
    let mut emitted_copy = false;
    let mut emitted_cut = false;

    for event in events {
        match event {
            egui::Event::Text(text) => {
                if focused && !text.is_empty() {
                    out.push(VirtualInputCommand::InsertText(text.clone()));
                }
            }

            egui::Event::Paste(text) => {
                if focused {
                    out.push(VirtualInputCommand::Paste(text.clone()));
                }
            }

            egui::Event::Copy => {
                if focused && !emitted_copy {
                    out.push(VirtualInputCommand::Copy);
                    emitted_copy = true;
                }
            }

            egui::Event::Cut => {
                if focused && !emitted_cut {
                    out.push(VirtualInputCommand::Cut);
                    emitted_cut = true;
                }
            }

            egui::Event::Ime(ime) => {
                if !focused {
                    continue;
                }
                match ime {
                    egui::ImeEvent::Enabled => out.push(VirtualInputCommand::ImeEnabled),
                    egui::ImeEvent::Preedit(text) => {
                        out.push(VirtualInputCommand::ImePreedit(text.clone()))
                    }
                    egui::ImeEvent::Commit(text) => {
                        out.push(VirtualInputCommand::ImeCommit(text.clone()))
                    }
                    egui::ImeEvent::Disabled => out.push(VirtualInputCommand::ImeDisabled),
                }
            }

            egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => {
                // --- Primary command shortcuts (Cmd on macOS, Ctrl on Win/Linux) ---
                // IMPORTANT: Do *not* early-return/continue here.
                // In egui, `modifiers.command == modifiers.ctrl` on Win/Linux.
                // Early-returning would swallow Ctrl+Arrow, Ctrl+Backspace, etc.
                if focused {
                    if let Some(command) =
                        map_primary_command_shortcut_for_platform(platform, *key, *modifiers)
                    {
                        match command {
                            VirtualInputCommand::Copy if emitted_copy => {}
                            VirtualInputCommand::Copy => {
                                out.push(command);
                                emitted_copy = true;
                            }
                            VirtualInputCommand::Cut if emitted_cut => {}
                            VirtualInputCommand::Cut => {
                                out.push(command);
                                emitted_cut = true;
                            }
                            _ => out.push(command),
                        }
                    }
                }

                // --- macOS ctrl-key text navigation (Emacs heritage) ---
                if focused && is_mac_ctrl_chord(platform, *modifiers) {
                    if let Some(cmd) = map_mac_ctrl_editing(*key, *modifiers) {
                        out.push(cmd);
                        continue;
                    }
                }

                if let Some(cmd) = map_navigation_key(platform, *key, *modifiers) {
                    if focused || should_emit_when_unfocused(&cmd) {
                        out.push(cmd);
                    }
                }
            }

            _ => {}
        }
    }

    out
}

#[cfg(test)]
#[path = "input_tests.rs"]
mod tests;
