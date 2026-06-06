use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

pub fn handle_key(app: &mut crate::app::App, key: KeyEvent) {
    if key.kind != KeyEventKind::Press {
        return;
    }

    // Ctrl+C quits the application even in raw mode.
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.should_quit = true;
        return;
    }

    match key.code {
        KeyCode::Char(c) => app.push_char(c),
        KeyCode::Backspace => app.backspace(),
        KeyCode::Enter => app.submit(),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventKind, KeyModifiers};

    use crate::app::App;

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new_with_kind(code, KeyModifiers::NONE, KeyEventKind::Press)
    }

    fn release(code: KeyCode) -> KeyEvent {
        KeyEvent::new_with_kind(code, KeyModifiers::NONE, KeyEventKind::Release)
    }

    #[test]
    fn char_events_accumulate_in_input() {
        let mut app = App::new();
        handle_key(&mut app, press(KeyCode::Char('h')));
        handle_key(&mut app, press(KeyCode::Char('i')));
        assert_eq!(app.input, "hi");
    }

    #[test]
    fn backspace_removes_last_char() {
        let mut app = App::new();
        handle_key(&mut app, press(KeyCode::Char('a')));
        handle_key(&mut app, press(KeyCode::Char('b')));
        handle_key(&mut app, press(KeyCode::Backspace));
        assert_eq!(app.input, "a");
    }

    #[test]
    fn enter_routes_through_submit() {
        let mut app = App::new();
        for c in "/clear".chars() {
            handle_key(&mut app, press(KeyCode::Char(c)));
        }
        handle_key(&mut app, press(KeyCode::Enter));
        assert!(app.log.is_empty());
        assert!(app.input.is_empty());
    }

    #[test]
    fn release_event_does_not_modify_input() {
        let mut app = App::new();
        handle_key(&mut app, release(KeyCode::Char('x')));
        assert_eq!(app.input, "");
    }

    #[test]
    fn ctrl_c_sets_should_quit() {
        let mut app = App::new();
        let ctrl_c = KeyEvent::new_with_kind(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
            KeyEventKind::Press,
        );
        handle_key(&mut app, ctrl_c);
        assert!(app.should_quit);
    }
}
