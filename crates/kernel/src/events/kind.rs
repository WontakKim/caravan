use serde::{Deserialize, Serialize};

/// The kind of application event that occurred.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum EventKind {
    AppStart,
    SlashCommand,
    HelpRequest,
    UserMessage,
    LogClear,
    ExitRequest,
    UnknownSlashCommand,
    RunCreate,
    RunStart,
    TurnStart,
    PromptCompile,
    ModelRoute,
    ModelOutputChunk,
    AssistantMessage,
    ModelUsage,
    RunComplete,
    RunFail,
    ModelError,
    ModelToolRequest,
    ToolPolicy,
    ApprovalRequest,
    ApprovalDecision,
    ApprovalResume,
    ToolCall,
    ToolResult,
    ToolError,
    ToolContextAttach,
    ToolContextClear,
}

impl EventKind {
    /// Returns the name of this variant as a static string.
    pub fn name(&self) -> &'static str {
        match self {
            EventKind::AppStart => "AppStart",
            EventKind::SlashCommand => "SlashCommand",
            EventKind::HelpRequest => "HelpRequest",
            EventKind::UserMessage => "UserMessage",
            EventKind::LogClear => "LogClear",
            EventKind::ExitRequest => "ExitRequest",
            EventKind::UnknownSlashCommand => "UnknownSlashCommand",
            EventKind::RunCreate => "RunCreate",
            EventKind::RunStart => "RunStart",
            EventKind::TurnStart => "TurnStart",
            EventKind::PromptCompile => "PromptCompile",
            EventKind::ModelRoute => "ModelRoute",
            EventKind::ModelOutputChunk => "ModelOutputChunk",
            EventKind::AssistantMessage => "AssistantMessage",
            EventKind::ModelUsage => "ModelUsage",
            EventKind::RunComplete => "RunComplete",
            EventKind::RunFail => "RunFail",
            EventKind::ModelError => "ModelError",
            EventKind::ModelToolRequest => "ModelToolRequest",
            EventKind::ToolPolicy => "ToolPolicy",
            EventKind::ApprovalRequest => "ApprovalRequest",
            EventKind::ApprovalDecision => "ApprovalDecision",
            EventKind::ApprovalResume => "ApprovalResume",
            EventKind::ToolCall => "ToolCall",
            EventKind::ToolResult => "ToolResult",
            EventKind::ToolError => "ToolError",
            EventKind::ToolContextAttach => "ToolContextAttach",
            EventKind::ToolContextClear => "ToolContextClear",
        }
    }
}
