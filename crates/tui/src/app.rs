use std::path::PathBuf;

use kernel::events::{EventKind, EventLog};
use kernel::manual_context::ManualToolContext;
use kernel::model_gateway::ModelGateway;
use kernel::model_tool_request::ModelToolRequest;
use kernel::storage::EventStore;

const INSPECTOR_SCROLL_STEP: u16 = 3;
const TOOL_LIST_PREVIEW_ENTRIES: usize = 50;
const TOOL_READ_PREVIEW_BYTES: usize = 4 * 1024;
const NO_TOOL_OUTPUT_NOTICE: &str =
    "No recent tool output to attach. Run /tool read or /tool list first.";

pub struct App {
    pub log: Vec<String>,
    pub input: String,
    pub should_exit: bool,
    pub event_log: EventLog,
    pub selected_event: Option<usize>,
    pub model_gateway: ModelGateway,
    pub inspector_scroll: u16,
    pub workspace_root: PathBuf,
    pub last_tool_output_candidate: Option<ManualToolContext>,
    pub pending_manual_tool_context: Option<ManualToolContext>,
    pub pending_model_tool_request: Option<ModelToolRequest>,
}

impl App {
    pub fn new() -> Self {
        let mut event_log = EventLog::new();
        event_log.append(EventKind::AppStart, "Caravan started.");
        Self {
            log: vec!["Caravan started.".to_string()],
            input: String::new(),
            should_exit: false,
            event_log,
            selected_event: None,
            model_gateway: ModelGateway::default(),
            inspector_scroll: 0,
            workspace_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            last_tool_output_candidate: None,
            pending_manual_tool_context: None,
            pending_model_tool_request: None,
        }
    }

    /// Constructs a store-backed app: loads existing events from `store`, appends
    /// `AppStart` (which is persisted at `seq = max_loaded + 1`), and returns
    /// a fresh app whose screen log starts with only "Caravan started.".
    pub fn with_store(store: EventStore) -> App {
        Self::with_store_and_gateway(store, ModelGateway::default())
    }

    /// Constructs a store-backed app with the given `gateway`: loads existing events
    /// from `store`, appends `AppStart`, and returns a fresh app whose screen log
    /// starts with only "Caravan started.".
    pub fn with_store_and_gateway(store: EventStore, gateway: ModelGateway) -> App {
        let workspace_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::with_store_gateway_and_workspace_root(store, gateway, workspace_root)
    }

    /// Constructs a store-backed app with an injected `workspace_root`: loads
    /// existing events from `store`, appends `AppStart`, and stores the given
    /// `workspace_root` for scoping file-system tool access.
    pub fn with_store_gateway_and_workspace_root(
        store: EventStore,
        gateway: ModelGateway,
        workspace_root: PathBuf,
    ) -> App {
        let mut event_log = EventLog::load_from(store);
        event_log.append(EventKind::AppStart, "Caravan started.");
        App {
            log: vec!["Caravan started.".to_string()],
            input: String::new(),
            should_exit: false,
            event_log,
            selected_event: None,
            model_gateway: gateway,
            inspector_scroll: 0,
            workspace_root,
            last_tool_output_candidate: None,
            pending_manual_tool_context: None,
            pending_model_tool_request: None,
        }
    }

    pub fn push_char(&mut self, c: char) {
        self.input.push(c);
    }

    pub fn backspace(&mut self) {
        self.input.pop();
    }

    /// Records a Ctrl+C exit as an `ExitRequest` event and sets `should_exit`.
    /// Ctrl+C is not a command-bar entry, so no `SlashCommand` event is emitted.
    pub fn exit_from_ctrl_c(&mut self) {
        self.event_log
            .append(EventKind::ExitRequest, "Exit requested (Ctrl+C)");
        self.should_exit = true;
    }

    pub fn submit(&mut self) {
        use kernel::commands::{Command, ParsedInput, parse_input};

        let raw = self.input.clone();
        match parse_input(&raw) {
            ParsedInput::Empty => return,
            ParsedInput::SlashCommand(cmd) => {
                self.event_log.append(EventKind::SlashCommand, &raw);
                match cmd {
                    Command::Help => {
                        let help_text = Self::help_lines().join(" ");
                        self.event_log.append(EventKind::HelpRequest, help_text);
                        self.log.extend(Self::help_lines());
                    }
                    Command::Clear => {
                        self.event_log
                            .append(EventKind::LogClear, "Screen log cleared");
                        self.log.clear();
                    }
                    Command::Exit => {
                        self.event_log
                            .append(EventKind::ExitRequest, "Exit requested");
                        self.should_exit = true;
                    }
                    Command::ResetSession => {
                        self.event_log.append(EventKind::LogClear, "Session reset");
                        self.log.clear();
                        self.last_tool_output_candidate = None;
                        self.pending_manual_tool_context = None;
                        self.pending_model_tool_request = None;
                    }
                    Command::Permissions => {
                        self.log.push(
                            "Permission posture: read-only (no write tools active)".to_string(),
                        );
                    }
                    Command::AllowedTools => {
                        self.log
                            .push("Allowed tools: list_files, read_file".to_string());
                    }
                    Command::Tool(tc) => self.handle_tool_command(tc),
                    Command::Context(cc) => self.handle_context_command(cc),
                    Command::Request(rc) => self.handle_request_command(rc),
                    Command::Approval(ac) => self.handle_approval_command(ac),
                    Command::Unknown(c) => {
                        self.event_log
                            .append(EventKind::UnknownSlashCommand, c.clone());
                        self.log.push(format!("Unknown command: {c}"));
                    }
                }
            }
            ParsedInput::UserMessage(message) => {
                self.event_log.append(EventKind::UserMessage, &message);
                let project_memory =
                    kernel::project_memory::load_project_memory(&self.workspace_root);
                let pending_context = self.pending_manual_tool_context.take();
                let output = kernel::runner::run_mock_turn(
                    &mut self.event_log,
                    &message,
                    &self.model_gateway,
                    &self.workspace_root,
                    pending_context.as_ref(),
                    Some(&project_memory),
                );
                self.log.push(format!("User: {}", output.user_message));
                if !output.assistant_response.is_empty() {
                    self.log
                        .push(format!("Assistant: {}", output.assistant_response));
                }
            }
        }

        self.input.clear();
    }

    pub fn help_lines() -> Vec<String> {
        use kernel::commands::default_command_help_sections;
        let mut lines = Vec::new();
        lines.push("Available commands:".to_string());
        lines.push("  Type a message (no leading /) to send it as a user message".to_string());
        for section in default_command_help_sections() {
            lines.push(format!("  {}:", section.header));
            for entry in section.entries {
                lines.push(format!("    {} - {}", entry.command, entry.description));
            }
        }
        lines
    }
}

mod approval;
mod context;
mod logging;
mod request;
mod selection;
mod tools;

#[cfg(test)]
mod tests;
