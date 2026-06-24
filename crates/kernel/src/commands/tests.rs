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
fn quit_parses_as_exit() {
    // `/quit` is an alias for `/exit`.
    assert!(matches!(
        parse_input("/quit"),
        ParsedInput::SlashCommand(Command::Exit)
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
fn tool_plan_write_with_path() {
    assert_eq!(
        parse_input("/tool plan-write README.md"),
        ParsedInput::SlashCommand(Command::Tool(ToolCommand::PlanWrite {
            path: "README.md".to_string()
        }))
    );
}

#[test]
fn tool_plan_write_bare_is_unknown() {
    assert!(matches!(
        parse_input("/tool plan-write"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

#[test]
fn tool_plan_write_extra_token_is_unknown() {
    assert!(matches!(
        parse_input("/tool plan-write README.md extra"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

#[test]
fn regression_quit_is_exit_alias() {
    assert!(matches!(
        parse_input("/quit"),
        ParsedInput::SlashCommand(Command::Exit)
    ));
}

#[test]
fn regression_ask_hello_is_unknown() {
    assert!(matches!(
        parse_input("/ask hello"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

// --- Session-command parsing tests ---

#[test]
fn reset_parses_as_reset_session() {
    assert!(matches!(
        parse_input("/reset"),
        ParsedInput::SlashCommand(Command::ResetSession)
    ));
}

#[test]
fn new_parses_as_reset_session() {
    assert!(matches!(
        parse_input("/new"),
        ParsedInput::SlashCommand(Command::ResetSession)
    ));
}

#[test]
fn permissions_parses_correctly() {
    assert!(matches!(
        parse_input("/permissions"),
        ParsedInput::SlashCommand(Command::Permissions)
    ));
}

#[test]
fn allowed_tools_parses_correctly() {
    assert!(matches!(
        parse_input("/allowed-tools"),
        ParsedInput::SlashCommand(Command::AllowedTools)
    ));
}

// --- Unsupported commands remain Unknown ---

#[test]
fn model_is_unknown() {
    assert!(matches!(
        parse_input("/model"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

#[test]
fn plan_is_unknown() {
    assert!(matches!(
        parse_input("/plan"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

#[test]
fn diff_is_unknown() {
    assert!(matches!(
        parse_input("/diff"),
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

#[test]
fn approval_approve_seq_parses_correctly() {
    assert_eq!(
        parse_input("/approval approve 12"),
        ParsedInput::SlashCommand(Command::Approval(ApprovalCommand::Approve { seq: 12 }))
    );
}

#[test]
fn approval_reject_seq_parses_correctly() {
    assert_eq!(
        parse_input("/approval reject 12"),
        ParsedInput::SlashCommand(Command::Approval(ApprovalCommand::Reject { seq: 12 }))
    );
}

#[test]
fn approval_approve_u64_max_parses_correctly() {
    assert_eq!(
        parse_input("/approval approve 18446744073709551615"),
        ParsedInput::SlashCommand(Command::Approval(ApprovalCommand::Approve {
            seq: u64::MAX
        }))
    );
}

#[test]
fn approval_approve_leading_zeros_normalizes() {
    assert_eq!(
        parse_input("/approval approve 00012"),
        ParsedInput::SlashCommand(Command::Approval(ApprovalCommand::Approve { seq: 12 }))
    );
}

#[test]
fn approval_approve_abc_is_unknown() {
    assert!(matches!(
        parse_input("/approval approve abc"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

#[test]
fn approval_reject_abc_is_unknown() {
    assert!(matches!(
        parse_input("/approval reject abc"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

#[test]
fn approval_approve_extra_token_is_unknown() {
    assert!(matches!(
        parse_input("/approval approve 12 because"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

#[test]
fn approval_reject_extra_token_is_unknown() {
    assert!(matches!(
        parse_input("/approval reject 12 because"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

#[test]
fn approval_approve_negative_is_unknown() {
    assert!(matches!(
        parse_input("/approval approve -1"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

#[test]
fn approval_approve_decimal_is_unknown() {
    assert!(matches!(
        parse_input("/approval approve 12.0"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

#[test]
fn approval_approve_overflow_is_unknown() {
    assert!(matches!(
        parse_input("/approval approve 18446744073709551616"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

#[test]
fn approval_approve_abc_raw_untrimmed_payload() {
    match parse_input("/approval approve abc  ") {
        ParsedInput::SlashCommand(Command::Unknown(s)) => {
            assert_eq!(s, "/approval approve abc  ")
        }
        other => panic!("expected SlashCommand(Unknown), got {:?}", other),
    }
}

#[test]
fn approval_resume_seq_parses_correctly() {
    assert_eq!(
        parse_input("/approval resume 12"),
        ParsedInput::SlashCommand(Command::Approval(ApprovalCommand::Resume { seq: 12 }))
    );
}

#[test]
fn approval_resume_bare_is_unknown() {
    assert!(matches!(
        parse_input("/approval resume"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

#[test]
fn approval_resume_abc_is_unknown() {
    assert!(matches!(
        parse_input("/approval resume abc"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

#[test]
fn approval_resume_extra_token_is_unknown() {
    assert!(matches!(
        parse_input("/approval resume 12 extra"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

#[test]
fn approval_run_is_unknown_no_regression() {
    assert!(matches!(
        parse_input("/approval run"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

// --- /tool preview-write parsing tests ---

#[test]
fn tool_preview_write_with_path() {
    assert_eq!(
        parse_input("/tool preview-write README.md"),
        ParsedInput::SlashCommand(Command::Tool(ToolCommand::PreviewWrite {
            path: "README.md".to_string()
        }))
    );
}

#[test]
fn tool_preview_write_bare_is_unknown() {
    assert!(matches!(
        parse_input("/tool preview-write"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

#[test]
fn tool_preview_write_extra_token_is_unknown() {
    assert!(matches!(
        parse_input("/tool preview-write README.md extra"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

// --- /tool propose-write parsing tests ---

#[test]
fn tool_propose_write_with_path() {
    assert_eq!(
        parse_input("/tool propose-write README.md"),
        ParsedInput::SlashCommand(Command::Tool(ToolCommand::ProposeWrite {
            path: "README.md".to_string()
        }))
    );
}

#[test]
fn tool_propose_write_bare_is_unknown() {
    assert!(matches!(
        parse_input("/tool propose-write"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

#[test]
fn tool_propose_write_extra_token_is_unknown() {
    assert!(matches!(
        parse_input("/tool propose-write README.md extra"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

// --- Explicit Unknown regressions ---

#[test]
fn tool_write_bare_is_unknown() {
    assert!(matches!(
        parse_input("/tool write"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

#[test]
fn tool_apply_is_unknown() {
    assert!(matches!(
        parse_input("/tool apply patch.diff"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

#[test]
fn approval_execute_is_unknown() {
    assert!(matches!(
        parse_input("/approval execute"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

// --- help catalog parity tests ---

#[test]
fn help_catalog_has_18_entries_in_exact_order() {
    let entries = command_help_entries();
    let commands: Vec<&str> = entries.iter().map(|e| e.command).collect();
    assert_eq!(
        commands,
        vec![
            "/help",
            "/clear",
            "/exit",
            "/tool list [path]",
            "/tool read <path>",
            "/tool plan-write <path>",
            "/tool preview-write <path>",
            "/tool propose-write <path>",
            "/context attach-last-tool",
            "/context clear",
            "/context status",
            "/request status",
            "/request clear",
            "/request run",
            "/approval status",
            "/approval approve <seq>",
            "/approval reject <seq>",
            "/approval resume <seq>",
        ]
    );
}

#[test]
fn help_catalog_excludes_removed_commands() {
    let commands: Vec<&str> = command_help_entries().iter().map(|e| e.command).collect();
    for forbidden in &[
        "/ask",
        "/quit",
        "/tool write",
        "/approval run",
        "/model",
        "/agent",
    ] {
        assert!(
            !commands.contains(forbidden),
            "help catalog must not contain {:?}",
            forbidden
        );
    }
}
