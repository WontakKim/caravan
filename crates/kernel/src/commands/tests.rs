use super::*;

#[test]
fn empty_input_returns_empty() {
    assert!(matches!(parse_input(""), ParsedInput::Empty));
}

#[test]
fn whitespace_input_returns_empty() {
    assert!(matches!(parse_input("   "), ParsedInput::Empty));
}

#[test]
fn help_command() {
    assert!(matches!(
        parse_input("/help"),
        ParsedInput::SlashCommand(Command::Help)
    ));
}

#[test]
fn clear_command() {
    assert!(matches!(
        parse_input("/clear"),
        ParsedInput::SlashCommand(Command::Clear)
    ));
}

#[test]
fn exit_command() {
    assert!(matches!(
        parse_input("/exit"),
        ParsedInput::SlashCommand(Command::Exit)
    ));
}

#[test]
fn unknown_slash_command() {
    assert!(matches!(
        parse_input("/foo"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

#[test]
fn quit_parses_as_unknown() {
    // `/quit` was removed in favour of `/exit`; it must fall through to
    // Command::Unknown so that typing `/quit` does NOT exit the application.
    assert!(matches!(
        parse_input("/quit"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

#[test]
fn plain_text() {
    assert!(matches!(parse_input("hello"), ParsedInput::UserMessage(_)));
}

#[test]
fn user_message_value_exact() {
    match parse_input("hello") {
        ParsedInput::UserMessage(s) => assert_eq!(s, "hello"),
        other => panic!("expected UserMessage, got {:?}", other),
    }
}

#[test]
fn user_message_trims_whitespace() {
    match parse_input("  hello  ") {
        ParsedInput::UserMessage(s) => assert_eq!(s, "hello"),
        other => panic!("expected UserMessage, got {:?}", other),
    }
}

#[test]
fn unknown_preserves_raw_untrimmed_input() {
    match parse_input("  /foo  ") {
        ParsedInput::SlashCommand(Command::Unknown(s)) => assert_eq!(s, "  /foo  "),
        other => panic!("expected SlashCommand(Unknown), got {:?}", other),
    }
}

#[test]
fn askx_stays_unknown() {
    assert!(matches!(
        parse_input("/askx"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

#[test]
fn ask_parses_as_unknown() {
    assert!(matches!(
        parse_input("/ask hello"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

// --- /tool command parsing tests ---

#[test]
fn tool_list_bare_defaults_to_dot() {
    assert_eq!(
        parse_input("/tool list"),
        ParsedInput::SlashCommand(Command::Tool(ToolCommand::List {
            path: ".".to_string()
        }))
    );
}

#[test]
fn tool_list_explicit_dot_stays_dot() {
    assert_eq!(
        parse_input("/tool list ."),
        ParsedInput::SlashCommand(Command::Tool(ToolCommand::List {
            path: ".".to_string()
        }))
    );
}

#[test]
fn tool_list_with_path() {
    assert_eq!(
        parse_input("/tool list crates/kernel"),
        ParsedInput::SlashCommand(Command::Tool(ToolCommand::List {
            path: "crates/kernel".to_string()
        }))
    );
}

#[test]
fn tool_read_with_file() {
    assert_eq!(
        parse_input("/tool read README.md"),
        ParsedInput::SlashCommand(Command::Tool(ToolCommand::Read {
            path: "README.md".to_string()
        }))
    );
}

#[test]
fn tool_read_bare_is_unknown() {
    assert!(matches!(
        parse_input("/tool read"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

#[test]
fn tool_bare_is_unknown() {
    assert!(matches!(
        parse_input("/tool"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

#[test]
fn tool_bogus_subcommand_is_unknown() {
    assert!(matches!(
        parse_input("/tool bogus"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

#[test]
fn tool_write_is_unknown() {
    assert!(matches!(
        parse_input("/tool write some-file"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

#[test]
fn regression_quit_is_unknown() {
    assert!(matches!(
        parse_input("/quit"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

#[test]
fn regression_ask_hello_is_unknown() {
    assert!(matches!(
        parse_input("/ask hello"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

// --- /context command parsing tests ---

#[test]
fn context_attach_last_tool_parses_correctly() {
    assert_eq!(
        parse_input("/context attach-last-tool"),
        ParsedInput::SlashCommand(Command::Context(ContextCommand::AttachLastTool))
    );
}

#[test]
fn context_clear_parses_correctly() {
    assert_eq!(
        parse_input("/context clear"),
        ParsedInput::SlashCommand(Command::Context(ContextCommand::Clear))
    );
}

#[test]
fn context_status_parses_correctly() {
    assert_eq!(
        parse_input("/context status"),
        ParsedInput::SlashCommand(Command::Context(ContextCommand::Status))
    );
}

#[test]
fn context_unknown_subcommand_is_unknown() {
    assert!(matches!(
        parse_input("/context unknown"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

#[test]
fn context_bare_is_unknown() {
    assert!(matches!(
        parse_input("/context"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

// --- /request command parsing tests ---

#[test]
fn request_status_parses_correctly() {
    assert_eq!(
        parse_input("/request status"),
        ParsedInput::SlashCommand(Command::Request(RequestCommand::Status))
    );
}

#[test]
fn request_clear_parses_correctly() {
    assert_eq!(
        parse_input("/request clear"),
        ParsedInput::SlashCommand(Command::Request(RequestCommand::Clear))
    );
}

#[test]
fn request_unknown_subcommand_is_unknown() {
    assert!(matches!(
        parse_input("/request unknown"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

#[test]
fn request_bare_is_unknown() {
    assert!(matches!(
        parse_input("/request"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

#[test]
fn request_run_parses_correctly() {
    assert_eq!(
        parse_input("/request run"),
        ParsedInput::SlashCommand(Command::Request(RequestCommand::Run))
    );
}

#[test]
fn request_approve_is_unknown() {
    assert!(matches!(
        parse_input("/request approve"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

#[test]
fn request_execute_is_unknown() {
    assert!(matches!(
        parse_input("/request execute"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

#[test]
fn request_accept_is_unknown() {
    assert!(matches!(
        parse_input("/request accept"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

// --- /approval command parsing tests ---

#[test]
fn approval_status_parses_correctly() {
    assert_eq!(
        parse_input("/approval status"),
        ParsedInput::SlashCommand(Command::Approval(ApprovalCommand::Status))
    );
}

#[test]
fn approval_bare_is_unknown() {
    assert!(matches!(
        parse_input("/approval"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

#[test]
fn approval_approve_is_unknown() {
    assert!(matches!(
        parse_input("/approval approve"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

#[test]
fn approval_reject_is_unknown() {
    assert!(matches!(
        parse_input("/approval reject"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

#[test]
fn approval_clear_is_unknown() {
    assert!(matches!(
        parse_input("/approval clear"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

#[test]
fn approval_run_is_unknown() {
    assert!(matches!(
        parse_input("/approval run"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

#[test]
fn approval_unknown_is_unknown() {
    assert!(matches!(
        parse_input("/approval unknown"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}
