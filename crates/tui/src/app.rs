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
        use kernel::commands::{
            Command, ContextCommand, ParsedInput, RequestCommand, ToolCommand, parse_input,
        };

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
                    Command::Tool(tc) => {
                        use kernel::{
                            ToolEventRunner, ToolExecutionContext, ToolOutput, ToolRequest,
                        };
                        let ctx = ToolExecutionContext {
                            workspace_root: self.workspace_root.clone(),
                        };
                        let (request, display_path) = match tc {
                            ToolCommand::List { path } => {
                                let dp = path.clone();
                                (ToolRequest::ListFiles { path }, dp)
                            }
                            ToolCommand::Read { path } => {
                                let dp = path.clone();
                                (ToolRequest::ReadFile { path }, dp)
                            }
                        };
                        match ToolEventRunner::new_readonly().run(
                            &mut self.event_log,
                            &ctx,
                            request,
                        ) {
                            Ok(ToolOutput::FileList { entries, .. }) => {
                                self.last_tool_output_candidate = Some(
                                    ManualToolContext::from_list_files(&display_path, &entries),
                                );
                                self.push_tool_list_output(&display_path, entries);
                            }
                            Ok(ToolOutput::FileContent { content, .. }) => {
                                self.last_tool_output_candidate = Some(
                                    ManualToolContext::from_read_file(&display_path, &content),
                                );
                                self.push_tool_read_output(&display_path, &content);
                            }
                            Err(error) => {
                                self.push_tool_error_output(error);
                            }
                        }
                    }
                    Command::Context(cc) => match cc {
                        ContextCommand::AttachLastTool => {
                            if let Some(candidate) = self.last_tool_output_candidate.clone() {
                                let summary = candidate.attach_summary();
                                self.pending_manual_tool_context = Some(candidate);
                                self.event_log
                                    .append(EventKind::ToolContextAttach, &summary);
                                self.log.push(format!("Tool context attached: {summary}"));
                            } else {
                                self.log.push(NO_TOOL_OUTPUT_NOTICE.to_string());
                            }
                        }
                        ContextCommand::Clear => {
                            self.pending_manual_tool_context = None;
                            self.event_log
                                .append(EventKind::ToolContextClear, "Tool context cleared");
                            self.log.push("Tool context cleared.".to_string());
                        }
                        ContextCommand::Status => {
                            let pending_summary = self
                                .pending_manual_tool_context
                                .as_ref()
                                .map(|ctx| ctx.attach_summary())
                                .unwrap_or_else(|| "none".to_string());
                            let candidate_summary = self
                                .last_tool_output_candidate
                                .as_ref()
                                .map(|ctx| ctx.attach_summary())
                                .unwrap_or_else(|| "none".to_string());
                            self.log.push("Context status:".to_string());
                            self.log.push(format!("- pending: {}", pending_summary));
                            self.log
                                .push(format!("- last tool output: {}", candidate_summary));
                        }
                    },
                    Command::Request(rc) => match rc {
                        RequestCommand::Status => {
                            self.log.push("Model tool request status:".to_string());
                            if let Some(req) = &self.pending_model_tool_request {
                                self.log.push(format!("- pending: {}", req.detail()));
                                self.log.push(format!(
                                    "- suggested command: {}",
                                    req.suggested_command()
                                ));
                                self.log.push(
                                    "- next: run /context attach-last-tool after the tool succeeds"
                                        .to_string(),
                                );
                            } else {
                                self.log.push("- pending: none".to_string());
                            }
                        }
                        RequestCommand::Clear => {
                            self.pending_model_tool_request = None;
                            self.log
                                .push("Cleared pending model tool request.".to_string());
                        }
                        RequestCommand::Run => {
                            use kernel::{ToolEventRunner, ToolExecutionContext, ToolOutput};
                            if let Some(req) = self.pending_model_tool_request.clone() {
                                let ctx = ToolExecutionContext {
                                    workspace_root: self.workspace_root.clone(),
                                };
                                let display_path = req.path.clone();
                                let tool_request = req.to_tool_request();
                                match ToolEventRunner::new_readonly().run(
                                    &mut self.event_log,
                                    &ctx,
                                    tool_request,
                                ) {
                                    Ok(ToolOutput::FileList { entries, .. }) => {
                                        self.last_tool_output_candidate =
                                            Some(ManualToolContext::from_list_files(
                                                &display_path,
                                                &entries,
                                            ));
                                        self.push_tool_list_output(&display_path, entries);
                                        self.pending_model_tool_request = None;
                                        self.log.push(
                                            "Run /context attach-last-tool to include this tool output in the next prompt.".to_string(),
                                        );
                                    }
                                    Ok(ToolOutput::FileContent { content, .. }) => {
                                        self.last_tool_output_candidate =
                                            Some(ManualToolContext::from_read_file(
                                                &display_path,
                                                &content,
                                            ));
                                        self.push_tool_read_output(&display_path, &content);
                                        self.pending_model_tool_request = None;
                                        self.log.push(
                                            "Run /context attach-last-tool to include this tool output in the next prompt.".to_string(),
                                        );
                                    }
                                    Err(error) => {
                                        self.push_tool_error_output(error);
                                        // Keep pending_model_tool_request unchanged on failure.
                                    }
                                }
                            } else {
                                self.log.push("No pending model tool request.".to_string());
                            }
                        }
                    },
                    Command::Unknown(c) => {
                        self.event_log
                            .append(EventKind::UnknownSlashCommand, c.clone());
                        self.log.push(format!("Unknown command: {c}"));
                    }
                }
            }
            ParsedInput::UserMessage(message) => {
                self.event_log.append(EventKind::UserMessage, &message);
                let pending_context = self.pending_manual_tool_context.take();
                let output = kernel::runner::run_mock_turn(
                    &mut self.event_log,
                    &message,
                    &self.model_gateway,
                    pending_context.as_ref(),
                );
                self.log.push(format!("User: {}", output.user_message));
                if !output.assistant_response.is_empty() {
                    self.log
                        .push(format!("Assistant: {}", output.assistant_response));
                }
                if let Some(req) = &output.detected_model_tool_request {
                    self.log.extend(req.user_guidance());
                    self.pending_model_tool_request = Some(req.clone());
                }
            }
        }

        self.input.clear();
    }

    pub fn select_next(&mut self) {
        let len_before = self.event_log.len();
        if len_before == 0 {
            return;
        }
        let new_idx = match self.selected_event {
            None => 0,
            Some(i) => {
                let clamped = (i + 1).min(len_before - 1);
                if clamped == i {
                    return;
                }
                clamped
            }
        };
        self.selected_event = Some(new_idx);
        self.inspector_scroll = 0;
    }

    pub fn select_prev(&mut self) {
        let len_before = self.event_log.len();
        if len_before == 0 {
            return;
        }
        let new_idx = match self.selected_event {
            None => len_before - 1,
            Some(i) => {
                if i == 0 {
                    return;
                }
                i - 1
            }
        };
        self.selected_event = Some(new_idx);
        self.inspector_scroll = 0;
    }

    pub fn scroll_inspector_down(&mut self) {
        self.inspector_scroll = self.inspector_scroll.saturating_add(INSPECTOR_SCROLL_STEP);
    }

    pub fn scroll_inspector_up(&mut self) {
        self.inspector_scroll = self.inspector_scroll.saturating_sub(INSPECTOR_SCROLL_STEP);
    }

    pub fn help_lines() -> Vec<String> {
        vec![
            "Available commands:".to_string(),
            "  Type a message (no leading /) to send it as a user message".to_string(),
            "  /help  - show this help".to_string(),
            "  /clear - clear the log".to_string(),
            "  /exit  - exit Caravan".to_string(),
            "  /tool list [path] - list files under the workspace".to_string(),
            "  /tool read <path> - read a UTF-8 text file under the workspace".to_string(),
            "  /context attach-last-tool - attach the latest read-only tool output to the next prompt".to_string(),
            "  /context clear - clear pending manual tool context".to_string(),
            "  /context status - show pending manual tool context and last tool output".to_string(),
            "  /request status - show the pending model tool request".to_string(),
            "  /request clear - clear the pending model tool request".to_string(),
            "  /request run - execute the pending model tool request (read-only)".to_string(),
        ]
    }
}

mod logging;

#[cfg(test)]
mod tests;
