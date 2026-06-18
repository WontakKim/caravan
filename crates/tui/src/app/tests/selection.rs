use super::super::*;
use super::common::*;
use kernel::events::EventLog;

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
