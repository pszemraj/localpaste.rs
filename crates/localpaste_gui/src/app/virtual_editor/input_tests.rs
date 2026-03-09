//! Regression tests for virtual-editor input translation and platform keymaps.

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
            ctrl: true,
            ..Default::default()
        },
    )];
    let commands = commands_from_events_for_platform(&events, true, PlatformFlavor::Other);
    assert_eq!(commands, vec![VirtualInputCommand::SelectAll]);
}

#[test]
fn rejects_extra_shift_or_alt_on_primary_shortcuts_non_mac() {
    let cases = [
        key_event(
            egui::Key::A,
            egui::Modifiers {
                command: true,
                ctrl: true,
                shift: true,
                ..Default::default()
            },
        ),
        key_event(
            egui::Key::C,
            egui::Modifiers {
                command: true,
                ctrl: true,
                alt: true,
                ..Default::default()
            },
        ),
        key_event(
            egui::Key::X,
            egui::Modifiers {
                command: true,
                ctrl: true,
                shift: true,
                ..Default::default()
            },
        ),
        key_event(
            egui::Key::Y,
            egui::Modifiers {
                command: true,
                ctrl: true,
                shift: true,
                ..Default::default()
            },
        ),
    ];

    for event in cases {
        let commands = commands_from_events_for_platform(&[event], true, PlatformFlavor::Other);
        assert!(commands.is_empty());
    }
}

#[test]
fn rejects_extra_shift_alt_or_ctrl_on_primary_shortcuts_mac() {
    let cases = [
        key_event(
            egui::Key::A,
            egui::Modifiers {
                command: true,
                shift: true,
                ..Default::default()
            },
        ),
        key_event(
            egui::Key::C,
            egui::Modifiers {
                command: true,
                alt: true,
                ..Default::default()
            },
        ),
        key_event(
            egui::Key::X,
            egui::Modifiers {
                command: true,
                ctrl: true,
                ..Default::default()
            },
        ),
    ];

    for event in cases {
        let commands = commands_from_events_for_platform(&[event], true, PlatformFlavor::Mac);
        assert!(commands.is_empty());
    }
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

    let non_mac_commands = commands_from_events_for_platform(&events, true, PlatformFlavor::Other);
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
        key_event(
            egui::Key::Backspace,
            egui::Modifiers {
                command: true,
                ..Default::default()
            },
        ),
        key_event(
            egui::Key::K,
            egui::Modifiers {
                ctrl: true,
                ..Default::default()
            },
        ),
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
                ctrl: true,
                command: true,
                ..Default::default()
            },
        },
        egui::Event::Copy,
    ];
    assert!(commands_from_events_for_platform(&events, false, PlatformFlavor::Other).is_empty());
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
                ctrl: true,
                command: true,
                ..Default::default()
            },
        ),
        egui::Event::Copy,
        key_event(
            egui::Key::X,
            egui::Modifiers {
                ctrl: true,
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
