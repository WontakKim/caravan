use crate::tool::registry::{DEFAULT_READ_RANGE_LIMIT_LINES, MAX_READ_RANGE_LIMIT_LINES};

use super::types::{
    ApprovalCommand, Command, ContextCommand, ParsedInput, RequestCommand, ToolCommand,
};

/// Parses the "read" subcommand after the path has been split off from the subcommand.
///
/// `rest` is everything after "read " (already trimmed, guaranteed non-empty by the caller).
/// Returns `Command::Unknown` for any invalid input.
fn parse_read_command(input: &str, rest: &str) -> Command {
    let mut tokens = rest.split_whitespace();
    let file_path = match tokens.next() {
        Some(p) => p.to_string(),
        None => return Command::Unknown(input.to_string()),
    };

    let remaining: Vec<&str> = tokens.collect();
    let mut offset: Option<usize> = None;
    let mut limit: Option<usize> = None;

    let mut i = 0;
    while i < remaining.len() {
        match remaining[i] {
            "--offset" => {
                if offset.is_some() {
                    return Command::Unknown(input.to_string());
                }
                i += 1;
                if i >= remaining.len() {
                    return Command::Unknown(input.to_string());
                }
                match remaining[i].parse::<usize>() {
                    Ok(0) => return Command::Unknown(input.to_string()),
                    Ok(n) => offset = Some(n),
                    Err(_) => return Command::Unknown(input.to_string()),
                }
            }
            "--limit" => {
                if limit.is_some() {
                    return Command::Unknown(input.to_string());
                }
                i += 1;
                if i >= remaining.len() {
                    return Command::Unknown(input.to_string());
                }
                match remaining[i].parse::<usize>() {
                    Ok(0) => return Command::Unknown(input.to_string()),
                    Ok(n) if n > MAX_READ_RANGE_LIMIT_LINES => {
                        return Command::Unknown(input.to_string());
                    }
                    Ok(n) => limit = Some(n),
                    Err(_) => return Command::Unknown(input.to_string()),
                }
            }
            _ => {
                return Command::Unknown(input.to_string());
            }
        }
        i += 1;
    }

    // Normalization rules:
    // - neither flag   => None, None (full read)
    // - --offset N only => offset=Some(N), limit=Some(DEFAULT_READ_RANGE_LIMIT_LINES)
    // - --limit N only  => offset=Some(1), limit=Some(N)
    // - both (any order) => offset=Some(N), limit=Some(M)
    let (final_offset, final_limit) = match (offset, limit) {
        (None, None) => (None, None),
        (Some(o), None) => (Some(o), Some(DEFAULT_READ_RANGE_LIMIT_LINES)),
        (None, Some(l)) => (Some(1), Some(l)),
        (Some(o), Some(l)) => (Some(o), Some(l)),
    };

    Command::Tool(ToolCommand::Read {
        path: file_path,
        offset: final_offset,
        limit: final_limit,
    })
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
            "/exit" | "/quit" => Command::Exit,
            "/reset" | "/new" => Command::ResetSession,
            "/permissions" => Command::Permissions,
            "/allowed-tools" => Command::AllowedTools,
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
                    "read" if !path.is_empty() => parse_read_command(input, path),
                    "plan-write" if !path.is_empty() && !path.contains(char::is_whitespace) => {
                        Command::Tool(ToolCommand::PlanWrite {
                            path: path.to_string(),
                        })
                    }
                    "preview-write" if !path.is_empty() && !path.contains(char::is_whitespace) => {
                        Command::Tool(ToolCommand::PreviewWrite {
                            path: path.to_string(),
                        })
                    }
                    "propose-write" if !path.is_empty() && !path.contains(char::is_whitespace) => {
                        Command::Tool(ToolCommand::ProposeWrite {
                            path: path.to_string(),
                        })
                    }
                    // Search accepts a multi-word query (no whitespace guard).
                    "search" if !path.is_empty() => Command::Tool(ToolCommand::Search {
                        query: path.to_string(),
                    }),
                    // Glob accepts a multi-word pattern (no whitespace guard), same as search.
                    "glob" if !path.is_empty() => Command::Tool(ToolCommand::Glob {
                        pattern: path.to_string(),
                    }),
                    // Any other subcommand, or commands with missing/invalid args
                    _ => Command::Unknown(input.to_string()),
                }
            }
            // Handles /approval approve <seq>, /approval reject <seq>, and /approval resume <seq>.
            t if t.starts_with("/approval ") => {
                let remainder = t["/approval ".len()..].trim();
                let tokens: Vec<&str> = remainder.split_whitespace().collect();
                match tokens.as_slice() {
                    ["approve", seq_str] => match seq_str.parse::<u64>() {
                        Ok(seq) => Command::Approval(ApprovalCommand::Approve { seq }),
                        Err(_) => Command::Unknown(input.to_string()),
                    },
                    ["reject", seq_str] => match seq_str.parse::<u64>() {
                        Ok(seq) => Command::Approval(ApprovalCommand::Reject { seq }),
                        Err(_) => Command::Unknown(input.to_string()),
                    },
                    ["resume", seq_str] => match seq_str.parse::<u64>() {
                        Ok(seq) => Command::Approval(ApprovalCommand::Resume { seq }),
                        Err(_) => Command::Unknown(input.to_string()),
                    },
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
