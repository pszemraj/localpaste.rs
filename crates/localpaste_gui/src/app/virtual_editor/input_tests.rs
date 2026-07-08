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

fn modifiers(ctrl: bool, command: bool, alt: bool, shift: bool) -> egui::Modifiers {
    egui::Modifiers {
        alt,
        ctrl,
        shift,
        command,
        ..Default::default()
    }
}

fn non_mac_ctrl() -> egui::Modifiers {
    modifiers(true, true, false, false)
}

fn non_mac_ctrl_shift() -> egui::Modifiers {
    modifiers(true, true, false, true)
}

fn mac_cmd() -> egui::Modifiers {
    modifiers(false, true, false, false)
}

fn mac_cmd_shift() -> egui::Modifiers {
    modifiers(false, true, false, true)
}

fn mac_option() -> egui::Modifiers {
    modifiers(false, false, true, false)
}

fn mac_option_shift() -> egui::Modifiers {
    modifiers(false, false, true, true)
}

fn mac_ctrl() -> egui::Modifiers {
    modifiers(true, false, false, false)
}

fn mac_ctrl_shift() -> egui::Modifiers {
    modifiers(true, false, false, true)
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

struct PlatformCommandCase {
    name: &'static str,
    events: Vec<egui::Event>,
    platform: PlatformFlavor,
    expected: Vec<VirtualInputCommand>,
}

fn assert_platform_commands_matrix(cases: &[PlatformCommandCase]) {
    for case in cases {
        let commands = commands_from_events_for_platform(&case.events, true, case.platform);
        assert_eq!(commands, case.expected, "case '{}'", case.name);
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
fn platform_keymap_matrix_covers_arrows_boundaries_deletion_and_insertion() {
    let shift = egui::Modifiers {
        shift: true,
        ..Default::default()
    };
    let cases = vec![
        PlatformCommandCase {
            name: "other plain arrows",
            events: vec![
                key_event(egui::Key::ArrowLeft, egui::Modifiers::default()),
                key_event(egui::Key::ArrowRight, egui::Modifiers::default()),
                key_event(egui::Key::ArrowUp, egui::Modifiers::default()),
                key_event(egui::Key::ArrowDown, egui::Modifiers::default()),
            ],
            platform: PlatformFlavor::Other,
            expected: vec![
                VirtualInputCommand::MoveLeft {
                    select: false,
                    word: false,
                },
                VirtualInputCommand::MoveRight {
                    select: false,
                    word: false,
                },
                VirtualInputCommand::MoveUp { select: false },
                VirtualInputCommand::MoveDown { select: false },
            ],
        },
        PlatformCommandCase {
            name: "other shift arrows",
            events: vec![
                key_event(egui::Key::ArrowLeft, shift),
                key_event(egui::Key::ArrowDown, shift),
            ],
            platform: PlatformFlavor::Other,
            expected: vec![
                VirtualInputCommand::MoveLeft {
                    select: true,
                    word: false,
                },
                VirtualInputCommand::MoveDown { select: true },
            ],
        },
        PlatformCommandCase {
            name: "other ctrl horizontal arrows are word movement",
            events: vec![
                key_event(egui::Key::ArrowLeft, non_mac_ctrl()),
                key_event(egui::Key::ArrowRight, non_mac_ctrl_shift()),
            ],
            platform: PlatformFlavor::Other,
            expected: vec![
                VirtualInputCommand::MoveLeft {
                    select: false,
                    word: true,
                },
                VirtualInputCommand::MoveRight {
                    select: true,
                    word: true,
                },
            ],
        },
        PlatformCommandCase {
            name: "other ctrl vertical arrows stay vertical",
            events: vec![
                key_event(egui::Key::ArrowUp, non_mac_ctrl()),
                key_event(egui::Key::ArrowDown, non_mac_ctrl_shift()),
            ],
            platform: PlatformFlavor::Other,
            expected: vec![
                VirtualInputCommand::MoveUp { select: false },
                VirtualInputCommand::MoveDown { select: true },
            ],
        },
        PlatformCommandCase {
            name: "other home end line and document boundaries",
            events: vec![
                key_event(egui::Key::Home, egui::Modifiers::default()),
                key_event(egui::Key::End, shift),
                key_event(egui::Key::Home, non_mac_ctrl()),
                key_event(egui::Key::End, non_mac_ctrl_shift()),
            ],
            platform: PlatformFlavor::Other,
            expected: vec![
                VirtualInputCommand::MoveLineHome { select: false },
                VirtualInputCommand::MoveLineEnd { select: true },
                VirtualInputCommand::MoveDocHome { select: false },
                VirtualInputCommand::MoveDocEnd { select: true },
            ],
        },
        PlatformCommandCase {
            name: "other paging deletion and insertion",
            events: vec![
                key_event(egui::Key::PageUp, egui::Modifiers::default()),
                key_event(egui::Key::PageDown, shift),
                key_event(egui::Key::Backspace, egui::Modifiers::default()),
                key_event(egui::Key::Delete, non_mac_ctrl()),
                key_event(egui::Key::Enter, egui::Modifiers::default()),
                key_event(egui::Key::Tab, egui::Modifiers::default()),
            ],
            platform: PlatformFlavor::Other,
            expected: vec![
                VirtualInputCommand::PageUp { select: false },
                VirtualInputCommand::PageDown { select: true },
                VirtualInputCommand::Backspace { word: false },
                VirtualInputCommand::DeleteForward { word: true },
                VirtualInputCommand::InsertNewline,
                VirtualInputCommand::InsertTab,
            ],
        },
        PlatformCommandCase {
            name: "mac plain arrows",
            events: vec![
                key_event(egui::Key::ArrowLeft, egui::Modifiers::default()),
                key_event(egui::Key::ArrowRight, egui::Modifiers::default()),
                key_event(egui::Key::ArrowUp, egui::Modifiers::default()),
                key_event(egui::Key::ArrowDown, egui::Modifiers::default()),
            ],
            platform: PlatformFlavor::Mac,
            expected: vec![
                VirtualInputCommand::MoveLeft {
                    select: false,
                    word: false,
                },
                VirtualInputCommand::MoveRight {
                    select: false,
                    word: false,
                },
                VirtualInputCommand::MoveUp { select: false },
                VirtualInputCommand::MoveDown { select: false },
            ],
        },
        PlatformCommandCase {
            name: "mac option horizontal arrows are word movement",
            events: vec![
                key_event(egui::Key::ArrowLeft, mac_option()),
                key_event(egui::Key::ArrowRight, mac_option_shift()),
            ],
            platform: PlatformFlavor::Mac,
            expected: vec![
                VirtualInputCommand::MoveLeft {
                    select: false,
                    word: true,
                },
                VirtualInputCommand::MoveRight {
                    select: true,
                    word: true,
                },
            ],
        },
        PlatformCommandCase {
            name: "mac command arrows are line and document boundaries",
            events: vec![
                key_event(egui::Key::ArrowLeft, mac_cmd()),
                key_event(egui::Key::ArrowRight, mac_cmd_shift()),
                key_event(egui::Key::ArrowUp, mac_cmd()),
                key_event(egui::Key::ArrowDown, mac_cmd_shift()),
            ],
            platform: PlatformFlavor::Mac,
            expected: vec![
                VirtualInputCommand::MoveLineHome { select: false },
                VirtualInputCommand::MoveLineEnd { select: true },
                VirtualInputCommand::MoveDocHome { select: false },
                VirtualInputCommand::MoveDocEnd { select: true },
            ],
        },
        PlatformCommandCase {
            name: "mac home end are document boundaries",
            events: vec![
                key_event(egui::Key::Home, egui::Modifiers::default()),
                key_event(egui::Key::End, shift),
            ],
            platform: PlatformFlavor::Mac,
            expected: vec![
                VirtualInputCommand::MoveDocHome { select: false },
                VirtualInputCommand::MoveDocEnd { select: true },
            ],
        },
        PlatformCommandCase {
            name: "mac paging deletion and insertion",
            events: vec![
                key_event(egui::Key::PageUp, egui::Modifiers::default()),
                key_event(egui::Key::PageDown, shift),
                key_event(egui::Key::Backspace, mac_option()),
                key_event(egui::Key::Delete, mac_cmd()),
                key_event(egui::Key::Enter, egui::Modifiers::default()),
                key_event(egui::Key::Tab, egui::Modifiers::default()),
            ],
            platform: PlatformFlavor::Mac,
            expected: vec![
                VirtualInputCommand::PageUp { select: false },
                VirtualInputCommand::PageDown { select: true },
                VirtualInputCommand::Backspace { word: true },
                VirtualInputCommand::DeleteToLineEnd,
                VirtualInputCommand::InsertNewline,
                VirtualInputCommand::InsertTab,
            ],
        },
        PlatformCommandCase {
            name: "mac ctrl emacs editing",
            events: vec![
                key_event(egui::Key::A, mac_ctrl_shift()),
                key_event(egui::Key::E, mac_ctrl()),
                key_event(egui::Key::B, mac_ctrl()),
                key_event(egui::Key::F, mac_ctrl_shift()),
                key_event(egui::Key::P, mac_ctrl()),
                key_event(egui::Key::N, mac_ctrl_shift()),
                key_event(egui::Key::K, mac_ctrl()),
            ],
            platform: PlatformFlavor::Mac,
            expected: vec![
                VirtualInputCommand::MoveLineHome { select: true },
                VirtualInputCommand::MoveLineEnd { select: false },
                VirtualInputCommand::MoveLeft {
                    select: false,
                    word: false,
                },
                VirtualInputCommand::MoveRight {
                    select: true,
                    word: false,
                },
                VirtualInputCommand::MoveUp { select: false },
                VirtualInputCommand::MoveDown { select: true },
                VirtualInputCommand::DeleteToLineEnd,
            ],
        },
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
