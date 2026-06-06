pub struct App {
    pub log: Vec<String>,
    pub input: String,
    pub should_quit: bool,
}

impl App {
    pub fn new() -> Self {
        Self {
            log: vec!["Caravan started.".to_string()],
            input: String::new(),
            should_quit: false,
        }
    }

    pub fn push_char(&mut self, c: char) {
        self.input.push(c);
    }

    pub fn backspace(&mut self) {
        self.input.pop();
    }

    pub fn submit(&mut self) {
        if self.input.is_empty() {
            return;
        }

        if self.input.starts_with('/') {
            match self.input.as_str() {
                "/help" => {
                    let lines = Self::help_lines();
                    self.log.extend(lines);
                }
                "/clear" => {
                    self.log.clear();
                }
                "/quit" => {
                    self.should_quit = true;
                }
                _ => {
                    let msg = format!("Unknown command: {}", self.input);
                    self.log.push(msg);
                }
            }
        } else {
            self.log.push(self.input.clone());
        }

        self.input.clear();
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

    #[test]
    fn new_seeds_startup_message() {
        let app = App::new();
        assert_eq!(app.log, vec!["Caravan started."]);
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
    fn clear_empties_log() {
        let mut app = App::new();
        app.input = "/clear".to_string();
        app.submit();
        assert!(app.log.is_empty());
        assert!(app.input.is_empty());
    }

    #[test]
    fn unknown_command_appends_error_line() {
        let mut app = App::new();
        app.input = "/unknown".to_string();
        app.submit();
        assert!(app.log.iter().any(|l| l.contains("Unknown command:")));
        assert!(app.input.is_empty());
    }

    #[test]
    fn plain_text_is_echoed_to_log() {
        let mut app = App::new();
        app.input = "hello world".to_string();
        app.submit();
        assert!(app.log.contains(&"hello world".to_string()));
        assert!(app.input.is_empty());
    }

    #[test]
    fn empty_submit_is_noop() {
        let mut app = App::new();
        let log_before = app.log.clone();
        app.submit();
        assert_eq!(app.log, log_before);
        assert!(app.input.is_empty());
    }

    #[test]
    fn quit_sets_should_quit() {
        let mut app = App::new();
        assert!(!app.should_quit);
        app.input = "/quit".to_string();
        app.submit();
        assert!(app.should_quit);
        assert!(app.input.is_empty());
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

    #[test]
    fn help_command_appends_exact_lines_to_log() {
        let mut app = App::new();
        app.input = "/help".to_string();
        app.submit();
        let expected = App::help_lines();
        for line in &expected {
            assert!(app.log.contains(line), "log missing line: {}", line);
        }
        // verify order: find the index of "Available commands:" and check subsequent lines
        let start = app
            .log
            .iter()
            .position(|l| l == "Available commands:")
            .expect("Available commands: not found");
        assert_eq!(app.log[start..start + 4], expected[..]);
    }
}
