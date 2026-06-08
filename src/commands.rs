#[derive(Debug)]
pub enum Command {
    Help,
    Clear,
    Exit,
    Unknown(String),
}

#[derive(Debug)]
pub enum ParsedInput {
    Empty,
    UserMessage(String),
    SlashCommand(Command),
}

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
}
