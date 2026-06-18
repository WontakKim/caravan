use super::super::*;
use super::common::*;
use kernel::events::{EventKind, EventSeq};
use kernel::storage::EventStore;

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
