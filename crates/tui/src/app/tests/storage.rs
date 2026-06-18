use super::super::*;
use super::common::*;
use kernel::events::EventKind;
use kernel::storage::EventStore;

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
