use super::super::*;
use super::common::*;
use std::collections::HashMap;

use kernel::events::{EventKind, EventLog, EventSeq};
use kernel::model_runtime_config::ModelRuntimeConfig;
use kernel::storage::EventStore;

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
