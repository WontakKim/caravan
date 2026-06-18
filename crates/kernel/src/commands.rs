mod types;
pub use types::{Command, ContextCommand, ParsedInput, RequestCommand, ToolCommand};

pub fn parse_input(input: &str) -> ParsedInput {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return ParsedInput::Empty;
    }
    if trimmed.starts_with('/') {
        return ParsedInput::SlashCommand(match trimmed {
            "/help" => Command::Help,
            "/clear" => Command::Clear,
            "/exit" => Command::Exit,
            "/context attach-last-tool" => Command::Context(ContextCommand::AttachLastTool),
            "/context clear" => Command::Context(ContextCommand::Clear),
            "/context status" => Command::Context(ContextCommand::Status),
            "/request status" => Command::Request(RequestCommand::Status),
            "/request clear" => Command::Request(RequestCommand::Clear),
            "/request run" => Command::Request(RequestCommand::Run),
            t if t.starts_with("/tool ") => {
                let after_tool = t["/tool ".len()..].trim();
                let (subcommand, path) = match after_tool.split_once(char::is_whitespace) {
                    Some((sub, rest)) => (sub, rest.trim()),
                    None => (after_tool, ""),
                };
                match subcommand {
                    "list" => {
                        let effective = if path.is_empty() { "." } else { path };
                        Command::Tool(ToolCommand::List {
                            path: effective.to_string(),
                        })
                    }
                    "read" if !path.is_empty() => Command::Tool(ToolCommand::Read {
                        path: path.to_string(),
                    }),
                    // Any other subcommand, or "read" with empty path
                    _ => Command::Unknown(input.to_string()),
                }
            }
            // Carry the raw (untrimmed) input so the recorded event detail is
            // exactly what the user typed.
            _ => Command::Unknown(input.to_string()),
        });
    }
    ParsedInput::UserMessage(trimmed.to_string())
}

#[cfg(test)]
mod tests {
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
}
