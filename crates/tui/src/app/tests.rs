use super::*;

mod common;
use self::common::*;
mod context;
mod lifecycle;
mod model_flow;
mod policy;
mod request;
mod selection;
mod storage;
mod tools;
use kernel::events::EventKind;

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
fn help_lines_exact_content() {
    let expected = vec![
        "Available commands:".to_string(),
        "  Type a message (no leading /) to send it as a user message".to_string(),
        "  /help  - show this help".to_string(),
        "  /clear - clear the log".to_string(),
        "  /exit  - exit Caravan".to_string(),
        "  /tool list [path] - list files under the workspace".to_string(),
        "  /tool read <path> - read a UTF-8 text file under the workspace".to_string(),
        "  /context attach-last-tool - attach the latest read-only tool output to the next prompt"
            .to_string(),
        "  /context clear - clear pending manual tool context".to_string(),
        "  /context status - show pending manual tool context and last tool output".to_string(),
        "  /request status - show the pending model tool request".to_string(),
        "  /request clear - clear the pending model tool request".to_string(),
        "  /request run - execute the pending model tool request (read-only)".to_string(),
    ];
    assert_eq!(App::help_lines(), expected);
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
