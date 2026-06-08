use crate::events::{EventKind, EventLog, RunId, TurnId};
use crate::storage::EventStore;

pub struct App {
    pub log: Vec<String>,
    pub input: String,
    pub should_exit: bool,
    pub event_log: EventLog,
    pub selected_event: Option<usize>,
}

impl App {
    pub fn new() -> Self {
        let mut event_log = EventLog::new();
        event_log.append(EventKind::AppStarted, "Caravan started.");
        Self {
            log: vec!["Caravan started.".to_string()],
            input: String::new(),
            should_exit: false,
            event_log,
            selected_event: None,
        }
    }

    /// Constructs a store-backed app: loads existing events from `store`, appends
    /// `AppStarted` (which is persisted at `seq = max_loaded + 1`), and returns
    /// a fresh app whose screen log starts with only "Caravan started.".
    pub fn with_store(store: EventStore) -> App {
        let mut event_log = EventLog::load_from(store);
        event_log.append(EventKind::AppStarted, "Caravan started.");
        App {
            log: vec!["Caravan started.".to_string()],
            input: String::new(),
            should_exit: false,
            event_log,
            selected_event: None,
        }
    }

    pub fn push_char(&mut self, c: char) {
        self.input.push(c);
    }

    pub fn backspace(&mut self) {
        self.input.pop();
    }

    /// Records a Ctrl+C exit as an `ExitRequested` event and sets `should_exit`.
    /// Ctrl+C is not a command-bar entry, so no `CommandEntered` event is emitted.
    pub fn exit_from_ctrl_c(&mut self) {
        self.event_log
            .append(EventKind::ExitRequested, "Exit requested (Ctrl+C)");
        self.should_exit = true;
    }

