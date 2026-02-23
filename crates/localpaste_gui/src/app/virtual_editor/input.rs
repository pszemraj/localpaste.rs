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
    MoveHome { select: bool },
    MoveEnd { select: bool },
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
        egui::Key::A => Some(VirtualInputCommand::MoveHome { select }),
        egui::Key::E => Some(VirtualInputCommand::MoveEnd { select }),
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
                    Some(VirtualInputCommand::MoveHome { select })
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
                    Some(VirtualInputCommand::MoveEnd { select })
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

        // --- Line boundaries (Home/End keys) ---
        egui::Key::Home => match platform {
            PlatformFlavor::Mac => {
                if modifiers.command {
                    Some(VirtualInputCommand::MoveDocHome { select })
                } else {
                    Some(VirtualInputCommand::MoveHome { select })
                }
            }
            PlatformFlavor::Other => {
                if modifiers.ctrl {
                    Some(VirtualInputCommand::MoveDocHome { select })
                } else {
                    Some(VirtualInputCommand::MoveHome { select })
                }
            }
        },
        egui::Key::End => match platform {
            PlatformFlavor::Mac => {
                if modifiers.command {
                    Some(VirtualInputCommand::MoveDocEnd { select })
                } else {
                    Some(VirtualInputCommand::MoveEnd { select })
                }
            }
            PlatformFlavor::Other => {
                if modifiers.ctrl {
                    Some(VirtualInputCommand::MoveDocEnd { select })
                } else {
                    Some(VirtualInputCommand::MoveEnd { select })
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
/// - `focused`: Whether the virtual editor currently owns keyboard focus.
///
/// # Returns
/// Ordered command list derived from `events`.
pub(crate) fn commands_from_events(
    events: &[egui::Event],
    focused: bool,
) -> Vec<VirtualInputCommand> {
    commands_from_events_for_platform(events, focused, PlatformFlavor::current())
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
                if !emitted_copy {
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
                if modifiers.command {
                    match key {
                        egui::Key::A if focused => out.push(VirtualInputCommand::SelectAll),
                        egui::Key::C if !emitted_copy => {
                            out.push(VirtualInputCommand::Copy);
                            emitted_copy = true;
                        }
                        egui::Key::X if focused && !emitted_cut => {
                            out.push(VirtualInputCommand::Cut);
                            emitted_cut = true;
                        }
                        egui::Key::Z if focused && modifiers.shift => {
                            out.push(VirtualInputCommand::Redo)
                        }
                        egui::Key::Z if focused => out.push(VirtualInputCommand::Undo),
                        egui::Key::Y if focused => out.push(VirtualInputCommand::Redo),
                        _ => {}
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
                    if focused {
                        out.push(cmd);
                    } else if should_emit_when_unfocused(&cmd) {
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
mod tests {
    use super::*;

    fn key_event(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    #[test]
    fn maps_command_shortcuts() {
        let events = vec![key_event(
            egui::Key::A,
            egui::Modifiers {
                command: true,
                ..Default::default()
            },
        )];
        let commands = commands_from_events_for_platform(&events, true, PlatformFlavor::Other);
        assert_eq!(commands, vec![VirtualInputCommand::SelectAll]);
    }

    #[test]
    fn does_not_swallow_ctrl_navigation_on_non_mac() {
        // On Win/Linux egui sets BOTH `ctrl` and `command` when Ctrl is held.
        let events = vec![key_event(
            egui::Key::ArrowLeft,
            egui::Modifiers {
                ctrl: true,
                command: true,
                ..Default::default()
            },
        )];
        let commands = commands_from_events_for_platform(&events, true, PlatformFlavor::Other);
        assert_eq!(
            commands,
            vec![VirtualInputCommand::MoveLeft {
                select: false,
                word: true,
            }]
        );
    }

    #[test]
    fn maps_option_word_movement_on_mac() {
        let events = vec![key_event(
            egui::Key::ArrowRight,
            egui::Modifiers {
                alt: true,
                ..Default::default()
            },
        )];
        let commands = commands_from_events_for_platform(&events, true, PlatformFlavor::Mac);
        assert_eq!(
            commands,
            vec![VirtualInputCommand::MoveRight {
                select: false,
                word: true,
            }]
        );
    }

    #[test]
    fn maps_cmd_line_and_doc_navigation_on_mac() {
        let events = vec![
            key_event(
                egui::Key::ArrowLeft,
                egui::Modifiers {
                    command: true,
                    ..Default::default()
                },
            ),
            key_event(
                egui::Key::ArrowUp,
                egui::Modifiers {
                    command: true,
                    ..Default::default()
                },
            ),
        ];
        let commands = commands_from_events_for_platform(&events, true, PlatformFlavor::Mac);
        assert_eq!(
            commands,
            vec![
                VirtualInputCommand::MoveHome { select: false },
                VirtualInputCommand::MoveDocHome { select: false },
            ]
        );
    }

    #[test]
    fn maps_ctrl_home_end_to_doc_on_non_mac() {
        let events = vec![
            key_event(
                egui::Key::Home,
                egui::Modifiers {
                    ctrl: true,
                    command: true,
                    ..Default::default()
                },
            ),
            key_event(
                egui::Key::End,
                egui::Modifiers {
                    ctrl: true,
                    command: true,
                    ..Default::default()
                },
            ),
        ];
        let commands = commands_from_events_for_platform(&events, true, PlatformFlavor::Other);
        assert_eq!(
            commands,
            vec![
                VirtualInputCommand::MoveDocHome { select: false },
                VirtualInputCommand::MoveDocEnd { select: false },
            ]
        );
    }

    #[test]
    fn maps_macos_delete_to_line_start_and_end() {
        let events = vec![
            // Cmd+Backspace => delete to line start.
            key_event(
                egui::Key::Backspace,
                egui::Modifiers {
                    command: true,
                    ..Default::default()
                },
            ),
            // Ctrl+K => delete to line end.
            key_event(
                egui::Key::K,
                egui::Modifiers {
                    ctrl: true,
                    ..Default::default()
                },
            ),
            // Cmd+Delete => delete to line end.
            key_event(
                egui::Key::Delete,
                egui::Modifiers {
                    command: true,
                    ..Default::default()
                },
            ),
        ];
        let commands = commands_from_events_for_platform(&events, true, PlatformFlavor::Mac);
        assert_eq!(
            commands,
            vec![
                VirtualInputCommand::DeleteToLineStart,
                VirtualInputCommand::DeleteToLineEnd,
                VirtualInputCommand::DeleteToLineEnd,
            ]
        );
    }

    #[test]
    fn maps_ime_events() {
        let events = vec![
            egui::Event::Ime(egui::ImeEvent::Enabled),
            egui::Event::Ime(egui::ImeEvent::Preedit("に".to_string())),
            egui::Event::Ime(egui::ImeEvent::Commit("日".to_string())),
            egui::Event::Ime(egui::ImeEvent::Disabled),
        ];
        let commands = commands_from_events_for_platform(&events, true, PlatformFlavor::Other);
        assert_eq!(
            commands,
            vec![
                VirtualInputCommand::ImeEnabled,
                VirtualInputCommand::ImePreedit("に".to_string()),
                VirtualInputCommand::ImeCommit("日".to_string()),
                VirtualInputCommand::ImeDisabled,
            ]
        );
    }

    #[test]
    fn maps_copy_and_cut_events() {
        let events = vec![egui::Event::Copy, egui::Event::Cut];
        let commands = commands_from_events_for_platform(&events, true, PlatformFlavor::Other);
        assert_eq!(
            commands,
            vec![VirtualInputCommand::Copy, VirtualInputCommand::Cut]
        );
    }

    #[test]
    fn copy_is_emitted_even_without_focus() {
        let events = vec![egui::Event::Copy];
        let commands = commands_from_events_for_platform(&events, false, PlatformFlavor::Other);
        assert_eq!(commands, vec![VirtualInputCommand::Copy]);
    }

    #[test]
    fn routes_copy_as_copy_only() {
        assert_eq!(
            VirtualInputCommand::Copy.route(),
            VirtualCommandRoute::CopyOnly
        );
    }

    #[test]
    fn routes_mutating_commands_as_focus_required() {
        let commands = [
            VirtualInputCommand::InsertText("x".to_string()),
            VirtualInputCommand::InsertNewline,
            VirtualInputCommand::MoveLeft {
                select: false,
                word: false,
            },
            VirtualInputCommand::MoveDocHome { select: false },
            VirtualInputCommand::DeleteToLineStart,
            VirtualInputCommand::Cut,
            VirtualInputCommand::Paste("x".to_string()),
            VirtualInputCommand::Undo,
            VirtualInputCommand::ImeCommit("x".to_string()),
        ];
        for command in commands {
            assert_eq!(command.route(), VirtualCommandRoute::FocusRequired);
        }
    }

    #[test]
    fn marks_cut_and_paste_as_post_focus_only() {
        assert!(VirtualInputCommand::Cut.requires_post_focus());
        assert!(VirtualInputCommand::Paste("x".to_string()).requires_post_focus());
        assert!(!VirtualInputCommand::Copy.requires_post_focus());
        assert!(!VirtualInputCommand::InsertText("x".to_string()).requires_post_focus());
    }
}
