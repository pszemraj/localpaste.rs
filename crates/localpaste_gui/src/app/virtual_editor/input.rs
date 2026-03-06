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

/// Maps primary command shortcuts (`Cmd`/`Ctrl`) to editor commands.
/// Shared by event extraction and fallback routing.
///
/// # Arguments
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
    if !modifiers.command {
        return None;
    }
    match key {
        egui::Key::A => Some(VirtualInputCommand::SelectAll),
        egui::Key::C => Some(VirtualInputCommand::Copy),
        egui::Key::X => Some(VirtualInputCommand::Cut),
        egui::Key::Z if modifiers.shift => Some(VirtualInputCommand::Redo),
        egui::Key::Z => Some(VirtualInputCommand::Undo),
        egui::Key::Y => Some(VirtualInputCommand::Redo),
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
                    if let Some(command) = map_primary_command_shortcut(*key, *modifiers) {
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
                VirtualInputCommand::MoveLineHome { select: false },
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
    fn maps_shift_selection_navigation_variants_non_mac() {
        let events = vec![
            key_event(
                egui::Key::ArrowLeft,
                egui::Modifiers {
                    ctrl: true,
                    command: true,
                    shift: true,
                    ..Default::default()
                },
            ),
            key_event(
                egui::Key::Home,
                egui::Modifiers {
                    ctrl: true,
                    command: true,
                    shift: true,
                    ..Default::default()
                },
            ),
            key_event(
                egui::Key::End,
                egui::Modifiers {
                    ctrl: true,
                    command: true,
                    shift: true,
                    ..Default::default()
                },
            ),
        ];
        let commands = commands_from_events_for_platform(&events, true, PlatformFlavor::Other);
        assert_eq!(
            commands,
            vec![
                VirtualInputCommand::MoveLeft {
                    select: true,
                    word: true,
                },
                VirtualInputCommand::MoveDocHome { select: true },
                VirtualInputCommand::MoveDocEnd { select: true },
            ]
        );
    }

    #[test]
    fn maps_shift_selection_navigation_variants_mac() {
        let events = vec![
            key_event(
                egui::Key::ArrowRight,
                egui::Modifiers {
                    alt: true,
                    shift: true,
                    ..Default::default()
                },
            ),
            key_event(
                egui::Key::ArrowLeft,
                egui::Modifiers {
                    command: true,
                    shift: true,
                    ..Default::default()
                },
            ),
            key_event(
                egui::Key::ArrowDown,
                egui::Modifiers {
                    command: true,
                    shift: true,
                    ..Default::default()
                },
            ),
        ];
        let commands = commands_from_events_for_platform(&events, true, PlatformFlavor::Mac);
        assert_eq!(
            commands,
            vec![
                VirtualInputCommand::MoveRight {
                    select: true,
                    word: true,
                },
                VirtualInputCommand::MoveLineHome { select: true },
                VirtualInputCommand::MoveDocEnd { select: true },
            ]
        );
    }

    fn assert_platform_commands_matrix(
        cases: &[(Vec<egui::Event>, PlatformFlavor, Vec<VirtualInputCommand>)],
    ) {
        for (events, platform, expected) in cases {
            let commands = commands_from_events_for_platform(events, true, *platform);
            assert_eq!(commands, *expected);
        }
    }

    #[test]
    fn maps_home_end_without_modifiers_to_doc_moves_on_mac_and_line_moves_elsewhere() {
        let events = vec![
            key_event(egui::Key::Home, egui::Modifiers::default()),
            key_event(egui::Key::End, egui::Modifiers::default()),
        ];
        let mac_commands = commands_from_events_for_platform(&events, true, PlatformFlavor::Mac);
        assert_eq!(
            mac_commands,
            vec![
                VirtualInputCommand::MoveDocHome { select: false },
                VirtualInputCommand::MoveDocEnd { select: false },
            ]
        );

        let non_mac_commands =
            commands_from_events_for_platform(&events, true, PlatformFlavor::Other);
        assert_eq!(
            non_mac_commands,
            vec![
                VirtualInputCommand::MoveLineHome { select: false },
                VirtualInputCommand::MoveLineEnd { select: false },
            ]
        );
    }

    #[test]
    fn maps_shift_home_end_to_line_selection_on_non_mac() {
        let events = vec![
            key_event(
                egui::Key::Home,
                egui::Modifiers {
                    shift: true,
                    ..Default::default()
                },
            ),
            key_event(
                egui::Key::End,
                egui::Modifiers {
                    shift: true,
                    ..Default::default()
                },
            ),
        ];
        let commands = commands_from_events_for_platform(&events, true, PlatformFlavor::Other);
        assert_eq!(
            commands,
            vec![
                VirtualInputCommand::MoveLineHome { select: true },
                VirtualInputCommand::MoveLineEnd { select: true },
            ]
        );
    }

    #[test]
    fn maps_shift_home_end_to_doc_selection_on_mac() {
        let events = vec![
            key_event(
                egui::Key::Home,
                egui::Modifiers {
                    shift: true,
                    ..Default::default()
                },
            ),
            key_event(
                egui::Key::End,
                egui::Modifiers {
                    shift: true,
                    ..Default::default()
                },
            ),
        ];
        let commands = commands_from_events_for_platform(&events, true, PlatformFlavor::Mac);
        assert_eq!(
            commands,
            vec![
                VirtualInputCommand::MoveDocHome { select: true },
                VirtualInputCommand::MoveDocEnd { select: true },
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
    fn copy_is_emitted_only_with_focus() {
        let events = vec![
            egui::Event::Key {
                key: egui::Key::C,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers {
                    command: true,
                    ..Default::default()
                },
            },
            egui::Event::Copy,
        ];
        assert!(
            commands_from_events_for_platform(&events, false, PlatformFlavor::Other).is_empty()
        );
        let commands = commands_from_events_for_platform(&events, true, PlatformFlavor::Other);
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
            VirtualInputCommand::MoveLineHome { select: false },
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

    #[test]
    fn dedupes_copy_and_cut_from_key_and_event_streams() {
        let events = vec![
            key_event(
                egui::Key::C,
                egui::Modifiers {
                    command: true,
                    ..Default::default()
                },
            ),
            egui::Event::Copy,
            key_event(
                egui::Key::X,
                egui::Modifiers {
                    command: true,
                    ..Default::default()
                },
            ),
            egui::Event::Cut,
        ];
        let commands = commands_from_events_for_platform(&events, true, PlatformFlavor::Other);
        assert_eq!(
            commands,
            vec![VirtualInputCommand::Copy, VirtualInputCommand::Cut]
        );
    }

    #[test]
    fn platform_specific_word_delete_and_vertical_selection_mappings() {
        let cases = vec![
            (
                vec![
                    key_event(
                        egui::Key::Backspace,
                        egui::Modifiers {
                            ctrl: true,
                            command: true,
                            ..Default::default()
                        },
                    ),
                    key_event(
                        egui::Key::Delete,
                        egui::Modifiers {
                            ctrl: true,
                            command: true,
                            ..Default::default()
                        },
                    ),
                ],
                PlatformFlavor::Other,
                vec![
                    VirtualInputCommand::Backspace { word: true },
                    VirtualInputCommand::DeleteForward { word: true },
                ],
            ),
            (
                vec![
                    key_event(
                        egui::Key::Backspace,
                        egui::Modifiers {
                            alt: true,
                            ..Default::default()
                        },
                    ),
                    key_event(
                        egui::Key::Delete,
                        egui::Modifiers {
                            alt: true,
                            ..Default::default()
                        },
                    ),
                ],
                PlatformFlavor::Mac,
                vec![
                    VirtualInputCommand::Backspace { word: true },
                    VirtualInputCommand::DeleteForward { word: true },
                ],
            ),
            (
                vec![
                    key_event(
                        egui::Key::ArrowUp,
                        egui::Modifiers {
                            shift: true,
                            ..Default::default()
                        },
                    ),
                    key_event(
                        egui::Key::ArrowDown,
                        egui::Modifiers {
                            shift: true,
                            ..Default::default()
                        },
                    ),
                ],
                PlatformFlavor::Other,
                vec![
                    VirtualInputCommand::MoveUp { select: true },
                    VirtualInputCommand::MoveDown { select: true },
                ],
            ),
            (
                vec![
                    key_event(
                        egui::Key::ArrowUp,
                        egui::Modifiers {
                            command: true,
                            shift: true,
                            ..Default::default()
                        },
                    ),
                    key_event(
                        egui::Key::ArrowDown,
                        egui::Modifiers {
                            shift: true,
                            ..Default::default()
                        },
                    ),
                ],
                PlatformFlavor::Mac,
                vec![
                    VirtualInputCommand::MoveDocHome { select: true },
                    VirtualInputCommand::MoveDown { select: true },
                ],
            ),
        ];
        assert_platform_commands_matrix(cases.as_slice());
    }

    #[test]
    fn unfocused_key_navigation_and_delete_are_dropped() {
        let events = vec![
            key_event(egui::Key::ArrowLeft, egui::Modifiers::default()),
            key_event(
                egui::Key::Delete,
                egui::Modifiers {
                    ctrl: true,
                    command: true,
                    ..Default::default()
                },
            ),
        ];
        let commands = commands_from_events_for_platform(&events, false, PlatformFlavor::Other);
        assert!(commands.is_empty());
    }
}
