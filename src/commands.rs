pub enum Command {
    Help,
    Clear,
    Exit,
    Text(String),
    Unknown(String),
    Empty,
}

pub fn parse_input(input: &str) -> Command {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Command::Empty;
    }
    if trimmed.starts_with('/') {
        return match trimmed {
            "/help" => Command::Help,
            "/clear" => Command::Clear,
            "/exit" => Command::Exit,
            // Carry the raw (untrimmed) input so the recorded event detail is
            // exactly what the user typed.
            _ => Command::Unknown(input.to_string()),
        };
    }
    Command::Text(input.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_returns_empty() {
        assert!(matches!(parse_input(""), Command::Empty));
    }

    #[test]
    fn whitespace_input_returns_empty() {
        assert!(matches!(parse_input("   "), Command::Empty));
    }

    #[test]
    fn help_command() {
        assert!(matches!(parse_input("/help"), Command::Help));
    }

    #[test]
    fn clear_command() {
        assert!(matches!(parse_input("/clear"), Command::Clear));
    }

    #[test]
    fn exit_command() {
        assert!(matches!(parse_input("/exit"), Command::Exit));
    }

    #[test]
    fn unknown_slash_command() {
        assert!(matches!(parse_input("/foo"), Command::Unknown(_)));
    }

    #[test]
    fn quit_parses_as_unknown() {
        // `/quit` was removed in favour of `/exit`; it must fall through to
        // Command::Unknown so that typing `/quit` does NOT exit the application.
        assert!(matches!(parse_input("/quit"), Command::Unknown(_)));
    }

    #[test]
    fn plain_text() {
        assert!(matches!(parse_input("hello"), Command::Text(_)));
    }

    #[test]
    fn text_preserves_raw_untrimmed_input() {
        match parse_input("  hello  ") {
            Command::Text(s) => assert_eq!(s, "  hello  "),
            other => panic!("expected Text, got {:?}", std::mem::discriminant(&other)),
        }
    }

    #[test]
    fn unknown_preserves_raw_untrimmed_input() {
        match parse_input("  /foo  ") {
            Command::Unknown(s) => assert_eq!(s, "  /foo  "),
            other => panic!("expected Unknown, got {:?}", std::mem::discriminant(&other)),
        }
    }
}
