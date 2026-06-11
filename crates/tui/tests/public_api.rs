use kernel::EventKind;
use tui::App;

#[test]
fn smoke_test_tui_public_api() {
    let app = App::new();

    assert_eq!(app.event_log.len(), 1);

    let first_event = app.event_log.get(0).expect("event at index 0 should exist");
    assert!(
        matches!(first_event.kind, EventKind::AppStart),
        "first event should be AppStart"
    );

    assert!(!app.should_exit);
}
