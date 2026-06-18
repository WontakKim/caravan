use std::collections::HashMap;

use super::*;

mod common;
use self::common::*;
mod context;
mod tools;
use kernel::events::{EventKind, EventLog, EventSeq};
use kernel::model_runtime_config::ModelRuntimeConfig;
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
fn plain_text_appends_user_text_then_run_turn() {
    let mut app = App::new();
    app.input = "hello".to_string();
    app.submit();

    let events = app.event_log.events();
    // First post-AppStart event is UserMessage with detail "hello"
    let first_after = events.get(1).expect("should have event after AppStart");
    assert_eq!(first_after.kind, EventKind::UserMessage);
    assert_eq!(first_after.detail, "hello");

    // NO SlashCommand event for plain text
    assert!(!events.iter().any(|e| e.kind == EventKind::SlashCommand));

    assert!(app.log.contains(&"User: hello".to_string()));
    assert!(
        app.log
            .contains(&"Assistant: Mock response for: hello".to_string())
    );

    assert!(app.input.is_empty());
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
fn plain_text_appends_full_run_turn_sequence() {
    let mut app = App::new();
    app.input = "hello".to_string();
    app.submit();

    let events = app.event_log.events();
    assert_eq!(events[0].kind, EventKind::AppStart);

    let after_app_started = &events[1..];
    let n = "Mock response for: hello".split_whitespace().count();
    let mut expected_kinds = vec![
        EventKind::UserMessage,
        EventKind::RunCreate,
        EventKind::RunStart,
        EventKind::TurnStart,
        EventKind::PromptCompile,
        EventKind::ModelRoute,
    ];
    for _ in 0..n {
        expected_kinds.push(EventKind::ModelOutputChunk);
    }
    expected_kinds.push(EventKind::AssistantMessage);
    expected_kinds.push(EventKind::RunComplete);

    assert_eq!(after_app_started.len(), expected_kinds.len());
    for (ev, expected) in after_app_started.iter().zip(expected_kinds.iter()) {
        assert_eq!(ev.kind, *expected);
    }

    assert!(app.log.contains(&"User: hello".to_string()));
    assert!(
        app.log
            .contains(&"Assistant: Mock response for: hello".to_string())
    );
}

#[test]
fn user_message_run_and_turn_ids_match_event_seqs() {
    let mut app = App::new();
    app.input = "hi".to_string();
    app.submit();

    let events = app.event_log.events();

    let run_created = events
        .iter()
        .find(|e| e.kind == EventKind::RunCreate)
        .expect("RunCreate event should exist");
    assert!(
        run_created
            .detail
            .contains(&format!("run_id=run-{}", run_created.seq)),
        "RunCreate detail should contain run_id=run-{{seq}}: {}",
        run_created.detail
    );

    let turn_started = events
        .iter()
        .find(|e| e.kind == EventKind::TurnStart)
        .expect("TurnStart event should exist");
    assert!(
        turn_started
            .detail
            .contains(&format!("turn_id=turn-{}", turn_started.seq)),
        "TurnStart detail should contain turn_id=turn-{{seq}}: {}",
        turn_started.detail
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

#[test]
fn prompt_compiled_detail_contains_template() {
    let mut app = App::new();
    app.input = "hello caravan".to_string();
    app.submit();

    let events = app.event_log.events();
    let pc = events
        .iter()
        .find(|e| e.kind == EventKind::PromptCompile)
        .expect("PromptCompile event should exist");

    assert!(pc.detail.contains("System:"));
    assert!(pc.detail.contains("User:"));
    assert!(pc.detail.contains("Context:"));
    assert!(pc.detail.contains("Output:"));
    assert!(pc.detail.contains("hello caravan"));
}

#[test]
fn openai_gateway_records_model_error_and_run_fail() {
    let dir = TempDir::new();
    let store = EventStore::new(dir.path());

    let vars = HashMap::from([("CARAVAN_MODEL_PROVIDER".to_string(), "openai".to_string())]);
    let runtime_config = ModelRuntimeConfig::from_env_map(&vars).unwrap();
    let gateway = ModelGateway::from_runtime_config(runtime_config);

    let mut app = App::with_store_and_gateway(store, gateway);
    app.input = "hello caravan".to_string();
    app.submit();

    let events = app.event_log.events();
    assert_eq!(events[0].kind, EventKind::AppStart);

    let after_app_start = &events[1..];
    let expected_kinds = [
        EventKind::UserMessage,
        EventKind::RunCreate,
        EventKind::RunStart,
        EventKind::TurnStart,
        EventKind::PromptCompile,
        EventKind::ModelError,
        EventKind::RunFail,
    ];
    assert_eq!(after_app_start.len(), expected_kinds.len());
    for (ev, expected) in after_app_start.iter().zip(expected_kinds.iter()) {
        assert_eq!(ev.kind, *expected);
    }

    // No ModelOutputChunk, RunComplete, or ModelRoute on the error path.
    assert!(!events.iter().any(|e| e.kind == EventKind::ModelOutputChunk));
    assert!(!events.iter().any(|e| e.kind == EventKind::RunComplete));
    assert!(!events.iter().any(|e| e.kind == EventKind::ModelRoute));

    // ModelError detail contains the expected skeleton message.
    let model_error_event = events
        .iter()
        .find(|e| e.kind == EventKind::ModelError)
        .expect("ModelError event should exist");
    assert!(
        model_error_event
            .detail
            .contains("OpenAI-compatible HTTP client is a skeleton in this POC"),
        "ModelError detail should contain expected message: {}",
        model_error_event.detail
    );

    // Screen log must contain the user message line.
    assert!(app.log.contains(&"User: hello caravan".to_string()));

    // No assistant line should be pushed on the error path.
    assert!(
        !app.log.iter().any(|l| l.starts_with("Assistant:")),
        "app.log should not contain any entry starting with 'Assistant:'"
    );
}

#[test]
fn blocking_gateway_missing_key_records_model_error_and_run_fail() {
    let dir = TempDir::new();
    let store = EventStore::new(dir.path());

    let vars = HashMap::from([
        ("CARAVAN_MODEL_PROVIDER".to_string(), "openai".to_string()),
        (
            "CARAVAN_OPENAI_HTTP_CLIENT".to_string(),
            "blocking".to_string(),
        ),
        (
            "CARAVAN_OPENAI_API_KEY_ENV".to_string(),
            "CARAVAN_TEST_MISSING_OPENAI_KEY_SHOULD_NOT_EXIST".to_string(),
        ),
    ]);
    let runtime_config = ModelRuntimeConfig::from_env_map(&vars).unwrap();
    let gateway = ModelGateway::from_runtime_config(runtime_config);

    let mut app = App::with_store_and_gateway(store, gateway);
    app.input = "hello caravan".to_string();
    app.submit();

    let events = app.event_log.events();
    assert_eq!(events[0].kind, EventKind::AppStart);

    let after_app_start = &events[1..];
    let expected_kinds = [
        EventKind::UserMessage,
        EventKind::RunCreate,
        EventKind::RunStart,
        EventKind::TurnStart,
        EventKind::PromptCompile,
        EventKind::ModelError,
        EventKind::RunFail,
    ];
    assert_eq!(after_app_start.len(), expected_kinds.len());
    for (ev, expected) in after_app_start.iter().zip(expected_kinds.iter()) {
        assert_eq!(ev.kind, *expected);
    }

    // No ModelOutputChunk or RunComplete on the missing-key error path.
    assert!(!events.iter().any(|e| e.kind == EventKind::ModelOutputChunk));
    assert!(!events.iter().any(|e| e.kind == EventKind::RunComplete));

    // ModelError detail contains the env var name but never a key value or Bearer header.
    let model_error_event = events
        .iter()
        .find(|e| e.kind == EventKind::ModelError)
        .expect("ModelError event should exist");
    assert!(
        model_error_event.detail.contains(
            "missing or empty API key env var: CARAVAN_TEST_MISSING_OPENAI_KEY_SHOULD_NOT_EXIST"
        ),
        "ModelError detail should contain expected message: {}",
        model_error_event.detail
    );
    assert!(
        !model_error_event.detail.contains("Bearer"),
        "ModelError detail must not contain Bearer: {}",
        model_error_event.detail
    );
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

#[test]
fn plain_text_produces_no_tool_events_and_correct_model_route() {
    let mut app = App::new();

    app.input = "hello caravan".to_string();
    app.submit();

    let events = app.event_log.events();

    assert!(
        !events.iter().any(|e| e.kind == EventKind::ToolPolicy),
        "plain text must not produce ToolPolicy events"
    );
    assert!(
        !events.iter().any(|e| e.kind == EventKind::ToolCall),
        "plain text must not produce ToolCall events"
    );
    assert!(
        !events.iter().any(|e| e.kind == EventKind::ToolResult),
        "plain text must not produce ToolResult events"
    );
    assert!(
        !events.iter().any(|e| e.kind == EventKind::ToolError),
        "plain text must not produce ToolError events"
    );

    let model_route = events
        .iter()
        .find(|e| e.kind == EventKind::ModelRoute)
        .expect("ModelRoute event should exist");
    assert_eq!(
        model_route.detail,
        "provider=mock model=mock-model adapter=MockModelAdapter"
    );
}

#[test]
fn hello_caravan_mock_flow_yields_expected_response_and_model_route() {
    let mut app = App::new();
    app.input = "hello caravan".to_string();
    app.submit();

    assert!(
        app.log
            .contains(&"Assistant: Mock response for: hello caravan".to_string()),
        "log should contain expected mock response"
    );

    let events = app.event_log.events();
    let model_route = events
        .iter()
        .find(|e| e.kind == EventKind::ModelRoute)
        .expect("ModelRoute event should exist");
    assert_eq!(
        model_route.detail,
        "provider=mock model=mock-model adapter=MockModelAdapter"
    );
}

// --- ModelToolRequest guidance tests ---

#[test]
fn model_tool_request_guidance_shown_in_log_and_invariants_hold() {
    // The first line of the message is plain text so the mock echo prefix
    // "Mock response for: " does not land on the same line as the
    // CARAVAN_TOOL_REQUEST delimiter, allowing the parser to detect it.
    let mut app = App::new();
    app.input =
        "read the readme\nCARAVAN_TOOL_REQUEST\ntool=read_file\npath=README.md\nEND_CARAVAN_TOOL_REQUEST"
            .to_string();
    app.submit();

    // (a) Positive case: guidance lines must appear in self.log.
    assert!(
        app.log.iter().any(|l| l == "Run: /tool read README.md"),
        "log must contain the exact line 'Run: /tool read README.md'"
    );
    assert!(
        app.log
            .iter()
            .any(|l| l.contains("/context attach-last-tool")),
        "log must contain a line with '/context attach-last-tool'"
    );
    assert!(
        app.log
            .iter()
            .any(|l| l.contains("did not execute it automatically")),
        "log must contain a line with 'did not execute it automatically'"
    );

    // (b) Invariants: exactly one ModelToolRequest event (round-trip guard),
    //     no tool-execution events, context fields untouched.
    let events = app.event_log.events();
    let model_tool_req_count = events
        .iter()
        .filter(|e| e.kind == EventKind::ModelToolRequest)
        .count();
    assert_eq!(
        model_tool_req_count, 1,
        "expected exactly one ModelToolRequest event (round-trip guard)"
    );
    assert!(
        !events.iter().any(|e| e.kind == EventKind::ToolCall),
        "must not emit ToolCall events"
    );
    assert!(
        !events.iter().any(|e| e.kind == EventKind::ToolResult),
        "must not emit ToolResult events"
    );
    assert!(
        !events.iter().any(|e| e.kind == EventKind::ToolError),
        "must not emit ToolError events"
    );
    assert!(
        app.last_tool_output_candidate.is_none(),
        "last_tool_output_candidate must remain None"
    );
    assert!(
        app.pending_manual_tool_context.is_none(),
        "pending_manual_tool_context must remain None"
    );
}

#[test]
fn plain_message_produces_no_model_tool_request_guidance() {
    let mut app = App::new();
    app.input = "hello caravan".to_string();
    app.submit();

    // (c) Negative case: no "Run: /tool" log lines and no ModelToolRequest event.
    assert!(
        !app.log.iter().any(|l| l.starts_with("Run: /tool")),
        "plain message must not produce any 'Run: /tool' log lines"
    );
    let events = app.event_log.events();
    assert!(
        !events.iter().any(|e| e.kind == EventKind::ModelToolRequest),
        "plain message must not produce a ModelToolRequest event"
    );
}

// --- pending_model_tool_request field tests ---

#[test]
fn model_tool_request_detected_sets_pending_and_keeps_invariants() {
    // The first line is plain text so the mock echo prefix does not land on
    // the same line as the CARAVAN_TOOL_REQUEST delimiter, allowing detection.
    let mut app = App::new();
    app.input =
        "read the readme\nCARAVAN_TOOL_REQUEST\ntool=read_file\npath=README.md\nEND_CARAVAN_TOOL_REQUEST"
            .to_string();
    app.submit();

    assert!(
        app.pending_model_tool_request.is_some(),
        "pending_model_tool_request should be Some after detection"
    );
    assert!(
        app.last_tool_output_candidate.is_none(),
        "last_tool_output_candidate must remain None"
    );
    assert!(
        app.pending_manual_tool_context.is_none(),
        "pending_manual_tool_context must remain None"
    );
}

#[test]
fn basic_mock_flow_keeps_pending_model_tool_request_none() {
    let mut app = App::new();
    app.input = "hello caravan".to_string();
    app.submit();

    assert!(
        app.pending_model_tool_request.is_none(),
        "pending_model_tool_request should remain None on plain mock response"
    );
}

#[test]
fn error_path_does_not_set_pending_model_tool_request() {
    let dir = TempDir::new();
    let store = EventStore::new(dir.path());

    let vars = HashMap::from([("CARAVAN_MODEL_PROVIDER".to_string(), "openai".to_string())]);
    let runtime_config = ModelRuntimeConfig::from_env_map(&vars).unwrap();
    let gateway = kernel::model_gateway::ModelGateway::from_runtime_config(runtime_config);

    let mut app = App::with_store_and_gateway(store, gateway);
    app.input = "hello".to_string();
    app.submit();

    assert!(
        app.pending_model_tool_request.is_none(),
        "error path must not set pending_model_tool_request"
    );
}

#[test]
fn none_detection_keeps_existing_pending() {
    use kernel::model_tool_request::{ModelToolRequest, ModelToolRequestKind};

    let mut app = App::new();
    let existing = ModelToolRequest {
        kind: ModelToolRequestKind::ReadFile,
        path: "README.md".to_string(),
    };
    app.pending_model_tool_request = Some(existing.clone());

    // Plain message — default mock returns no tool request block (detected = None).
    app.input = "hello caravan".to_string();
    app.submit();

    assert_eq!(
        app.pending_model_tool_request,
        Some(existing),
        "existing pending must remain unchanged when detection is None"
    );
}

#[test]
fn error_path_keeps_existing_pending() {
    use kernel::model_tool_request::{ModelToolRequest, ModelToolRequestKind};

    let dir = TempDir::new();
    let store = EventStore::new(dir.path());

    let vars = HashMap::from([("CARAVAN_MODEL_PROVIDER".to_string(), "openai".to_string())]);
    let runtime_config = ModelRuntimeConfig::from_env_map(&vars).unwrap();
    let gateway = kernel::model_gateway::ModelGateway::from_runtime_config(runtime_config);

    let mut app = App::with_store_and_gateway(store, gateway);
    let existing = ModelToolRequest {
        kind: ModelToolRequestKind::ReadFile,
        path: "README.md".to_string(),
    };
    app.pending_model_tool_request = Some(existing.clone());

    app.input = "hello".to_string();
    app.submit();

    assert_eq!(
        app.pending_model_tool_request,
        Some(existing),
        "error path must not clear existing pending_model_tool_request"
    );
}

#[test]
fn second_detection_replaces_pending() {
    use kernel::model_tool_request::{ModelToolRequest, ModelToolRequestKind};

    let mut app = App::new();
    // Pre-seed with a ListFiles request.
    app.pending_model_tool_request = Some(ModelToolRequest {
        kind: ModelToolRequestKind::ListFiles,
        path: ".".to_string(),
    });

    // Submit a message that triggers read_file README.md detection via the mock path.
    app.input =
        "read the readme\nCARAVAN_TOOL_REQUEST\ntool=read_file\npath=README.md\nEND_CARAVAN_TOOL_REQUEST"
            .to_string();
    app.submit();

    let expected = ModelToolRequest {
        kind: ModelToolRequestKind::ReadFile,
        path: "README.md".to_string(),
    };
    assert_eq!(
        app.pending_model_tool_request,
        Some(expected),
        "second detection must replace the existing pending_model_tool_request"
    );
}

// --- /request command tests ---

#[test]
fn request_status_without_pending_logs_none_and_only_slash_command() {
    let mut app = App::new();
    assert!(app.pending_model_tool_request.is_none());

    let log_len_before = app.log.len();
    let event_len_before = app.event_log.len();

    app.input = "/request status".to_string();
    app.submit();

    // Exactly two log lines added: header and "none" line.
    assert_eq!(app.log[log_len_before], "Model tool request status:");
    assert_eq!(app.log[log_len_before + 1], "- pending: none");
    assert_eq!(app.log.len(), log_len_before + 2);

    // Exactly one event appended and it is SlashCommand.
    assert_eq!(app.event_log.len(), event_len_before + 1);
    let new_event = app.event_log.get(event_len_before).unwrap();
    assert_eq!(new_event.kind, EventKind::SlashCommand);
    assert_eq!(new_event.detail, "/request status");

    // No RunCreate event anywhere.
    assert!(
        !app.event_log
            .events()
            .iter()
            .any(|e| e.kind == EventKind::RunCreate),
        "/request status must not start a model run"
    );
}

#[test]
fn request_status_with_pending_logs_suggested_command_and_next_step() {
    use kernel::model_tool_request::{ModelToolRequest, ModelToolRequestKind};

    let mut app = App::new();
    let req = ModelToolRequest {
        kind: ModelToolRequestKind::ReadFile,
        path: "README.md".to_string(),
    };
    app.pending_model_tool_request = Some(req.clone());

    let log_len_before = app.log.len();
    let event_len_before = app.event_log.len();

    app.input = "/request status".to_string();
    app.submit();

    // Exactly four log lines added.
    assert_eq!(app.log[log_len_before], "Model tool request status:");
    assert_eq!(
        app.log[log_len_before + 1],
        format!("- pending: {}", req.detail())
    );
    assert_eq!(
        app.log[log_len_before + 2],
        format!("- suggested command: {}", req.suggested_command())
    );
    assert_eq!(
        app.log[log_len_before + 3],
        "- next: run /context attach-last-tool after the tool succeeds"
    );
    assert_eq!(app.log.len(), log_len_before + 4);

    // Exactly one event appended and it is SlashCommand.
    assert_eq!(app.event_log.len(), event_len_before + 1);
    let new_event = app.event_log.get(event_len_before).unwrap();
    assert_eq!(new_event.kind, EventKind::SlashCommand);

    // Status must NOT clear the pending request.
    assert_eq!(
        app.pending_model_tool_request,
        Some(req),
        "/request status must not clear pending_model_tool_request"
    );

    // No RunCreate event.
    assert!(
        !app.event_log
            .events()
            .iter()
            .any(|e| e.kind == EventKind::RunCreate),
        "/request status must not start a model run"
    );
}

#[test]
fn request_clear_clears_pending_and_logs_message() {
    use kernel::model_tool_request::{ModelToolRequest, ModelToolRequestKind};

    let mut app = App::new();
    app.pending_model_tool_request = Some(ModelToolRequest {
        kind: ModelToolRequestKind::ReadFile,
        path: "README.md".to_string(),
    });

    let log_len_before = app.log.len();
    let event_len_before = app.event_log.len();

    app.input = "/request clear".to_string();
    app.submit();

    // pending_model_tool_request must be None after clear.
    assert!(
        app.pending_model_tool_request.is_none(),
        "pending_model_tool_request must be None after /request clear"
    );

    // Exactly one log line added.
    assert_eq!(
        app.log[log_len_before],
        "Cleared pending model tool request."
    );
    assert_eq!(app.log.len(), log_len_before + 1);

    // Exactly one event appended and it is SlashCommand.
    assert_eq!(app.event_log.len(), event_len_before + 1);
    let new_event = app.event_log.get(event_len_before).unwrap();
    assert_eq!(new_event.kind, EventKind::SlashCommand);

    // No RunCreate event.
    assert!(
        !app.event_log
            .events()
            .iter()
            .any(|e| e.kind == EventKind::RunCreate),
        "/request clear must not start a model run"
    );
}

#[test]
fn request_clear_without_pending_does_not_panic() {
    let mut app = App::new();
    assert!(app.pending_model_tool_request.is_none());

    let log_len_before = app.log.len();
    let event_len_before = app.event_log.len();

    // Must not panic.
    app.input = "/request clear".to_string();
    app.submit();

    assert!(app.pending_model_tool_request.is_none());

    // Exactly one log line added.
    assert_eq!(
        app.log[log_len_before],
        "Cleared pending model tool request."
    );
    assert_eq!(app.log.len(), log_len_before + 1);

    // Exactly one event appended and it is SlashCommand.
    assert_eq!(app.event_log.len(), event_len_before + 1);
    let new_event = app.event_log.get(event_len_before).unwrap();
    assert_eq!(new_event.kind, EventKind::SlashCommand);
}

// --- /request run tests ---

#[test]
fn request_run_without_pending() {
    let mut app = App::new();
    assert!(app.pending_model_tool_request.is_none());

    let event_len_before = app.event_log.len();

    app.input = "/request run".to_string();
    app.submit();

    let new_events = &app.event_log.events()[event_len_before..];

    // No ToolCall/ToolResult/ToolError events appended.
    assert!(
        !new_events.iter().any(|e| e.kind == EventKind::ToolCall),
        "must not emit ToolCall when no pending request"
    );
    assert!(
        !new_events.iter().any(|e| e.kind == EventKind::ToolResult),
        "must not emit ToolResult when no pending request"
    );
    assert!(
        !new_events.iter().any(|e| e.kind == EventKind::ToolError),
        "must not emit ToolError when no pending request"
    );
    // No model run events.
    assert!(
        !new_events.iter().any(|e| e.kind == EventKind::RunCreate),
        "must not start a model run"
    );
    assert!(
        !new_events.iter().any(|e| e.kind == EventKind::ModelRoute),
        "must not emit ModelRoute"
    );
    assert!(
        !new_events
            .iter()
            .any(|e| e.kind == EventKind::AssistantMessage),
        "must not emit AssistantMessage"
    );
    // "No pending model tool request." must appear in the log.
    assert!(
        app.log
            .iter()
            .any(|l| l == "No pending model tool request."),
        "log must contain 'No pending model tool request.'"
    );
    // pending_model_tool_request remains None (header: Request: none).
    assert!(app.pending_model_tool_request.is_none());
}

#[test]
fn request_run_pending_read_file_success() {
    use kernel::manual_context::ManualToolContext;
    use kernel::model_tool_request::{ModelToolRequest, ModelToolRequestKind};

    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();
    std::fs::write(workspace_dir.path().join("notes.txt"), "hello world").unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    app.pending_model_tool_request = Some(ModelToolRequest {
        kind: ModelToolRequestKind::ReadFile,
        path: "notes.txt".to_string(),
    });

    // Pre-seed a sentinel pending_manual_tool_context.
    let sentinel_ctx = ManualToolContext::from_read_file("sentinel.txt", "sentinel content");
    let sentinel_summary = sentinel_ctx.attach_summary();
    app.pending_manual_tool_context = Some(sentinel_ctx);

    let event_len_before = app.event_log.len();

    app.input = "/request run".to_string();
    app.submit();

    let new_events = &app.event_log.events()[event_len_before..];

    // Events in order: SlashCommand → ToolPolicy → ToolCall → ToolResult.
    assert!(
        new_events.len() >= 4,
        "expected at least 4 new events, got {}",
        new_events.len()
    );
    assert_eq!(new_events[0].kind, EventKind::SlashCommand);
    assert_eq!(new_events[1].kind, EventKind::ToolPolicy);
    assert_eq!(new_events[2].kind, EventKind::ToolCall);
    assert_eq!(new_events[3].kind, EventKind::ToolResult);

    // No model run events.
    assert!(!new_events.iter().any(|e| e.kind == EventKind::RunCreate));
    assert!(!new_events.iter().any(|e| e.kind == EventKind::ModelRoute));
    assert!(
        !new_events
            .iter()
            .any(|e| e.kind == EventKind::AssistantMessage)
    );

    // pending_model_tool_request cleared on success (header: Request: none).
    assert!(
        app.pending_model_tool_request.is_none(),
        "pending_model_tool_request must be None after successful /request run"
    );

    // last_tool_output_candidate updated.
    assert!(
        app.last_tool_output_candidate.is_some(),
        "last_tool_output_candidate must be Some after successful /request run"
    );

    // pending_manual_tool_context unchanged.
    assert_eq!(
        app.pending_manual_tool_context
            .as_ref()
            .map(|c| c.attach_summary()),
        Some(sentinel_summary),
        "pending_manual_tool_context must remain unchanged"
    );

    // Log contains read preview and attach-guidance line.
    assert!(
        app.log.iter().any(|l| l.contains("Tool read")),
        "log must contain read preview"
    );
    assert!(
        app.log.iter().any(|l| {
            l == "Run /context attach-last-tool to include this tool output in the next prompt."
        }),
        "log must contain attach-guidance line"
    );
}

#[test]
fn request_run_pending_list_files_success() {
    use kernel::model_tool_request::{ModelToolRequest, ModelToolRequestKind};

    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();
    std::fs::write(workspace_dir.path().join("file_a.txt"), "a").unwrap();
    std::fs::write(workspace_dir.path().join("file_b.txt"), "b").unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    app.pending_model_tool_request = Some(ModelToolRequest {
        kind: ModelToolRequestKind::ListFiles,
        path: ".".to_string(),
    });

    let event_len_before = app.event_log.len();

    app.input = "/request run".to_string();
    app.submit();

    let new_events = &app.event_log.events()[event_len_before..];

    // Events in order: SlashCommand → ToolPolicy → ToolCall → ToolResult.
    assert!(
        new_events.len() >= 4,
        "expected at least 4 new events, got {}",
        new_events.len()
    );
    assert_eq!(new_events[0].kind, EventKind::SlashCommand);
    assert_eq!(new_events[1].kind, EventKind::ToolPolicy);
    assert_eq!(new_events[2].kind, EventKind::ToolCall);
    assert_eq!(new_events[3].kind, EventKind::ToolResult);

    // No model run events.
    assert!(!new_events.iter().any(|e| e.kind == EventKind::RunCreate));
    assert!(!new_events.iter().any(|e| e.kind == EventKind::ModelRoute));
    assert!(
        !new_events
            .iter()
            .any(|e| e.kind == EventKind::AssistantMessage)
    );

    // pending cleared (header: Request: none).
    assert!(
        app.pending_model_tool_request.is_none(),
        "pending_model_tool_request must be None after successful /request run"
    );

    // last_tool_output_candidate updated.
    assert!(
        app.last_tool_output_candidate.is_some(),
        "last_tool_output_candidate must be Some after successful /request run"
    );

    // Log contains list preview and attach-guidance line.
    assert!(
        app.log.iter().any(|l| l.contains("Tool list")),
        "log must contain list preview"
    );
    assert!(
        app.log.iter().any(|l| {
            l == "Run /context attach-last-tool to include this tool output in the next prompt."
        }),
        "log must contain attach-guidance line"
    );
}

#[test]
fn request_run_pending_path_violation_tool_error() {
    use kernel::manual_context::ManualToolContext;
    use kernel::model_tool_request::{ModelToolRequest, ModelToolRequestKind};

    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    let escaping_req = ModelToolRequest {
        kind: ModelToolRequestKind::ReadFile,
        path: "../../etc/passwd".to_string(),
    };
    app.pending_model_tool_request = Some(escaping_req.clone());

    // Pre-seed sentinel values for both candidates.
    let sentinel_candidate = ManualToolContext::from_read_file("sentinel.txt", "sentinel content");
    let sentinel_candidate_summary = sentinel_candidate.attach_summary();
    app.last_tool_output_candidate = Some(sentinel_candidate);

    let sentinel_ctx = ManualToolContext::from_read_file("ctx_sentinel.txt", "ctx content");
    let sentinel_ctx_summary = sentinel_ctx.attach_summary();
    app.pending_manual_tool_context = Some(sentinel_ctx);

    let event_len_before = app.event_log.len();

    app.input = "/request run".to_string();
    app.submit();

    let new_events = &app.event_log.events()[event_len_before..];

    // Events in order: SlashCommand → ToolPolicy → ToolCall → ToolError.
    assert!(
        new_events.len() >= 4,
        "expected at least 4 new events, got {}",
        new_events.len()
    );
    assert_eq!(new_events[0].kind, EventKind::SlashCommand);
    assert_eq!(new_events[1].kind, EventKind::ToolPolicy);
    assert_eq!(new_events[2].kind, EventKind::ToolCall);
    assert_eq!(new_events[3].kind, EventKind::ToolError);

    // No model run events.
    assert!(!new_events.iter().any(|e| e.kind == EventKind::RunCreate));
    assert!(!new_events.iter().any(|e| e.kind == EventKind::ModelRoute));
    assert!(
        !new_events
            .iter()
            .any(|e| e.kind == EventKind::AssistantMessage)
    );

    // pending_model_tool_request kept (header: Request: pending).
    assert!(
        app.pending_model_tool_request.is_some(),
        "pending_model_tool_request must remain Some after tool error"
    );

    // Both sentinels unchanged.
    assert_eq!(
        app.last_tool_output_candidate
            .as_ref()
            .map(|c| c.attach_summary()),
        Some(sentinel_candidate_summary),
        "last_tool_output_candidate must be unchanged on failure"
    );
    assert_eq!(
        app.pending_manual_tool_context
            .as_ref()
            .map(|c| c.attach_summary()),
        Some(sentinel_ctx_summary),
        "pending_manual_tool_context must be unchanged on failure"
    );

    // Log contains a tool error message.
    assert!(
        app.log.iter().any(|l| l.contains("Tool error:")),
        "log must contain a tool error message"
    );
}

// --- New /request run event-sequence tests (Step 9) ---

#[test]
fn request_run_pending_success_appends_slash_policy_call_result() {
    use kernel::model_tool_request::{ModelToolRequest, ModelToolRequestKind};

    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();
    std::fs::write(workspace_dir.path().join("notes.txt"), "hello world").unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    app.pending_model_tool_request = Some(ModelToolRequest {
        kind: ModelToolRequestKind::ReadFile,
        path: "notes.txt".to_string(),
    });

    let event_len_before = app.event_log.len();
    app.input = "/request run".to_string();
    app.submit();

    let new_events = &app.event_log.events()[event_len_before..];

    assert_eq!(new_events.len(), 4);
    assert_eq!(new_events[0].kind, EventKind::SlashCommand);
    assert_eq!(new_events[1].kind, EventKind::ToolPolicy);
    assert_eq!(new_events[2].kind, EventKind::ToolCall);
    assert_eq!(new_events[3].kind, EventKind::ToolResult);
}

#[test]
fn request_run_pending_failure_appends_slash_policy_call_error() {
    use kernel::model_tool_request::{ModelToolRequest, ModelToolRequestKind};

    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    app.pending_model_tool_request = Some(ModelToolRequest {
        kind: ModelToolRequestKind::ReadFile,
        path: "../secret.txt".to_string(),
    });

    let event_len_before = app.event_log.len();
    app.input = "/request run".to_string();
    app.submit();

    let new_events = &app.event_log.events()[event_len_before..];

    assert_eq!(new_events.len(), 4);
    assert_eq!(new_events[0].kind, EventKind::SlashCommand);
    assert_eq!(new_events[1].kind, EventKind::ToolPolicy);
    assert_eq!(new_events[2].kind, EventKind::ToolCall);
    assert_eq!(new_events[3].kind, EventKind::ToolError);
}

// --- Negative-path ToolPolicy tests (Step 11) ---

#[test]
fn invalid_tool_subcommand_produces_no_tool_policy_event() {
    let mut app = App::new();
    app.input = "/tool bogus".to_string();
    app.submit();

    let events = app.event_log.events();
    assert!(
        !events.iter().any(|e| e.kind == EventKind::ToolPolicy),
        "invalid /tool command must not produce ToolPolicy events"
    );
}

#[test]
fn request_run_without_pending_produces_no_tool_policy_event() {
    let mut app = App::new();
    assert!(app.pending_model_tool_request.is_none());

    app.input = "/request run".to_string();
    app.submit();

    let events = app.event_log.events();
    assert!(
        !events.iter().any(|e| e.kind == EventKind::ToolPolicy),
        "/request run with no pending request must not produce ToolPolicy events"
    );
}

#[test]
fn model_tool_request_detection_only_produces_no_tool_policy_event() {
    let mut app = App::new();
    app.input =
        "read the readme\nCARAVAN_TOOL_REQUEST\ntool=read_file\npath=README.md\nEND_CARAVAN_TOOL_REQUEST"
            .to_string();
    app.submit();

    let events = app.event_log.events();
    assert!(
        !events.iter().any(|e| e.kind == EventKind::ToolPolicy),
        "ModelToolRequest detection without execution must not produce ToolPolicy events"
    );
}

// --- PolicyDenied arm test (T-5) ---

#[test]
fn push_tool_error_output_policy_denied_formats_message() {
    let mut app = App::new();
    app.push_tool_error_output(kernel::ToolError::PolicyDenied {
        reason: "test_reason".to_string(),
    });
    let last = app.log.last().unwrap();
    assert!(
        last.contains("policy denied"),
        "log must contain 'policy denied': {last}"
    );
    assert!(
        last.contains("test_reason"),
        "log must contain 'test_reason': {last}"
    );
}