    pub fn submit(&mut self) {
        use crate::commands::{Command, ParsedInput, parse_input};

        let raw = self.input.clone();
        match parse_input(&raw) {
            ParsedInput::Empty => return,
            ParsedInput::SlashCommand(cmd) => {
                self.event_log.append(EventKind::CommandEntered, &raw);
                match cmd {
                    Command::Help => {
                        let help_text = Self::help_lines().join(" ");
                        self.event_log.append(EventKind::HelpRequested, help_text);
                        self.log.extend(Self::help_lines());
                    }
                    Command::Clear => {
                        self.event_log
                            .append(EventKind::LogCleared, "Screen log cleared");
                        self.log.clear();
                    }
                    Command::Exit => {
                        self.event_log
                            .append(EventKind::ExitRequested, "Exit requested");
                        self.should_exit = true;
                    }
                    Command::Unknown(c) => {
                        self.event_log.append(EventKind::UnknownCommand, c.clone());
                        self.log.push(format!("Unknown command: {c}"));
                    }
                }
            }
            ParsedInput::UserMessage(message) => {
                self.event_log.append(EventKind::UserTextEntered, &message);
                let run_id_n = self.event_log.next_seq_value();
                let run_id = RunId(format!("run-{}", run_id_n));
                self.event_log.append(
                    EventKind::RunCreated,
                    format!("run_id={} input=\"{}\"", run_id, message),
                );
                self.event_log
                    .append(EventKind::RunStarted, format!("run_id={}", run_id));
                let turn_id_n = self.event_log.next_seq_value();
                let turn_id = TurnId(format!("turn-{}", turn_id_n));
                self.event_log.append(
                    EventKind::TurnStarted,
                    format!("run_id={} turn_id={}", run_id, turn_id),
                );
                self.event_log.append(
                    EventKind::PromptCompiled,
                    crate::prompt::compile_prompt(&message),
                );
                let mock_response = format!("Mock response for: {}", message);
                for word in mock_response.split_whitespace() {
                    self.event_log.append(
                        EventKind::ModelToken,
                        format!("run_id={} turn_id={} text=\"{}\"", run_id, turn_id, word),
                    );
                }
                self.event_log.append(
                    EventKind::RunCompleted,
                    format!("run_id={} outcome=ok", run_id),
                );
                self.log.push(format!("User: {}", message));
                self.log
                    .push(format!("Assistant: Mock response for: {}", message));
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
        let seq = self
            .event_log
            .get(new_idx)
            .expect("index is within len_before")
            .seq;
        self.event_log.append(
            EventKind::InspectorSelectionChanged,
            format!("Selected seq {}", seq),
        );
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
        let seq = self
            .event_log
            .get(new_idx)
            .expect("index is within len_before")
            .seq;
        self.event_log.append(
            EventKind::InspectorSelectionChanged,
            format!("Selected seq {}", seq),
        );
    }

    pub fn help_lines() -> Vec<String> {
        vec![
            "Available commands:".to_string(),
            "  Type a message (no leading /) to send it as a user message".to_string(),
            "  /help  - show this help".to_string(),
            "  /clear - clear the log".to_string(),
            "  /exit  - exit Caravan".to_string(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::events::{EventKind, EventLog, EventSeq};
    use crate::storage::EventStore;

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
        assert_eq!(ev.kind, EventKind::AppStarted);
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
        assert_eq!(ce.kind, EventKind::CommandEntered);
        assert_eq!(ce.detail, "/help");
        let hr = app.event_log.get(2).unwrap();
        assert_eq!(hr.kind, EventKind::HelpRequested);
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
        // First post-AppStarted event is UserTextEntered with detail "hello"
        let first_after = events.get(1).expect("should have event after AppStarted");
        assert_eq!(first_after.kind, EventKind::UserTextEntered);
        assert_eq!(first_after.detail, "hello");

        // NO CommandEntered event for plain text
        assert!(!events.iter().any(|e| e.kind == EventKind::CommandEntered));

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
        assert_eq!(ce.kind, EventKind::CommandEntered);
        assert_eq!(ce.detail, "/foo");
        let uc = app.event_log.get(2).unwrap();
        assert_eq!(uc.kind, EventKind::UnknownCommand);
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
        assert_eq!(ce.kind, EventKind::CommandEntered);
        assert_eq!(ce.detail, "/clear");
        let lc = app.event_log.get(n - 1).unwrap();
        assert_eq!(lc.kind, EventKind::LogCleared);
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
        assert_eq!(ce.kind, EventKind::CommandEntered);
        assert_eq!(ce.detail, "/exit");
        let qr = app.event_log.get(2).unwrap();
        assert_eq!(qr.kind, EventKind::ExitRequested);
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
        assert_eq!(last.kind, EventKind::ExitRequested);
        // No CommandEntered is emitted for a Ctrl+C exit (not a command-bar entry).
        assert!(
            !app.event_log
                .events()
                .iter()
                .any(|e| e.kind == EventKind::CommandEntered)
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
            .find(|e| e.kind == EventKind::UserTextEntered)
            .expect("UserTextEntered should exist");
        assert_eq!(ute.detail, "hello");

        let mut app2 = App::new();
        app2.input = "  /foo  ".to_string();
        app2.submit();
        let events2 = app2.event_log.events();
        let uc = events2
            .iter()
            .find(|e| e.kind == EventKind::UnknownCommand)
            .expect("UnknownCommand should exist");
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
        assert_eq!(app.event_log.len(), len_before + 1);
        let new_ev = app.event_log.get(app.event_log.len() - 1).unwrap();
        assert_eq!(new_ev.kind, EventKind::InspectorSelectionChanged);
        assert_eq!(new_ev.detail, "Selected seq 1");
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
        ];
        assert_eq!(App::help_lines(), expected);
    }

    #[test]
    fn with_store_restart_persists_app_started() {
        let dir = TempDir::new();

        // First run: one AppStarted event persisted.
        let store1 = EventStore::new(dir.path());
        let app1 = App::with_store(store1);
        let first_event_count = app1.event_log.len(); // 1
        let first_max_seq = app1.event_log.get(first_event_count - 1).unwrap().seq.0;
        drop(app1);

        // Second run: reloads first run's events, then appends a new AppStarted.
        let store2 = EventStore::new(dir.path());
        let app2 = App::with_store(store2);

        assert_eq!(app2.event_log.len(), first_event_count + 1);
        let last = app2.event_log.get(app2.event_log.len() - 1).unwrap();
        assert_eq!(last.kind, EventKind::AppStarted);
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

        // /clear appends CommandEntered + LogCleared (2 events).
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
            content.lines().any(|l| l.contains("UserTextEntered")),
            "events file should contain UserTextEntered"
        );
        assert!(
            content.lines().any(|l| l.contains("RunCreated")),
            "events file should contain RunCreated"
        );
        assert!(
            content.lines().any(|l| l.contains("RunCompleted")),
            "events file should contain RunCompleted"
        );
    }

    #[test]
    fn plain_text_appends_full_run_turn_sequence() {
        let mut app = App::new();
        app.input = "hello".to_string();
        app.submit();

        let events = app.event_log.events();
        assert_eq!(events[0].kind, EventKind::AppStarted);

        let after_app_started = &events[1..];
        let n = "Mock response for: hello".split_whitespace().count();
        let mut expected_kinds = vec![
            EventKind::UserTextEntered,
            EventKind::RunCreated,
            EventKind::RunStarted,
            EventKind::TurnStarted,
            EventKind::PromptCompiled,
        ];
        for _ in 0..n {
            expected_kinds.push(EventKind::ModelToken);
        }
        expected_kinds.push(EventKind::RunCompleted);

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
            .find(|e| e.kind == EventKind::RunCreated)
            .expect("RunCreated event should exist");
        assert!(
            run_created
                .detail
                .contains(&format!("run_id=run-{}", run_created.seq)),
            "RunCreated detail should contain run_id=run-{{seq}}: {}",
            run_created.detail
        );

        let turn_started = events
            .iter()
            .find(|e| e.kind == EventKind::TurnStarted)
            .expect("TurnStarted event should exist");
        assert!(
            turn_started
                .detail
                .contains(&format!("turn_id=turn-{}", turn_started.seq)),
            "TurnStarted detail should contain turn_id=turn-{{seq}}: {}",
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
            events.iter().any(|e| e.kind == EventKind::RunCreated),
            "reloaded log should contain RunCreated"
        );
        assert!(
            events.iter().any(|e| e.kind == EventKind::ModelToken),
            "reloaded log should contain ModelToken"
        );
        assert!(
            events.iter().any(|e| e.kind == EventKind::RunCompleted),
            "reloaded log should contain RunCompleted"
        );

        // The new AppStarted from the second run should have a seq past the prior max.
        let new_app_started = events
            .iter()
            .filter(|e| e.kind == EventKind::AppStarted)
            .last()
            .expect("there should be an AppStarted from the second run");
        assert!(
            new_app_started.seq.0 > max_seq,
            "new AppStarted seq {} should be > prior max seq {}",
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
            events.iter().any(|e| e.kind == EventKind::UnknownCommand),
            "should have UnknownCommand event"
        );
        assert!(
            !events.iter().any(|e| e.kind == EventKind::RunCreated),
            "should NOT have RunCreated event for /ask"
        );
        assert!(
            !events.iter().any(|e| e.kind == EventKind::PromptCompiled),
            "should NOT have PromptCompiled event for /ask"
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
            .find(|e| e.kind == EventKind::PromptCompiled)
            .expect("PromptCompiled event should exist");

        assert!(pc.detail.contains("System:"));
        assert!(pc.detail.contains("User:"));
        assert!(pc.detail.contains("Context:"));
        assert!(pc.detail.contains("Output:"));
        assert!(pc.detail.contains("hello caravan"));
    }
}
