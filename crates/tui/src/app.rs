use std::path::PathBuf;

use kernel::events::{EventKind, EventLog};
use kernel::manual_context::ManualToolContext;
use kernel::model_gateway::ModelGateway;
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
        use kernel::commands::{Command, ContextCommand, ParsedInput, ToolCommand, parse_input};

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
        ]
    }

    /// Pushes a sorted directory listing to the screen log, capped at
    /// [`TOOL_LIST_PREVIEW_ENTRIES`] lines plus an overflow trailer.
    fn push_tool_list_output(&mut self, display_path: &str, mut entries: Vec<String>) {
        entries.sort();
        self.log.push(format!("Tool list {}:", display_path));
        let total = entries.len();
        for entry in entries.iter().take(TOOL_LIST_PREVIEW_ENTRIES) {
            self.log.push(format!("- {}", entry));
        }
        if total > TOOL_LIST_PREVIEW_ENTRIES {
            self.log.push(format!(
                "... and {} more",
                total - TOOL_LIST_PREVIEW_ENTRIES
            ));
        }
    }

    /// Pushes a UTF-8 content preview to the screen log, truncated to at most
    /// [`TOOL_READ_PREVIEW_BYTES`] bytes on a valid char boundary using a
    /// backward scan with [`str::is_char_boundary`].
    fn push_tool_read_output(&mut self, display_path: &str, content: &str) {
        self.log.push(format!("Tool read {}:", display_path));
        let mut limit = TOOL_READ_PREVIEW_BYTES.min(content.len());
        while limit > 0 && !content.is_char_boundary(limit) {
            limit -= 1;
        }
        let preview = &content[..limit];
        self.log.push(preview.to_string());
        if content.len() > limit {
            self.log.push("... [truncated]".to_string());
        }
    }

    /// Pushes a single human-readable error line derived from a [`kernel::ToolError`].
    fn push_tool_error_output(&mut self, error: kernel::ToolError) {
        let msg = match error {
            kernel::ToolError::WorkspaceViolation { path } => {
                format!("Tool error: path '{}' is outside the workspace", path)
            }
            kernel::ToolError::NotFound { path } => {
                format!("Tool error: '{}' not found", path)
            }
            kernel::ToolError::NotAFile { path } => {
                format!("Tool error: '{}' is not a file", path)
            }
            kernel::ToolError::NotADirectory { path } => {
                format!("Tool error: '{}' is not a directory", path)
            }
            kernel::ToolError::NonUtf8 { path } => {
                format!("Tool error: '{}' is not valid UTF-8", path)
            }
            kernel::ToolError::TooLarge { path, max_bytes } => {
                format!("Tool error: '{}' exceeds {} byte limit", path, max_bytes)
            }
            kernel::ToolError::Io { message } => {
                format!("Tool error: I/O error: {}", message)
            }
        };
        self.log.push(msg);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use kernel::events::{EventKind, EventLog, EventSeq};
    use kernel::model_runtime_config::ModelRuntimeConfig;
    use kernel::storage::EventStore;

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TempDir {
        path: std::path::PathBuf,
    }

    impl TempDir {
        fn new() -> Self {
            let count = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
            let name = format!("caravan_apptest_{}_{}", std::process::id(), count);
            let path = std::env::temp_dir().join(name);
            std::fs::create_dir_all(&path).expect("failed to create temp dir");
            TempDir { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn new_yields_app_started_event() {
        let app = App::new();
        assert_eq!(app.event_log.len(), 1);
        let ev = app.event_log.get(0).unwrap();
        assert_eq!(ev.kind, EventKind::AppStart);
        assert_eq!(ev.detail, "Caravan started.");
        assert_eq!(ev.seq, EventSeq(1));
        assert_eq!(app.selected_event, None);
    }

    #[test]
    fn push_char_and_backspace_edit_input() {
        let mut app = App::new();
        app.push_char('h');
        app.push_char('i');
        assert_eq!(app.input, "hi");
        app.backspace();
        assert_eq!(app.input, "h");
        app.backspace();
        assert_eq!(app.input, "");
        // backspace on empty input is a no-op
        app.backspace();
        assert_eq!(app.input, "");
    }

    #[test]
    fn help_appends_command_entered_then_help_requested() {
        let mut app = App::new();
        app.input = "/help".to_string();
        app.submit();
        assert_eq!(app.event_log.len(), 3);
        let ce = app.event_log.get(1).unwrap();
        assert_eq!(ce.kind, EventKind::SlashCommand);
        assert_eq!(ce.detail, "/help");
        let hr = app.event_log.get(2).unwrap();
        assert_eq!(hr.kind, EventKind::HelpRequest);
        for line in App::help_lines() {
            assert!(app.log.contains(&line), "log missing line: {}", line);
        }
    }

    #[test]
    fn plain_text_appends_user_text_then_run_turn() {
        let mut app = App::new();
        app.input = "hello".to_string();
        app.submit();

        let events = app.event_log.events();
        // First post-AppStart event is UserMessage with detail "hello"
        let first_after = events.get(1).expect("should have event after AppStart");
        assert_eq!(first_after.kind, EventKind::UserMessage);
        assert_eq!(first_after.detail, "hello");

        // NO SlashCommand event for plain text
        assert!(!events.iter().any(|e| e.kind == EventKind::SlashCommand));

        assert!(app.log.contains(&"User: hello".to_string()));
        assert!(
            app.log
                .contains(&"Assistant: Mock response for: hello".to_string())
        );

        assert!(app.input.is_empty());
    }

    #[test]
    fn unknown_command_appends_command_entered_then_unknown_command() {
        let mut app = App::new();
        app.input = "/foo".to_string();
        app.submit();
        assert_eq!(app.event_log.len(), 3);
        let ce = app.event_log.get(1).unwrap();
        assert_eq!(ce.kind, EventKind::SlashCommand);
        assert_eq!(ce.detail, "/foo");
        let uc = app.event_log.get(2).unwrap();
        assert_eq!(uc.kind, EventKind::UnknownSlashCommand);
        assert_eq!(uc.detail, "/foo");
        assert!(app.log.iter().any(|l| l.contains("Unknown command:")));
        assert!(app.input.is_empty());
    }

    #[test]
    fn clear_appends_events_empties_log_keeps_event_log() {
        let mut app = App::new();
        // Seed the screen log with some content first
        app.input = "hello".to_string();
        app.submit();
        let event_len_before = app.event_log.len();
        app.input = "/clear".to_string();
        app.submit();
        assert!(app.log.is_empty());
        assert!(app.event_log.len() > event_len_before);
        let n = app.event_log.len();
        let ce = app.event_log.get(n - 2).unwrap();
        assert_eq!(ce.kind, EventKind::SlashCommand);
        assert_eq!(ce.detail, "/clear");
        let lc = app.event_log.get(n - 1).unwrap();
        assert_eq!(lc.kind, EventKind::LogClear);
        assert!(app.input.is_empty());
    }

    #[test]
    fn exit_appends_command_entered_then_exit_requested() {
        let mut app = App::new();
        assert!(!app.should_exit);
        app.input = "/exit".to_string();
        app.submit();
        assert!(app.should_exit);
        assert_eq!(app.event_log.len(), 3);
        let ce = app.event_log.get(1).unwrap();
        assert_eq!(ce.kind, EventKind::SlashCommand);
        assert_eq!(ce.detail, "/exit");
        let qr = app.event_log.get(2).unwrap();
        assert_eq!(qr.kind, EventKind::ExitRequest);
        assert!(app.input.is_empty());
    }

    #[test]
    fn exit_from_ctrl_c_emits_exit_requested_and_sets_should_exit() {
        let mut app = App::new();
        let len_before = app.event_log.len();
        app.exit_from_ctrl_c();
        assert!(app.should_exit);
        assert_eq!(app.event_log.len(), len_before + 1);
        let last = app.event_log.get(app.event_log.len() - 1).unwrap();
        assert_eq!(last.kind, EventKind::ExitRequest);
        // No SlashCommand is emitted for a Ctrl+C exit (not a command-bar entry).
        assert!(
            !app.event_log
                .events()
                .iter()
                .any(|e| e.kind == EventKind::SlashCommand)
        );
    }

    #[test]
    fn user_message_detail_trimmed_unknown_detail_raw() {
        let mut app = App::new();
        app.input = "  hello  ".to_string();
        app.submit();
        let events = app.event_log.events();
        let ute = events
            .iter()
            .find(|e| e.kind == EventKind::UserMessage)
            .expect("UserMessage should exist");
        assert_eq!(ute.detail, "hello");

        let mut app2 = App::new();
        app2.input = "  /foo  ".to_string();
        app2.submit();
        let events2 = app2.event_log.events();
        let uc = events2
            .iter()
            .find(|e| e.kind == EventKind::UnknownSlashCommand)
            .expect("UnknownSlashCommand should exist");
        assert_eq!(uc.detail, "  /foo  ");
    }

    #[test]
    fn empty_submit_is_noop() {
        let mut app = App::new();
        let log_before = app.log.clone();
        let event_len_before = app.event_log.len();
        // input is already ""
        app.submit();
        assert_eq!(app.log, log_before);
        assert_eq!(app.event_log.len(), event_len_before);
        assert!(app.input.is_empty());
    }

    #[test]
    fn whitespace_only_submit_is_noop() {
        let mut app = App::new();
        let log_before = app.log.clone();
        let event_len_before = app.event_log.len();
        app.input = "   ".to_string();
        app.submit();
        assert_eq!(app.log, log_before);
        assert_eq!(app.event_log.len(), event_len_before);
        // input is NOT cleared
        assert_eq!(app.input, "   ");
    }

    #[test]
    fn select_next_from_fresh_app() {
        let mut app = App::new();
        let len_before = app.event_log.len(); // 1
        app.select_next();
        assert_eq!(app.selected_event, Some(0));
        // Navigation is pure UI state and must not append events.
        assert_eq!(app.event_log.len(), len_before);
    }

    #[test]
    fn select_prev_from_some_zero_is_noop() {
        let mut app = App::new();
        // Navigate to Some(0) first
        app.select_next();
        assert_eq!(app.selected_event, Some(0));
        let len_before = app.event_log.len();
        // select_prev from Some(0): already at lower boundary, no-op
        app.select_prev();
        assert_eq!(app.selected_event, Some(0));
        assert_eq!(app.event_log.len(), len_before);
    }

    #[test]
    fn select_next_at_upper_boundary_is_noop() {
        let mut app = App::new();
        // Manually set selected_event to the last valid index
        // App::new() yields len = 1, so last index = 0
        app.selected_event = Some(app.event_log.len() - 1); // Some(0)
        let len_before = app.event_log.len();
        // select_next from Some(0) where len = 1: 0 == len-1, no-op
        app.select_next();
        assert_eq!(app.selected_event, Some(0));
        assert_eq!(app.event_log.len(), len_before);
    }

    #[test]
    fn select_next_and_prev_on_empty_event_log_do_nothing() {
        let mut app = App::new();
        // Replace event_log with an empty one to simulate the hypothetical
        app.event_log = EventLog::new();
        app.selected_event = None;

        app.select_next();
        assert_eq!(app.selected_event, None);
        assert_eq!(app.event_log.len(), 0);

        app.select_prev();
        assert_eq!(app.selected_event, None);
        assert_eq!(app.event_log.len(), 0);
    }

    #[test]
    fn help_lines_exact_content() {
        let expected = vec![
            "Available commands:".to_string(),
            "  Type a message (no leading /) to send it as a user message".to_string(),
            "  /help  - show this help".to_string(),
            "  /clear - clear the log".to_string(),
            "  /exit  - exit Caravan".to_string(),
            "  /tool list [path] - list files under the workspace".to_string(),
            "  /tool read <path> - read a UTF-8 text file under the workspace".to_string(),
            "  /context attach-last-tool - attach the latest read-only tool output to the next prompt".to_string(),
            "  /context clear - clear pending manual tool context".to_string(),
        ];
        assert_eq!(App::help_lines(), expected);
    }

    #[test]
    fn with_store_restart_persists_app_started() {
        let dir = TempDir::new();

        // First run: one AppStart event persisted.
        let store1 = EventStore::new(dir.path());
        let app1 = App::with_store(store1);
        let first_event_count = app1.event_log.len(); // 1
        let first_max_seq = app1.event_log.get(first_event_count - 1).unwrap().seq.0;
        drop(app1);

        // Second run: reloads first run's events, then appends a new AppStart.
        let store2 = EventStore::new(dir.path());
        let app2 = App::with_store(store2);

        assert_eq!(app2.event_log.len(), first_event_count + 1);
        let last = app2.event_log.get(app2.event_log.len() - 1).unwrap();
        assert_eq!(last.kind, EventKind::AppStart);
        assert_eq!(last.seq.0, first_max_seq + 1);
    }

    #[test]
    fn clear_does_not_truncate_event_file() {
        let dir = TempDir::new();
        let store = EventStore::new(dir.path());
        let events_path = store.events_path();

        let mut app = App::with_store(store);

        // Write some events before /clear.
        app.input = "hello".to_string();
        app.submit();

        let events_before_clear = app.event_log.len();

        // /clear appends SlashCommand + LogClear (2 events).
        app.input = "/clear".to_string();
        app.submit();

        let content = std::fs::read_to_string(&events_path).expect("events file should exist");
        let non_empty_lines = content.lines().filter(|l| !l.is_empty()).count();

        assert_eq!(non_empty_lines, events_before_clear + 2);
    }

    #[test]
    fn submit_persists_events_to_file() {
        let dir = TempDir::new();
        let store = EventStore::new(dir.path());
        let events_path = store.events_path();

        let mut app = App::with_store(store);
        app.input = "hello world".to_string();
        app.submit();

        let content = std::fs::read_to_string(&events_path).expect("events file should exist");

        assert!(
            content.lines().any(|l| l.contains("UserMessage")),
            "events file should contain UserMessage"
        );
        assert!(
            content.lines().any(|l| l.contains("RunCreate")),
            "events file should contain RunCreate"
        );
        assert!(
            content.lines().any(|l| l.contains("RunComplete")),
            "events file should contain RunComplete"
        );
    }

    #[test]
    fn plain_text_appends_full_run_turn_sequence() {
        let mut app = App::new();
        app.input = "hello".to_string();
        app.submit();

        let events = app.event_log.events();
        assert_eq!(events[0].kind, EventKind::AppStart);

        let after_app_started = &events[1..];
        let n = "Mock response for: hello".split_whitespace().count();
        let mut expected_kinds = vec![
            EventKind::UserMessage,
            EventKind::RunCreate,
            EventKind::RunStart,
            EventKind::TurnStart,
            EventKind::PromptCompile,
            EventKind::ModelRoute,
        ];
        for _ in 0..n {
            expected_kinds.push(EventKind::ModelOutputChunk);
        }
        expected_kinds.push(EventKind::AssistantMessage);
        expected_kinds.push(EventKind::RunComplete);

        assert_eq!(after_app_started.len(), expected_kinds.len());
        for (ev, expected) in after_app_started.iter().zip(expected_kinds.iter()) {
            assert_eq!(ev.kind, *expected);
        }

        assert!(app.log.contains(&"User: hello".to_string()));
        assert!(
            app.log
                .contains(&"Assistant: Mock response for: hello".to_string())
        );
    }

    #[test]
    fn user_message_run_and_turn_ids_match_event_seqs() {
        let mut app = App::new();
        app.input = "hi".to_string();
        app.submit();

        let events = app.event_log.events();

        let run_created = events
            .iter()
            .find(|e| e.kind == EventKind::RunCreate)
            .expect("RunCreate event should exist");
        assert!(
            run_created
                .detail
                .contains(&format!("run_id=run-{}", run_created.seq)),
            "RunCreate detail should contain run_id=run-{{seq}}: {}",
            run_created.detail
        );

        let turn_started = events
            .iter()
            .find(|e| e.kind == EventKind::TurnStart)
            .expect("TurnStart event should exist");
        assert!(
            turn_started
                .detail
                .contains(&format!("turn_id=turn-{}", turn_started.seq)),
            "TurnStart detail should contain turn_id=turn-{{seq}}: {}",
            turn_started.detail
        );
    }

    #[test]
    fn user_message_events_persist_and_reload() {
        let dir = TempDir::new();

        let store1 = EventStore::new(dir.path());
        let mut app1 = App::with_store(store1);
        app1.input = "hi".to_string();
        app1.submit();
        let max_seq = app1
            .event_log
            .events()
            .iter()
            .map(|e| e.seq.0)
            .max()
            .unwrap();
        drop(app1);

        let store2 = EventStore::new(dir.path());
        let app2 = App::with_store(store2);

        let events = app2.event_log.events();
        assert!(
            events.iter().any(|e| e.kind == EventKind::RunCreate),
            "reloaded log should contain RunCreate"
        );
        assert!(
            events.iter().any(|e| e.kind == EventKind::ModelOutputChunk),
            "reloaded log should contain ModelOutputChunk"
        );
        assert!(
            events.iter().any(|e| e.kind == EventKind::RunComplete),
            "reloaded log should contain RunComplete"
        );

        // The new AppStart from the second run should have a seq past the prior max.
        let new_app_started = events
            .iter()
            .filter(|e| e.kind == EventKind::AppStart)
            .last()
            .expect("there should be an AppStart from the second run");
        assert!(
            new_app_started.seq.0 > max_seq,
            "new AppStart seq {} should be > prior max seq {}",
            new_app_started.seq.0,
            max_seq
        );
    }

    #[test]
    fn slash_ask_is_unknown_and_creates_no_run() {
        let mut app = App::new();
        app.input = "/ask hello".to_string();
        app.submit();

        let events = app.event_log.events();
        assert!(
            events
                .iter()
                .any(|e| e.kind == EventKind::UnknownSlashCommand),
            "should have UnknownSlashCommand event"
        );
        assert!(
            !events.iter().any(|e| e.kind == EventKind::RunCreate),
            "should NOT have RunCreate event for /ask"
        );
        assert!(
            !events.iter().any(|e| e.kind == EventKind::PromptCompile),
            "should NOT have PromptCompile event for /ask"
        );
    }

    #[test]
    fn prompt_compiled_detail_contains_template() {
        let mut app = App::new();
        app.input = "hello caravan".to_string();
        app.submit();

        let events = app.event_log.events();
        let pc = events
            .iter()
            .find(|e| e.kind == EventKind::PromptCompile)
            .expect("PromptCompile event should exist");

        assert!(pc.detail.contains("System:"));
        assert!(pc.detail.contains("User:"));
        assert!(pc.detail.contains("Context:"));
        assert!(pc.detail.contains("Output:"));
        assert!(pc.detail.contains("hello caravan"));
    }

    #[test]
    fn openai_gateway_records_model_error_and_run_fail() {
        let dir = TempDir::new();
        let store = EventStore::new(dir.path());

        let vars = HashMap::from([("CARAVAN_MODEL_PROVIDER".to_string(), "openai".to_string())]);
        let runtime_config = ModelRuntimeConfig::from_env_map(&vars).unwrap();
        let gateway = ModelGateway::from_runtime_config(runtime_config);

        let mut app = App::with_store_and_gateway(store, gateway);
        app.input = "hello caravan".to_string();
        app.submit();

        let events = app.event_log.events();
        assert_eq!(events[0].kind, EventKind::AppStart);

        let after_app_start = &events[1..];
        let expected_kinds = [
            EventKind::UserMessage,
            EventKind::RunCreate,
            EventKind::RunStart,
            EventKind::TurnStart,
            EventKind::PromptCompile,
            EventKind::ModelError,
            EventKind::RunFail,
        ];
        assert_eq!(after_app_start.len(), expected_kinds.len());
        for (ev, expected) in after_app_start.iter().zip(expected_kinds.iter()) {
            assert_eq!(ev.kind, *expected);
        }

        // No ModelOutputChunk, RunComplete, or ModelRoute on the error path.
        assert!(!events.iter().any(|e| e.kind == EventKind::ModelOutputChunk));
        assert!(!events.iter().any(|e| e.kind == EventKind::RunComplete));
        assert!(!events.iter().any(|e| e.kind == EventKind::ModelRoute));

        // ModelError detail contains the expected skeleton message.
        let model_error_event = events
            .iter()
            .find(|e| e.kind == EventKind::ModelError)
            .expect("ModelError event should exist");
        assert!(
            model_error_event
                .detail
                .contains("OpenAI-compatible HTTP client is a skeleton in this POC"),
            "ModelError detail should contain expected message: {}",
            model_error_event.detail
        );

        // Screen log must contain the user message line.
        assert!(app.log.contains(&"User: hello caravan".to_string()));

        // No assistant line should be pushed on the error path.
        assert!(
            !app.log.iter().any(|l| l.starts_with("Assistant:")),
            "app.log should not contain any entry starting with 'Assistant:'"
        );
    }

    #[test]
    fn blocking_gateway_missing_key_records_model_error_and_run_fail() {
        let dir = TempDir::new();
        let store = EventStore::new(dir.path());

        let vars = HashMap::from([
            ("CARAVAN_MODEL_PROVIDER".to_string(), "openai".to_string()),
            (
                "CARAVAN_OPENAI_HTTP_CLIENT".to_string(),
                "blocking".to_string(),
            ),
            (
                "CARAVAN_OPENAI_API_KEY_ENV".to_string(),
                "CARAVAN_TEST_MISSING_OPENAI_KEY_SHOULD_NOT_EXIST".to_string(),
            ),
        ]);
        let runtime_config = ModelRuntimeConfig::from_env_map(&vars).unwrap();
        let gateway = ModelGateway::from_runtime_config(runtime_config);

        let mut app = App::with_store_and_gateway(store, gateway);
        app.input = "hello caravan".to_string();
        app.submit();

        let events = app.event_log.events();
        assert_eq!(events[0].kind, EventKind::AppStart);

        let after_app_start = &events[1..];
        let expected_kinds = [
            EventKind::UserMessage,
            EventKind::RunCreate,
            EventKind::RunStart,
            EventKind::TurnStart,
            EventKind::PromptCompile,
            EventKind::ModelError,
            EventKind::RunFail,
        ];
        assert_eq!(after_app_start.len(), expected_kinds.len());
        for (ev, expected) in after_app_start.iter().zip(expected_kinds.iter()) {
            assert_eq!(ev.kind, *expected);
        }

        // No ModelOutputChunk or RunComplete on the missing-key error path.
        assert!(!events.iter().any(|e| e.kind == EventKind::ModelOutputChunk));
        assert!(!events.iter().any(|e| e.kind == EventKind::RunComplete));

        // ModelError detail contains the env var name but never a key value or Bearer header.
        let model_error_event = events
            .iter()
            .find(|e| e.kind == EventKind::ModelError)
            .expect("ModelError event should exist");
        assert!(
            model_error_event.detail.contains(
                "missing or empty API key env var: CARAVAN_TEST_MISSING_OPENAI_KEY_SHOULD_NOT_EXIST"
            ),
            "ModelError detail should contain expected message: {}",
            model_error_event.detail
        );
        assert!(
            !model_error_event.detail.contains("Bearer"),
            "ModelError detail must not contain Bearer: {}",
            model_error_event.detail
        );
    }

    #[test]
    fn new_initializes_inspector_scroll_to_zero() {
        let app = App::new();
        assert_eq!(app.inspector_scroll, 0);
    }

    #[test]
    fn with_store_initializes_inspector_scroll_to_zero() {
        let dir = TempDir::new();
        let store = EventStore::new(dir.path());
        let app = App::with_store(store);
        assert_eq!(app.inspector_scroll, 0);
    }

    #[test]
    fn scroll_inspector_down_then_up_changes_scroll_without_side_effects() {
        let mut app = App::new();
        let initial_log_len = app.event_log.len();
        let initial_selected = app.selected_event;

        app.scroll_inspector_down();
        assert_eq!(app.inspector_scroll, 3);
        assert_eq!(app.event_log.len(), initial_log_len);
        assert_eq!(app.selected_event, initial_selected);

        app.scroll_inspector_up();
        assert_eq!(app.inspector_scroll, 0);
        assert_eq!(app.event_log.len(), initial_log_len);
        assert_eq!(app.selected_event, initial_selected);
    }

    #[test]
    fn scroll_inspector_up_saturates_at_zero() {
        let mut app = App::new();
        app.inspector_scroll = 1; // below INSPECTOR_SCROLL_STEP (3)
        app.scroll_inspector_up();
        assert_eq!(app.inspector_scroll, 0);
    }

    #[test]
    fn selection_change_resets_inspector_scroll() {
        let mut app = App::new();
        app.inspector_scroll = 9;
        // select_next from None moves to Some(0) — an actual selection change.
        app.select_next();
        assert_eq!(app.inspector_scroll, 0);
    }

    #[test]
    fn noop_selection_preserves_inspector_scroll() {
        let mut app = App::new();
        // Navigate to Some(0) first.
        app.select_next();
        app.inspector_scroll = 6;
        // select_prev from Some(0) is a no-op — scroll must not reset.
        app.select_prev();
        assert_eq!(app.inspector_scroll, 6);
        assert_eq!(app.selected_event, Some(0));
    }

    #[test]
    fn with_workspace_root_constructor_sets_root() {
        let store_dir = TempDir::new();
        let workspace_dir = TempDir::new();
        let store = EventStore::new(store_dir.path());
        let workspace_root = workspace_dir.path().to_path_buf();
        let app = App::with_store_gateway_and_workspace_root(
            store,
            ModelGateway::default(),
            workspace_root.clone(),
        );
        assert_eq!(app.workspace_root, workspace_root);
    }

    #[test]
    fn tool_list_success_appends_slash_tool_call_result_events() {
        let store_dir = TempDir::new();
        let workspace_dir = TempDir::new();

        std::fs::write(workspace_dir.path().join("alpha.txt"), "a").unwrap();
        std::fs::write(workspace_dir.path().join("beta.txt"), "b").unwrap();
        std::fs::write(workspace_dir.path().join("gamma.txt"), "g").unwrap();
        let entry_count = 3;

        let store = EventStore::new(store_dir.path());
        let mut app = App::with_store_gateway_and_workspace_root(
            store,
            kernel::model_gateway::ModelGateway::default(),
            workspace_dir.path().to_path_buf(),
        );

        let event_len_before = app.event_log.len();
        app.input = "/tool list .".to_string();
        app.submit();

        assert_eq!(app.event_log.len(), event_len_before + 3);
        let events = app.event_log.events();
        let n = events.len();
        assert_eq!(events[n - 3].kind, EventKind::SlashCommand);
        assert_eq!(events[n - 2].kind, EventKind::ToolCall);
        assert_eq!(events[n - 1].kind, EventKind::ToolResult);

        assert!(app.log.iter().any(|l| l == "Tool list .:"));
        assert!(
            events[n - 1]
                .detail
                .contains(&format!("entries={}", entry_count))
        );
    }

    #[test]
    fn tool_read_success_appends_slash_tool_call_result_events() {
        let store_dir = TempDir::new();
        let workspace_dir = TempDir::new();

        let content = "hello, world!";
        std::fs::write(workspace_dir.path().join("greeting.txt"), content).unwrap();

        let store = EventStore::new(store_dir.path());
        let mut app = App::with_store_gateway_and_workspace_root(
            store,
            kernel::model_gateway::ModelGateway::default(),
            workspace_dir.path().to_path_buf(),
        );

        let event_len_before = app.event_log.len();
        app.input = "/tool read greeting.txt".to_string();
        app.submit();

        assert_eq!(app.event_log.len(), event_len_before + 3);
        let events = app.event_log.events();
        let n = events.len();
        assert_eq!(events[n - 3].kind, EventKind::SlashCommand);
        assert_eq!(events[n - 2].kind, EventKind::ToolCall);
        assert_eq!(events[n - 1].kind, EventKind::ToolResult);

        assert!(app.log.iter().any(|l| l == "Tool read greeting.txt:"));
        assert!(
            events[n - 1]
                .detail
                .contains(&format!("bytes={}", content.len()))
        );

        // No event in the event_log may contain the raw file content.
        assert!(!events.iter().any(|e| e.detail.contains(content)));
    }

    #[test]
    fn tool_read_workspace_violation_appends_slash_tool_call_error_events() {
        let store_dir = TempDir::new();
        let workspace_dir = TempDir::new();

        let store = EventStore::new(store_dir.path());
        let mut app = App::with_store_gateway_and_workspace_root(
            store,
            kernel::model_gateway::ModelGateway::default(),
            workspace_dir.path().to_path_buf(),
        );

        let log_len_before = app.log.len();
        let event_len_before = app.event_log.len();
        app.input = "/tool read ../secret.txt".to_string();
        app.submit();

        assert_eq!(app.event_log.len(), event_len_before + 3);
        let events = app.event_log.events();
        let n = events.len();
        assert_eq!(events[n - 3].kind, EventKind::SlashCommand);
        assert_eq!(events[n - 2].kind, EventKind::ToolCall);
        assert_eq!(events[n - 1].kind, EventKind::ToolError);

        // Screen log gained a readable error line.
        assert!(app.log.len() > log_len_before);
        assert!(app.log.iter().any(|l| l.contains("Tool error:")));
    }

    #[test]
    fn tool_malformed_commands_produce_only_slash_and_unknown_events() {
        for input in &["/tool", "/tool read", "/tool foo some-file"] {
            let mut app = App::new();
            let event_len_before = app.event_log.len();
            app.input = input.to_string();
            app.submit();

            assert_eq!(
                app.event_log.len(),
                event_len_before + 2,
                "expected +2 events for input: {input}"
            );

            let events = app.event_log.events();
            let n = events.len();
            assert_eq!(
                events[n - 2].kind,
                EventKind::SlashCommand,
                "expected SlashCommand for: {input}"
            );
            assert_eq!(
                events[n - 1].kind,
                EventKind::UnknownSlashCommand,
                "expected UnknownSlashCommand for: {input}"
            );
            assert_ne!(events[n - 2].kind, EventKind::ToolCall);
            assert_ne!(events[n - 1].kind, EventKind::ToolCall);
        }
    }

    #[test]
    fn tool_list_bounded_output_shows_preview_and_trailer() {
        let store_dir = TempDir::new();
        let workspace_dir = TempDir::new();

        let total = 53usize;
        for i in 0..total {
            std::fs::write(workspace_dir.path().join(format!("file_{:02}.txt", i)), "").unwrap();
        }

        let store = EventStore::new(store_dir.path());
        let mut app = App::with_store_gateway_and_workspace_root(
            store,
            kernel::model_gateway::ModelGateway::default(),
            workspace_dir.path().to_path_buf(),
        );

        app.input = "/tool list .".to_string();
        app.submit();

        let entry_lines: Vec<_> = app.log.iter().filter(|l| l.starts_with("- ")).collect();
        assert_eq!(
            entry_lines.len(),
            TOOL_LIST_PREVIEW_ENTRIES,
            "expected exactly {} entry lines",
            TOOL_LIST_PREVIEW_ENTRIES
        );

        let expected_trailer = format!("... and {} more", total - TOOL_LIST_PREVIEW_ENTRIES);
        assert!(
            app.log.iter().any(|l| l == &expected_trailer),
            "expected trailer '{}' in log",
            expected_trailer
        );
    }

    #[test]
    fn tool_read_bounded_utf8_boundary_does_not_split_multibyte_char() {
        let store_dir = TempDir::new();
        let workspace_dir = TempDir::new();

        // 4095 'a's + 'é' (2 bytes: 0xC3 0xA9) + 'Z' = 4098 bytes total.
        // At byte offset 4096 we are inside 'é', so the backward scan must
        // retreat to 4095 (the start of 'é'), which is a valid char boundary.
        let content = format!("{}\u{00e9}Z", "a".repeat(4095));
        assert_eq!(content.len(), 4098);

        std::fs::write(workspace_dir.path().join("boundary.txt"), &content).unwrap();

        let store = EventStore::new(store_dir.path());
        let mut app = App::with_store_gateway_and_workspace_root(
            store,
            kernel::model_gateway::ModelGateway::default(),
            workspace_dir.path().to_path_buf(),
        );

        // Must not panic.
        app.input = "/tool read boundary.txt".to_string();
        app.submit();

        // Find the preview line immediately after the header.
        let header_pos = app
            .log
            .iter()
            .position(|l| l == "Tool read boundary.txt:")
            .expect("header line must be present");
        let preview_line = &app.log[header_pos + 1];

        // Preview stops at byte 4095 — the 'é' must not be split.
        assert_eq!(preview_line.len(), 4095, "preview must stop before 'é'");
        assert!(
            !preview_line.contains('\u{00e9}'),
            "preview must not contain 'é'"
        );

        // Truncation marker must be present.
        assert!(
            app.log.iter().any(|l| l == "... [truncated]"),
            "log must contain '... [truncated]'"
        );
    }

    #[test]
    fn plain_text_produces_no_tool_events_and_correct_model_route() {
        let mut app = App::new();

        app.input = "hello caravan".to_string();
        app.submit();

        let events = app.event_log.events();

        assert!(
            !events.iter().any(|e| e.kind == EventKind::ToolCall),
            "plain text must not produce ToolCall events"
        );
        assert!(
            !events.iter().any(|e| e.kind == EventKind::ToolResult),
            "plain text must not produce ToolResult events"
        );
        assert!(
            !events.iter().any(|e| e.kind == EventKind::ToolError),
            "plain text must not produce ToolError events"
        );

        let model_route = events
            .iter()
            .find(|e| e.kind == EventKind::ModelRoute)
            .expect("ModelRoute event should exist");
        assert_eq!(
            model_route.detail,
            "provider=mock model=mock-model adapter=MockModelAdapter"
        );
    }

    // --- /context command tests ---

    #[test]
    fn tool_read_success_stores_last_tool_output_candidate() {
        let store_dir = TempDir::new();
        let workspace_dir = TempDir::new();

        std::fs::write(workspace_dir.path().join("file.txt"), "hello world").unwrap();

        let store = EventStore::new(store_dir.path());
        let mut app = App::with_store_gateway_and_workspace_root(
            store,
            kernel::model_gateway::ModelGateway::default(),
            workspace_dir.path().to_path_buf(),
        );

        assert!(app.last_tool_output_candidate.is_none());
        app.input = "/tool read file.txt".to_string();
        app.submit();

        assert!(
            app.last_tool_output_candidate.is_some(),
            "last_tool_output_candidate should be set after /tool read success"
        );
    }

    #[test]
    fn tool_list_success_stores_last_tool_output_candidate() {
        let store_dir = TempDir::new();
        let workspace_dir = TempDir::new();

        std::fs::write(workspace_dir.path().join("alpha.txt"), "a").unwrap();

        let store = EventStore::new(store_dir.path());
        let mut app = App::with_store_gateway_and_workspace_root(
            store,
            kernel::model_gateway::ModelGateway::default(),
            workspace_dir.path().to_path_buf(),
        );

        assert!(app.last_tool_output_candidate.is_none());
        app.input = "/tool list .".to_string();
        app.submit();

        assert!(
            app.last_tool_output_candidate.is_some(),
            "last_tool_output_candidate should be set after /tool list success"
        );
    }

    #[test]
    fn tool_read_failure_does_not_update_candidate() {
        let store_dir = TempDir::new();
        let workspace_dir = TempDir::new();

        let store = EventStore::new(store_dir.path());
        let mut app = App::with_store_gateway_and_workspace_root(
            store,
            kernel::model_gateway::ModelGateway::default(),
            workspace_dir.path().to_path_buf(),
        );

        // Try to read a file that does not exist — this is an error path.
        app.input = "/tool read nonexistent.txt".to_string();
        app.submit();

        assert!(
            app.last_tool_output_candidate.is_none(),
            "last_tool_output_candidate must remain None after a failed /tool"
        );
    }

    #[test]
    fn context_attach_with_candidate_appends_tool_context_attach_and_no_run() {
        let store_dir = TempDir::new();
        let workspace_dir = TempDir::new();
        std::fs::write(workspace_dir.path().join("data.txt"), "some content").unwrap();

        let store = EventStore::new(store_dir.path());
        let mut app = App::with_store_gateway_and_workspace_root(
            store,
            kernel::model_gateway::ModelGateway::default(),
            workspace_dir.path().to_path_buf(),
        );

        // Populate the candidate by reading a file.
        app.input = "/tool read data.txt".to_string();
        app.submit();

        let event_len_before = app.event_log.len();
        app.input = "/context attach-last-tool".to_string();
        app.submit();

        let events = app.event_log.events();
        assert!(
            events
                .iter()
                .any(|e| e.kind == EventKind::ToolContextAttach),
            "ToolContextAttach event should be present"
        );
        assert!(
            !events.iter().any(|e| e.kind == EventKind::RunCreate),
            "/context attach-last-tool must not start a model run"
        );
        assert!(
            !events.iter().any(|e| e.kind == EventKind::RunComplete),
            "/context attach-last-tool must not complete a model run"
        );
        assert!(
            !events.iter().any(|e| e.kind == EventKind::AssistantMessage),
            "/context attach-last-tool must not produce AssistantMessage"
        );
        // SlashCommand + ToolContextAttach = 2 events appended.
        assert_eq!(app.event_log.len(), event_len_before + 2);
        assert!(app.pending_manual_tool_context.is_some());
    }

    #[test]
    fn context_attach_without_candidate_appends_no_tool_context_attach_and_no_run() {
        let mut app = App::new();
        assert!(app.last_tool_output_candidate.is_none());

        let event_len_before = app.event_log.len();
        app.input = "/context attach-last-tool".to_string();
        app.submit(); // must not panic

        let events = app.event_log.events();
        assert!(
            !events
                .iter()
                .any(|e| e.kind == EventKind::ToolContextAttach),
            "no ToolContextAttach should be emitted when there is no candidate"
        );
        assert!(
            !events.iter().any(|e| e.kind == EventKind::RunCreate),
            "/context attach-last-tool with no candidate must not start a run"
        );
        // SlashCommand only (no ToolContextAttach).
        assert_eq!(app.event_log.len(), event_len_before + 1);
        assert!(app.pending_manual_tool_context.is_none());
    }

    #[test]
    fn context_clear_appends_tool_context_clear_and_no_run() {
        let mut app = App::new();

        let event_len_before = app.event_log.len();
        app.input = "/context clear".to_string();
        app.submit();

        let events = app.event_log.events();
        assert!(
            events.iter().any(|e| e.kind == EventKind::ToolContextClear),
            "ToolContextClear event should be present"
        );
        assert!(
            !events.iter().any(|e| e.kind == EventKind::RunCreate),
            "/context clear must not start a model run"
        );
        // SlashCommand + ToolContextClear = 2 events.
        assert_eq!(app.event_log.len(), event_len_before + 2);
        assert!(app.pending_manual_tool_context.is_none());
    }

    #[test]
    fn context_clear_when_nothing_pending_does_not_panic() {
        let mut app = App::new();
        assert!(app.pending_manual_tool_context.is_none());
        app.input = "/context clear".to_string();
        app.submit(); // must not panic
        assert!(app.pending_manual_tool_context.is_none());
    }

    #[test]
    fn after_attach_user_message_prompt_compile_contains_manual_tool_context() {
        let store_dir = TempDir::new();
        let workspace_dir = TempDir::new();
        let file_content = "bounded file body";
        std::fs::write(workspace_dir.path().join("notes.txt"), file_content).unwrap();

        let store = EventStore::new(store_dir.path());
        let mut app = App::with_store_gateway_and_workspace_root(
            store,
            kernel::model_gateway::ModelGateway::default(),
            workspace_dir.path().to_path_buf(),
        );

        app.input = "/tool read notes.txt".to_string();
        app.submit();

        app.input = "/context attach-last-tool".to_string();
        app.submit();

        // The pending context should be set.
        assert!(app.pending_manual_tool_context.is_some());

        app.input = "tell me about the file".to_string();
        app.submit();

        // After the user message, the context should be cleared (one-shot).
        assert!(app.pending_manual_tool_context.is_none());

        let events = app.event_log.events();
        let pc = events
            .iter()
            .filter(|e| e.kind == EventKind::PromptCompile)
            .next()
            .expect("PromptCompile event should exist");

        // The bounded content must appear in the Context section (after "Context:").
        assert!(
            pc.detail.contains("Manual Tool Context:"),
            "PromptCompile must contain Manual Tool Context: section"
        );
        assert!(
            pc.detail.contains(file_content),
            "PromptCompile must contain bounded file content"
        );

        // The bounded content must NOT appear in the Conversation section.
        let conversation_section = pc
            .detail
            .split("\n\nCurrent User:")
            .next()
            .expect("Conversation section must exist");
        assert!(
            !conversation_section.contains(file_content),
            "bounded content must not appear in the Conversation section"
        );
    }

    #[test]
    fn after_attach_second_user_message_does_not_reuse_context() {
        let store_dir = TempDir::new();
        let workspace_dir = TempDir::new();
        let file_content = "unique attached content xyz";
        std::fs::write(workspace_dir.path().join("unique.txt"), file_content).unwrap();

        let store = EventStore::new(store_dir.path());
        let mut app = App::with_store_gateway_and_workspace_root(
            store,
            kernel::model_gateway::ModelGateway::default(),
            workspace_dir.path().to_path_buf(),
        );

        app.input = "/tool read unique.txt".to_string();
        app.submit();

        app.input = "/context attach-last-tool".to_string();
        app.submit();

        // First user message consumes the context.
        app.input = "first message".to_string();
        app.submit();

        // Second user message must NOT have the context.
        app.input = "second message".to_string();
        app.submit();

        let events = app.event_log.events();
        let second_pc = events
            .iter()
            .filter(|e| e.kind == EventKind::PromptCompile)
            .nth(1)
            .expect("second PromptCompile event should exist");

        assert!(
            !second_pc.detail.contains("Manual Tool Context:"),
            "second PromptCompile must NOT contain Manual Tool Context"
        );
        assert!(
            !second_pc.detail.contains(file_content),
            "second PromptCompile must NOT contain bounded content (one-shot consumed)"
        );
    }

    #[test]
    fn one_shot_auto_clear_emits_no_tool_context_clear_event() {
        let store_dir = TempDir::new();
        let workspace_dir = TempDir::new();
        std::fs::write(workspace_dir.path().join("sample.txt"), "data").unwrap();

        let store = EventStore::new(store_dir.path());
        let mut app = App::with_store_gateway_and_workspace_root(
            store,
            kernel::model_gateway::ModelGateway::default(),
            workspace_dir.path().to_path_buf(),
        );

        app.input = "/tool read sample.txt".to_string();
        app.submit();

        app.input = "/context attach-last-tool".to_string();
        app.submit();

        // Verify ToolContextClear is NOT in the log yet.
        assert!(
            !app.event_log
                .events()
                .iter()
                .any(|e| e.kind == EventKind::ToolContextClear),
            "no ToolContextClear before user message"
        );

        // Consuming user message — triggers one-shot clear internally.
        app.input = "consume the context".to_string();
        app.submit();

        // ToolContextClear must still NOT appear (auto-clear emits no event).
        assert!(
            !app.event_log
                .events()
                .iter()
                .any(|e| e.kind == EventKind::ToolContextClear),
            "one-shot auto-clear must NOT emit ToolContextClear event"
        );
    }

    #[test]
    fn attached_content_does_not_appear_in_assistant_message_or_transcript() {
        let store_dir = TempDir::new();
        let workspace_dir = TempDir::new();
        let file_content = "super_secret_content_xyz_12345";
        std::fs::write(workspace_dir.path().join("secret.txt"), file_content).unwrap();

        let store = EventStore::new(store_dir.path());
        let mut app = App::with_store_gateway_and_workspace_root(
            store,
            kernel::model_gateway::ModelGateway::default(),
            workspace_dir.path().to_path_buf(),
        );

        app.input = "/tool read secret.txt".to_string();
        app.submit();

        app.input = "/context attach-last-tool".to_string();
        app.submit();

        app.input = "what is in the file".to_string();
        app.submit();

        let events = app.event_log.events();

        // AssistantMessage detail must not contain the raw file content.
        for ev in events
            .iter()
            .filter(|e| e.kind == EventKind::AssistantMessage)
        {
            assert!(
                !ev.detail.contains(file_content),
                "AssistantMessage must not contain raw file content: {}",
                ev.detail
            );
        }

        // ConversationTranscript projection must not contain the raw file content.
        let transcript = kernel::transcript::ConversationTranscript::from_event_log(&app.event_log);
        for msg in &transcript.messages {
            assert!(
                !msg.content.contains(file_content),
                "ConversationTranscript must not contain raw file content: {}",
                msg.content
            );
        }
    }

    #[test]
    fn tool_context_attach_event_detail_does_not_contain_full_file_content() {
        let store_dir = TempDir::new();
        let workspace_dir = TempDir::new();
        let file_content = "very_unique_raw_content_do_not_expose_9999";
        std::fs::write(workspace_dir.path().join("expose.txt"), file_content).unwrap();

        let store = EventStore::new(store_dir.path());
        let mut app = App::with_store_gateway_and_workspace_root(
            store,
            kernel::model_gateway::ModelGateway::default(),
            workspace_dir.path().to_path_buf(),
        );

        app.input = "/tool read expose.txt".to_string();
        app.submit();

        app.input = "/context attach-last-tool".to_string();
        app.submit();

        let events = app.event_log.events();
        let attach_ev = events
            .iter()
            .find(|e| e.kind == EventKind::ToolContextAttach)
            .expect("ToolContextAttach event should exist");

        assert!(
            !attach_ev.detail.contains(file_content),
            "ToolContextAttach detail must not contain raw file content: {}",
            attach_ev.detail
        );
        assert!(
            attach_ev.detail.contains("bytes="),
            "ToolContextAttach detail should contain bytes="
        );
    }

    #[test]
    fn hello_caravan_mock_flow_yields_expected_response_and_model_route() {
        let mut app = App::new();
        app.input = "hello caravan".to_string();
        app.submit();

        assert!(
            app.log
                .contains(&"Assistant: Mock response for: hello caravan".to_string()),
            "log should contain expected mock response"
        );

        let events = app.event_log.events();
        let model_route = events
            .iter()
            .find(|e| e.kind == EventKind::ModelRoute)
            .expect("ModelRoute event should exist");
        assert_eq!(
            model_route.detail,
            "provider=mock model=mock-model adapter=MockModelAdapter"
        );
    }

    #[test]
    fn context_unknown_produces_unknown_slash_command() {
        let mut app = App::new();
        app.input = "/context unknown".to_string();
        app.submit();

        let events = app.event_log.events();
        assert!(
            events
                .iter()
                .any(|e| e.kind == EventKind::UnknownSlashCommand),
            "/context unknown must produce UnknownSlashCommand event"
        );
        assert!(
            !events.iter().any(|e| e.kind == EventKind::RunCreate),
            "/context unknown must not start a run"
        );
    }

    // --- /context status tests ---

    #[test]
    fn context_status_appends_only_slash_command_and_no_run() {
        let mut app = App::new();
        let before = app.event_log.len();
        app.input = "/context status".to_string();
        app.submit();

        assert_eq!(
            app.event_log.len(),
            before + 1,
            "only one SlashCommand event should be appended"
        );

        let events = app.event_log.events();
        let last = &events[events.len() - 1];
        assert_eq!(last.kind, EventKind::SlashCommand);

        assert!(!events.iter().any(|e| e.kind == EventKind::RunCreate));
        assert!(!events.iter().any(|e| e.kind == EventKind::RunComplete));
        assert!(!events.iter().any(|e| e.kind == EventKind::AssistantMessage));
        assert!(
            !events
                .iter()
                .any(|e| e.kind == EventKind::ToolContextAttach)
        );
        assert!(!events.iter().any(|e| e.kind == EventKind::ToolContextClear));
    }

    #[test]
    fn context_status_no_context_shows_none() {
        let mut app = App::new();
        app.input = "/context status".to_string();
        app.submit();

        assert!(
            app.log.contains(&"- pending: none".to_string()),
            "log should contain '- pending: none'"
        );
        assert!(
            app.log.contains(&"- last tool output: none".to_string()),
            "log should contain '- last tool output: none'"
        );
    }

    #[test]
    fn context_status_candidate_only_shows_pending_none() {
        let store_dir = TempDir::new();
        let workspace_dir = TempDir::new();
        std::fs::write(workspace_dir.path().join("file.txt"), "some content").unwrap();

        let store = EventStore::new(store_dir.path());
        let mut app = App::with_store_gateway_and_workspace_root(
            store,
            kernel::model_gateway::ModelGateway::default(),
            workspace_dir.path().to_path_buf(),
        );

        // Populate last_tool_output_candidate but do NOT attach.
        app.input = "/tool read file.txt".to_string();
        app.submit();

        app.input = "/context status".to_string();
        app.submit();

        assert!(
            app.log.contains(&"- pending: none".to_string()),
            "pending should be none when nothing is attached"
        );
        assert!(
            app.log
                .iter()
                .any(|l| l.starts_with("- last tool output: source=")),
            "last tool output line should start with 'source='"
        );
    }

    #[test]
    fn context_status_pending_shows_pending_summary() {
        let store_dir = TempDir::new();
        let workspace_dir = TempDir::new();
        std::fs::write(workspace_dir.path().join("data.txt"), "hello world").unwrap();

        let store = EventStore::new(store_dir.path());
        let mut app = App::with_store_gateway_and_workspace_root(
            store,
            kernel::model_gateway::ModelGateway::default(),
            workspace_dir.path().to_path_buf(),
        );

        app.input = "/tool read data.txt".to_string();
        app.submit();

        app.input = "/context attach-last-tool".to_string();
        app.submit();

        app.input = "/context status".to_string();
        app.submit();

        assert!(
            app.log.iter().any(|l| l.starts_with("- pending: source=")),
            "pending line should start with 'source=' after attach"
        );
    }

    #[test]
    fn context_status_pending_and_candidate_both_present() {
        let store_dir = TempDir::new();
        let workspace_dir = TempDir::new();

        // fileA and fileB have distinct paths and contents so their summaries differ.
        std::fs::write(workspace_dir.path().join("fileA.txt"), "content of file A").unwrap();
        std::fs::write(
            workspace_dir.path().join("fileB.txt"),
            "content of file B different",
        )
        .unwrap();

        let store = EventStore::new(store_dir.path());
        let mut app = App::with_store_gateway_and_workspace_root(
            store,
            kernel::model_gateway::ModelGateway::default(),
            workspace_dir.path().to_path_buf(),
        );

        // Read fileA and attach it (pending = fileA's summary).
        app.input = "/tool read fileA.txt".to_string();
        app.submit();
        let file_a_summary = app
            .last_tool_output_candidate
            .as_ref()
            .unwrap()
            .attach_summary();

        app.input = "/context attach-last-tool".to_string();
        app.submit();

        // Read fileB (last candidate = fileB's summary, pending still fileA).
        app.input = "/tool read fileB.txt".to_string();
        app.submit();
        let file_b_summary = app
            .last_tool_output_candidate
            .as_ref()
            .unwrap()
            .attach_summary();

        assert_ne!(
            file_a_summary, file_b_summary,
            "fileA and fileB summaries must differ for this test to be meaningful"
        );

        app.input = "/context status".to_string();
        app.submit();

        // Find the "Context status:" line in the log.
        let status_pos = app
            .log
            .iter()
            .rposition(|l| l == "Context status:")
            .expect("'Context status:' line must be present");

        assert_eq!(app.log[status_pos], "Context status:");
        assert_eq!(
            app.log[status_pos + 1],
            format!("- pending: {}", file_a_summary),
            "pending line must contain fileA summary"
        );
        assert_eq!(
            app.log[status_pos + 2],
            format!("- last tool output: {}", file_b_summary),
            "last tool output line must contain fileB summary"
        );
    }

    #[test]
    fn context_status_does_not_mutate_context_state() {
        let store_dir = TempDir::new();
        let workspace_dir = TempDir::new();
        std::fs::write(workspace_dir.path().join("note.txt"), "important data").unwrap();

        let store = EventStore::new(store_dir.path());
        let mut app = App::with_store_gateway_and_workspace_root(
            store,
            kernel::model_gateway::ModelGateway::default(),
            workspace_dir.path().to_path_buf(),
        );

        // Stage both pending and candidate.
        app.input = "/tool read note.txt".to_string();
        app.submit();

        app.input = "/context attach-last-tool".to_string();
        app.submit();

        assert!(app.pending_manual_tool_context.is_some());
        assert!(app.last_tool_output_candidate.is_some());

        let pending_summary_before = app
            .pending_manual_tool_context
            .as_ref()
            .unwrap()
            .attach_summary();
        let candidate_summary_before = app
            .last_tool_output_candidate
            .as_ref()
            .unwrap()
            .attach_summary();

        // Run /context status — must not mutate either field.
        app.input = "/context status".to_string();
        app.submit();

        assert!(
            app.pending_manual_tool_context.is_some(),
            "pending_manual_tool_context must remain Some after /context status"
        );
        assert!(
            app.last_tool_output_candidate.is_some(),
            "last_tool_output_candidate must remain Some after /context status"
        );
        assert_eq!(
            app.pending_manual_tool_context
                .as_ref()
                .unwrap()
                .attach_summary(),
            pending_summary_before,
            "pending context must be unchanged"
        );
        assert_eq!(
            app.last_tool_output_candidate
                .as_ref()
                .unwrap()
                .attach_summary(),
            candidate_summary_before,
            "last tool output candidate must be unchanged"
        );

        // A subsequent user message should still consume the pending context.
        app.input = "use the context".to_string();
        app.submit();

        assert!(
            app.pending_manual_tool_context.is_none(),
            "pending context must be consumed (None) after the user message"
        );
    }

    #[test]
    fn context_status_output_excludes_raw_content() {
        let store_dir = TempDir::new();
        let workspace_dir = TempDir::new();
        let raw_content = "raw_secret_xyz_do_not_expose_in_status_98765";
        std::fs::write(workspace_dir.path().join("secret.txt"), raw_content).unwrap();

        let store = EventStore::new(store_dir.path());
        let mut app = App::with_store_gateway_and_workspace_root(
            store,
            kernel::model_gateway::ModelGateway::default(),
            workspace_dir.path().to_path_buf(),
        );

        app.input = "/tool read secret.txt".to_string();
        app.submit();

        app.input = "/context status".to_string();
        app.submit();

        // Find the three status lines in the log.
        let status_pos = app
            .log
            .iter()
            .rposition(|l| l == "Context status:")
            .expect("'Context status:' line must be present");

        for i in 0..3 {
            assert!(
                !app.log[status_pos + i].contains(raw_content),
                "status line {} must not contain raw file content: {}",
                i,
                app.log[status_pos + i]
            );
        }
    }

    #[test]
    fn prompt_context_survives_clear() {
        // /clear is a screen-only command: it clears the visible log but not the
        // event log, so prior conversation must still appear in a later turn's
        // compiled prompt. This locks /clear as NOT a prompt-context boundary.
        let mut app = App::new();

        app.input = "first".to_string();
        app.submit();

        app.input = "/clear".to_string();
        app.submit();

        app.input = "second".to_string();
        app.submit();

        let events = app.event_log.events();
        let second_pc = events
            .iter()
            .filter(|e| e.kind == EventKind::PromptCompile)
            .nth(1)
            .expect("second PromptCompile event should exist");

        assert!(second_pc.detail.contains("User: first"));
        assert!(
            second_pc
                .detail
                .contains("Assistant: Mock response for: first")
        );
        assert!(second_pc.detail.contains("Current User:\nsecond"));
        assert!(!second_pc.detail.contains("No prior conversation context."));
    }
}
