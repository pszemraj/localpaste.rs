//! IME-specific regression coverage for virtual editor command handling.

use super::*;

#[test]
fn mutating_commands_cancel_active_preedit_before_applying_edits() {
    struct Case {
        mutation: VirtualInputCommand,
        expected_text: &'static str,
    }

    let cases = [
        Case {
            mutation: VirtualInputCommand::Undo,
            expected_text: "ab",
        },
        Case {
            mutation: VirtualInputCommand::Backspace { word: false },
            expected_text: "b",
        },
    ];

    for case in cases {
        let mut harness = make_app();
        harness.app.reset_virtual_editor("ab");
        let len = harness.app.virtual_editor_buffer.len_chars();
        harness.app.virtual_editor_state.set_cursor(1, len);
        let ctx = egui::Context::default();

        let commands = vec![
            VirtualInputCommand::ImeEnabled,
            VirtualInputCommand::ImePreedit("に".to_string()),
            case.mutation.clone(),
            VirtualInputCommand::ImeDisabled,
        ];
        let result = harness
            .app
            .apply_virtual_commands(&ctx, commands.as_slice());

        assert!(result.changed);
        assert_eq!(
            harness.app.virtual_editor_buffer.to_string(),
            case.expected_text
        );
        assert!(!harness.app.virtual_editor_state.ime.enabled);
        assert!(harness.app.virtual_editor_state.ime.preedit_range.is_none());
        assert!(harness.app.virtual_editor_state.ime.preedit_text.is_empty());
    }
}
