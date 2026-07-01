use super::super::*;
use super::common::*;
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
        "  Type @path or @path:line-line in a message to attach workspace context".to_string(),
        "  Claude-like core commands:".to_string(),
        "    /help - show this help".to_string(),
        "    /clear - clear the log".to_string(),
        "    /exit - exit Caravan".to_string(),
        "    /reset - reset the session (clears screen log and pending state)".to_string(),
        "    /new - start a new session (alias for /reset)".to_string(),
        "    /quit - quit Caravan (alias for /exit)".to_string(),
        "    /permissions - show the current permission posture".to_string(),
        "    /allowed-tools - list the tools that are currently allowed".to_string(),
        "  Basic workspace tools:".to_string(),
        "    /tool list [path] - list files under the workspace".to_string(),
        "    /tool read <path> [--offset <line>] [--limit <lines>] - read a UTF-8 text file under the workspace".to_string(),
        "    /tool search <query> - search for a string across workspace files".to_string(),
        "    /tool glob <pattern> - find files matching a glob pattern in the workspace"
            .to_string(),
    ];
    assert_eq!(App::help_lines(), expected);
}

#[test]
fn help_lines_excludes_harness_commands() {
    let lines = App::help_lines();

    // Experimental harness commands must not appear in the default /help surface.
    for pattern in &[
        "/context",
        "/request",
        "/approval",
        "/tool plan-write",
        "/tool preview-write",
        "/tool propose-write",
    ] {
        assert!(
            !lines.iter().any(|l| l.contains(pattern)),
            "help_lines should not contain {}",
            pattern
        );
    }

    // Basic workspace and core commands must appear.
    assert!(
        lines.iter().any(|l| l.contains("/tool list [path]")),
        "help_lines should contain /tool list [path]"
    );
    assert!(
        lines.iter().any(|l| l.contains("/tool read <path>")),
        "help_lines should contain /tool read <path>"
    );
    assert!(
        lines.iter().any(|l| l.contains("/permissions")),
        "help_lines should contain /permissions"
    );
    assert!(
        lines.iter().any(|l| l.contains("/allowed-tools")),
        "help_lines should contain /allowed-tools"
    );
}

#[test]
fn help_lines_excludes_unsupported_commands() {
    let lines = App::help_lines();
    // /quit is now an implemented exit alias and intentionally appears in help output.
    assert!(
        !lines.iter().any(|l| l.contains("/ask")),
        "help_lines should not reference /ask"
    );
    assert!(
        !lines.iter().any(|l| l.contains("/model")),
        "help_lines should not reference /model"
    );
    assert!(
        !lines.iter().any(|l| l.contains("/plan")),
        "help_lines should not reference /plan"
    );
    assert!(
        !lines.iter().any(|l| l.contains("/diff")),
        "help_lines should not reference /diff"
    );
}

#[test]
fn help_lines_mentions_workspace_reference_syntax() {
    let lines = App::help_lines();

    let hint = lines
        .iter()
        .find(|l| l.contains("@path"))
        .expect("help_lines should contain a line mentioning @path");
    assert!(
        hint.contains(":line"),
        "the @path hint should mention the :line-line syntax, got: {}",
        hint
    );
    assert!(
        !hint.contains('/'),
        "the @path hint should not introduce a /-command, got: {}",
        hint
    );
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
