use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

pub fn handle_key(app: &mut crate::app::App, key: KeyEvent) {
    if key.kind != KeyEventKind::Press {
        return;
    }

    // Ctrl+C exits the application even in raw mode, recording an ExitRequest event.
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.exit_from_ctrl_c();
        return;
    }

    match key.code {
        KeyCode::Up => app.select_prev(),
        KeyCode::Down => app.select_next(),
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
    fn ctrl_c_sets_should_exit() {
        let mut app = App::new();
        let ctrl_c = KeyEvent::new_with_kind(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
            KeyEventKind::Press,
        );
        handle_key(&mut app, ctrl_c);
        assert!(app.should_exit);
        // Ctrl+C records an ExitRequest event (matches the README).
        let last = app.event_log.get(app.event_log.len() - 1).unwrap();
        assert_eq!(last.kind, crate::events::EventKind::ExitRequest);
    }

    #[test]
    fn down_from_fresh_app_selects_index_zero_without_appending() {
        let mut app = App::new();
        handle_key(&mut app, press(KeyCode::Down));
        assert_eq!(app.selected_event, Some(0));
        // Navigation is pure UI state and must not append events.
        assert_eq!(app.event_log.len(), 1);
    }

    #[test]
    fn up_from_fresh_app_selects_index_zero_without_appending() {
        let mut app = App::new();
        handle_key(&mut app, press(KeyCode::Up));
        assert_eq!(app.selected_event, Some(0));
        // Navigation is pure UI state and must not append events.
        assert_eq!(app.event_log.len(), 1);
    }

    #[test]
    fn down_twice_advances_selection_without_appending() {
        let mut app = App::new();
        // Seed a second event so there is something to advance to.
        app.event_log
            .append(crate::events::EventKind::UserMessage, "hello".to_string());
        handle_key(&mut app, press(KeyCode::Down));
        handle_key(&mut app, press(KeyCode::Down));
        assert_eq!(app.selected_event, Some(1));
        // Navigation is pure UI state and must not append events.
        assert_eq!(app.event_log.len(), 2);
    }

    #[test]
    fn up_from_some_zero_is_noop() {
        let mut app = App::new();
        // Navigate to Some(0) first
        handle_key(&mut app, press(KeyCode::Down));
        assert_eq!(app.selected_event, Some(0));
        let len_before = app.event_log.len();
        // Up from Some(0) is a no-op (lower-boundary clamp)
        handle_key(&mut app, press(KeyCode::Up));
        assert_eq!(app.selected_event, Some(0));
        assert_eq!(app.event_log.len(), len_before);
    }
}
