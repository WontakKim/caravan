pub enum Command {
    Help,
    Clear,
    Quit,
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
            "/quit" => Command::Quit,
            _ => Command::Unknown(trimmed.to_string()),
        };
    }
    Command::Text(trimmed.to_string())
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
    fn quit_command() {
        assert!(matches!(parse_input("/quit"), Command::Quit));
    }

    #[test]
    fn unknown_slash_command() {
        assert!(matches!(parse_input("/foo"), Command::Unknown(_)));
    }

    #[test]
    fn plain_text() {
        assert!(matches!(parse_input("hello"), Command::Text(_)));
    }
}
