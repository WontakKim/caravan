#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolCommand {
    List { path: String },
    Read { path: String },
    PlanWrite { path: String },
    PreviewWrite { path: String },
    ProposeWrite { path: String },
    Search { query: String },
    Glob { pattern: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextCommand {
    AttachLastTool,
    Clear,
    Status,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestCommand {
    Status,
    Clear,
    Run,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalCommand {
    Status,
    Approve { seq: u64 },
    Reject { seq: u64 },
    Resume { seq: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Help,
    Clear,
    Exit,
    ResetSession,
    Permissions,
    AllowedTools,
    Tool(ToolCommand),
    Context(ContextCommand),
    Request(RequestCommand),
    Approval(ApprovalCommand),
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedInput {
    Empty,
    UserMessage(String),
    SlashCommand(Command),
}
