use super::types::{
    ApprovalCommand, Command, ContextCommand, ParsedInput, RequestCommand, ToolCommand,
};

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
            "/approval status" => Command::Approval(ApprovalCommand::Status),
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
