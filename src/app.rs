use crate::events::{EventKind, EventLog};

pub struct App {
    pub log: Vec<String>,
    pub input: String,
    pub should_quit: bool,
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
            should_quit: false,
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

    pub fn submit(&mut self) {
        use crate::commands::{Command, parse_input};

        let raw = self.input.clone();
        match parse_input(&raw) {
            Command::Empty => return,
            Command::Help => {
                self.event_log.append(EventKind::CommandEntered, &raw);
                let help_text = Self::help_lines().join(" ");
                self.event_log.append(EventKind::HelpRequested, help_text);
                self.log.extend(Self::help_lines());
            }
            Command::Clear => {
                self.event_log.append(EventKind::CommandEntered, &raw);
                self.event_log
                    .append(EventKind::LogCleared, "Screen log cleared");
                self.log.clear();
            }
            Command::Quit => {
                self.event_log.append(EventKind::CommandEntered, &raw);
                self.event_log
                    .append(EventKind::QuitRequested, "Quit requested");
                self.should_quit = true;
            }
            Command::Text(t) => {
                self.event_log.append(EventKind::CommandEntered, &raw);
                self.event_log.append(EventKind::UserTextEntered, t.clone());
                self.log.push(t);
            }
            Command::Unknown(c) => {
                self.event_log.append(EventKind::CommandEntered, &raw);
                self.event_log.append(EventKind::UnknownCommand, c.clone());
                self.log.push(format!("Unknown command: {c}"));
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
            "  /help  - show this help".to_string(),
            "  /clear - clear the log".to_string(),
            "  /quit  - quit Caravan".to_string(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{EventKind, EventLog, EventSeq};

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
    fn plain_text_appends_command_entered_then_user_text_entered() {
        let mut app = App::new();
        app.input = "hello".to_string();
        app.submit();
        assert_eq!(app.event_log.len(), 3);
        let ce = app.event_log.get(1).unwrap();
        assert_eq!(ce.kind, EventKind::CommandEntered);
        assert_eq!(ce.detail, "hello");
        let ute = app.event_log.get(2).unwrap();
        assert_eq!(ute.kind, EventKind::UserTextEntered);
        assert_eq!(ute.detail, "hello");
        assert!(app.log.contains(&"hello".to_string()));
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
    fn quit_appends_command_entered_then_quit_requested() {
        let mut app = App::new();
        assert!(!app.should_quit);
        app.input = "/quit".to_string();
        app.submit();
        assert!(app.should_quit);
        assert_eq!(app.event_log.len(), 3);
        let ce = app.event_log.get(1).unwrap();
        assert_eq!(ce.kind, EventKind::CommandEntered);
        assert_eq!(ce.detail, "/quit");
        let qr = app.event_log.get(2).unwrap();
        assert_eq!(qr.kind, EventKind::QuitRequested);
        assert!(app.input.is_empty());
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
            "  /help  - show this help".to_string(),
            "  /clear - clear the log".to_string(),
            "  /quit  - quit Caravan".to_string(),
        ];
        assert_eq!(App::help_lines(), expected);
    }
}
