//! Input-event reducer for the virtual editor.

use eframe::egui;

/// Normalized commands consumed by the virtual editor state machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum VirtualInputCommand {
    MoveLeft { select: bool, word: bool },
    MoveRight { select: bool, word: bool },
    MoveUp { select: bool },
    MoveDown { select: bool },
    MoveHome { select: bool },
    MoveEnd { select: bool },
    PageUp { select: bool },
    PageDown { select: bool },
    Backspace { word: bool },
    DeleteForward { word: bool },
    InsertText(String),
    InsertNewline,
    InsertTab,
    SelectAll,
    Copy,
    Cut,
    Paste(String),
    Undo,
    Redo,
    ImeEnabled,
    ImePreedit(String),
    ImeCommit(String),
    ImeDisabled,
}

fn word_modifier(modifiers: egui::Modifiers) -> bool {
    modifiers.ctrl || modifiers.alt
}

/// Convert egui input events into virtual-editor commands.
pub(crate) fn commands_from_events(
    events: &[egui::Event],
    focused: bool,
) -> Vec<VirtualInputCommand> {
    if !focused {
        return Vec::new();
    }
    let mut out = Vec::new();
    for event in events {
        match event {
            egui::Event::Text(text) => {
                if !text.is_empty() {
                    out.push(VirtualInputCommand::InsertText(text.clone()));
                }
            }
            egui::Event::Paste(text) => out.push(VirtualInputCommand::Paste(text.clone())),
            egui::Event::Ime(ime) => match ime {
                egui::ImeEvent::Enabled => out.push(VirtualInputCommand::ImeEnabled),
                egui::ImeEvent::Preedit(text) => {
                    out.push(VirtualInputCommand::ImePreedit(text.clone()))
                }
                egui::ImeEvent::Commit(text) => {
                    out.push(VirtualInputCommand::ImeCommit(text.clone()))
                }
                egui::ImeEvent::Disabled => out.push(VirtualInputCommand::ImeDisabled),
            },
            egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => {
                if modifiers.command {
                    match key {
                        egui::Key::A => out.push(VirtualInputCommand::SelectAll),
                        egui::Key::C => out.push(VirtualInputCommand::Copy),
                        egui::Key::X => out.push(VirtualInputCommand::Cut),
                        egui::Key::Z if modifiers.shift => out.push(VirtualInputCommand::Redo),
                        egui::Key::Z => out.push(VirtualInputCommand::Undo),
                        egui::Key::Y => out.push(VirtualInputCommand::Redo),
                        _ => {}
                    }
                    continue;
                }
                match key {
                    egui::Key::ArrowLeft => out.push(VirtualInputCommand::MoveLeft {
                        select: modifiers.shift,
                        word: word_modifier(*modifiers),
                    }),
                    egui::Key::ArrowRight => out.push(VirtualInputCommand::MoveRight {
                        select: modifiers.shift,
                        word: word_modifier(*modifiers),
                    }),
                    egui::Key::ArrowUp => out.push(VirtualInputCommand::MoveUp {
                        select: modifiers.shift,
                    }),
                    egui::Key::ArrowDown => out.push(VirtualInputCommand::MoveDown {
                        select: modifiers.shift,
                    }),
                    egui::Key::Home => out.push(VirtualInputCommand::MoveHome {
                        select: modifiers.shift,
                    }),
                    egui::Key::End => out.push(VirtualInputCommand::MoveEnd {
                        select: modifiers.shift,
                    }),
                    egui::Key::PageUp => out.push(VirtualInputCommand::PageUp {
                        select: modifiers.shift,
                    }),
                    egui::Key::PageDown => out.push(VirtualInputCommand::PageDown {
                        select: modifiers.shift,
                    }),
                    egui::Key::Backspace => out.push(VirtualInputCommand::Backspace {
                        word: word_modifier(*modifiers),
                    }),
                    egui::Key::Delete => out.push(VirtualInputCommand::DeleteForward {
                        word: word_modifier(*modifiers),
                    }),
                    egui::Key::Enter => out.push(VirtualInputCommand::InsertNewline),
                    egui::Key::Tab => out.push(VirtualInputCommand::InsertTab),
                    _ => {}
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

    #[test]
    fn maps_command_shortcuts() {
        let events = vec![egui::Event::Key {
            key: egui::Key::A,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers {
                command: true,
                ..Default::default()
            },
        }];
        let commands = commands_from_events(&events, true);
        assert_eq!(commands, vec![VirtualInputCommand::SelectAll]);
    }

    #[test]
    fn maps_ime_events() {
        let events = vec![
            egui::Event::Ime(egui::ImeEvent::Enabled),
            egui::Event::Ime(egui::ImeEvent::Preedit("に".to_string())),
            egui::Event::Ime(egui::ImeEvent::Commit("日".to_string())),
            egui::Event::Ime(egui::ImeEvent::Disabled),
        ];
        let commands = commands_from_events(&events, true);
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
}
