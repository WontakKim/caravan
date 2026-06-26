use super::super::*;
use super::common::*;
use std::collections::HashMap;

use kernel::events::{EventKind, EventLog, EventSeq};
use kernel::manual_context::ManualToolContext;
use kernel::model_runtime_config::ModelRuntimeConfig;
use kernel::model_tool_request::{ModelToolRequest, ModelToolRequestKind};
use kernel::runner::{MockRunOutput, ModelToolActivity};
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

// ============================================================================
// Native tool activity screen-log + state-isolation tests
//
// These tests drive App::push_run_output_to_log (the private helper that
// submit() delegates to after run_mock_turn) directly with synthetic
// MockRunOutput values.  This avoids the need to inject a fake HTTP client
// into the kernel gateway while still exercising the exact code path that
// submit() runs.
// ============================================================================

// Helper: build a synthetic MockRunOutput for a succeeded native read_file turn.
// The assistant_response is always a short summary — the raw file body is never
// placed in the output fields, which is what "no full tool output in the log" tests.
fn make_succeeded_output(path: &str) -> MockRunOutput {
    MockRunOutput {
        user_message: format!("read {path}"),
        assistant_response: format!("I read {path}."),
        run_id: "run-test".to_string(),
        turn_id: "turn-test".to_string(),
        detected_model_tool_request: None,
        tool_activities: vec![ModelToolActivity {
            name: "read_file".to_string(),
            path: path.to_string(),
            succeeded: true,
        }],
    }
}

// Helper: build a synthetic MockRunOutput for a failed native read_file turn.
fn make_failed_output(path: &str) -> MockRunOutput {
    MockRunOutput {
        user_message: format!("read {path}"),
        assistant_response: format!("Could not read {path}."),
        run_id: "run-test".to_string(),
        turn_id: "turn-test".to_string(),
        detected_model_tool_request: None,
        tool_activities: vec![ModelToolActivity {
            name: "read_file".to_string(),
            path: path.to_string(),
            succeeded: false,
        }],
    }
}

// --- Test (a): screen-log contains Tool:/Tool completed: before Assistant: ---

#[test]
fn native_tool_screen_log_has_tool_and_tool_completed_before_assistant() {
    let mut app = App::new();
    // The raw file body is never placed in the MockRunOutput fields — the
    // helper only logs concise activity lines, never raw tool output.
    let file_body = "the quick brown fox — must not appear in the log";
    let output = make_succeeded_output("fox.txt");

    // Drive the helper that submit() uses internally.
    app.push_run_output_to_log(&output);

    let tool_line = "Tool: read_file fox.txt".to_string();
    let completed_line = "Tool completed: read_file fox.txt".to_string();

    assert!(
        app.log.contains(&tool_line),
        "log must contain 'Tool: read_file fox.txt'; log={:?}",
        app.log
    );
    assert!(
        app.log.contains(&completed_line),
        "log must contain 'Tool completed: read_file fox.txt'; log={:?}",
        app.log
    );
    assert!(
        app.log.iter().any(|l| l.starts_with("Assistant:")),
        "log must contain an 'Assistant:' line"
    );

    // Order: Tool: < Tool completed: < Assistant:
    let tool_idx = app.log.iter().position(|l| l == &tool_line).unwrap();
    let completed_idx = app.log.iter().position(|l| l == &completed_line).unwrap();
    let assistant_idx = app
        .log
        .iter()
        .position(|l| l.starts_with("Assistant:"))
        .unwrap();
    assert!(
        tool_idx < completed_idx,
        "Tool: must precede Tool completed:"
    );
    assert!(
        completed_idx < assistant_idx,
        "Tool completed: must precede Assistant:"
    );

    // The raw file body must not appear anywhere in the screen log — the
    // helper only logs summary lines, never raw content.
    assert!(
        !app.log.iter().any(|l| l.contains(file_body)),
        "screen log must not contain raw file body"
    );
}

// --- Test (b): failing tool turn logs "Tool failed: ..." ---------------------

#[test]
fn native_tool_screen_log_has_tool_failed_when_tool_errors() {
    let mut app = App::new();
    let output = make_failed_output("missing_sentinel.txt");

    app.push_run_output_to_log(&output);

    assert!(
        app.log
            .contains(&"Tool: read_file missing_sentinel.txt".to_string()),
        "log must contain 'Tool: read_file missing_sentinel.txt'; log={:?}",
        app.log
    );
    assert!(
        app.log
            .contains(&"Tool failed: read_file missing_sentinel.txt".to_string()),
        "log must contain 'Tool failed: read_file missing_sentinel.txt'; log={:?}",
        app.log
    );
    assert!(
        !app.log
            .iter()
            .any(|l| l.contains("Tool completed: read_file")),
        "log must NOT contain 'Tool completed:' when tool failed"
    );
}

// --- Test (c): sentinel state isolation --------------------------------------
// last_tool_output_candidate and pending_model_tool_request must NOT be
// touched by a native tool turn (neither cleared nor overwritten).

#[test]
fn native_tool_output_does_not_modify_sentinel_manual_state_fields() {
    let mut app = App::new();

    // Plant non-None sentinels in the manual-flow state fields.
    let sentinel_mtc = ManualToolContext::from_read_file("sentinel-manual.txt", "sentinel content");
    let sentinel_mtr = ModelToolRequest {
        kind: ModelToolRequestKind::ReadFile,
        path: "sentinel-model-tool.txt".to_string(),
    };
    app.last_tool_output_candidate = Some(sentinel_mtc.clone());
    app.pending_model_tool_request = Some(sentinel_mtr.clone());

    // Run the code path that processes native tool activity.
    let output = make_succeeded_output("iso.txt");
    app.push_run_output_to_log(&output);

    // Both sentinel fields must be unchanged after processing native tool output.
    let ltoc = app
        .last_tool_output_candidate
        .as_ref()
        .expect("last_tool_output_candidate must still be Some after native tool output");
    assert_eq!(
        ltoc.source, sentinel_mtc.source,
        "source must match sentinel"
    );
    assert_eq!(
        ltoc.content, sentinel_mtc.content,
        "content must match sentinel"
    );
    assert_eq!(
        ltoc.truncated, sentinel_mtc.truncated,
        "truncated must match sentinel"
    );

    assert_eq!(
        app.pending_model_tool_request,
        Some(sentinel_mtr),
        "pending_model_tool_request must still equal the sentinel after native tool output"
    );
}

// --- Test (d): pending_manual_tool_context one-shot .take() ------------------
// If pending_manual_tool_context is set before a turn it is consumed by
// submit() via .take() and is NOT replaced by the native tool result.

#[test]
fn submit_consumes_pending_manual_tool_context_via_take_and_does_not_replace_it() {
    let mut app = App::new();

    // Set a pending manual context before the turn.
    let pre_context =
        ManualToolContext::from_read_file("pre-manual.txt", "pre-manual content for context");
    app.pending_manual_tool_context = Some(pre_context);

    // Run a plain user-message turn through submit() (Mock gateway: no native tool).
    app.input = "hello".to_string();
    app.submit();

    // The one-shot .take() must have consumed the context; nothing replaced it.
    assert!(
        app.pending_manual_tool_context.is_none(),
        "pending_manual_tool_context must be None after submit() consumed it"
    );
}

// --- Test (e): attach-last-tool after native turn uses the sentinel -----------
// /context attach-last-tool must attach the most recent MANUAL /tool output
// (last_tool_output_candidate), not the native tool result.

#[test]
fn attach_last_tool_after_native_output_attaches_manual_sentinel_not_native_result() {
    let mut app = App::new();

    // Plant the sentinel as the most-recent manual tool output candidate.
    let sentinel =
        ManualToolContext::from_read_file("manual-sentinel.txt", "manual sentinel content");
    app.last_tool_output_candidate = Some(sentinel.clone());

    // Process native tool output — this must NOT overwrite last_tool_output_candidate.
    let output = make_succeeded_output("native.txt");
    app.push_run_output_to_log(&output);

    // The sentinel must still be in last_tool_output_candidate.
    let candidate_after = app
        .last_tool_output_candidate
        .as_ref()
        .expect("last_tool_output_candidate must still be Some after native tool output");
    assert_eq!(
        candidate_after.source, sentinel.source,
        "last_tool_output_candidate source must equal sentinel"
    );

    // /context attach-last-tool must attach the sentinel, not the native result.
    app.input = "/context attach-last-tool".to_string();
    app.submit();

    let attached = app
        .pending_manual_tool_context
        .as_ref()
        .expect("pending_manual_tool_context must be Some after attach-last-tool");
    assert_eq!(
        attached.source, sentinel.source,
        "attached context source must equal the manual sentinel, not native result"
    );
    assert_eq!(
        attached.content, sentinel.content,
        "attached context content must equal the manual sentinel"
    );
}

// --- Test (f): two_activities in vec order before Assistant: -----------------
// Verifies that push_run_output_to_log emits both activity blocks (Tool: +
// Tool completed:) in vec order, and that all activity lines precede the
// Assistant: line.

#[test]
fn two_activities_in_tool_activities_both_logged_in_vec_order_before_assistant() {
    let mut app = App::new();

    let a1 = ModelToolActivity {
        name: "read_file".to_string(),
        path: "first.txt".to_string(),
        succeeded: true,
    };
    let a2 = ModelToolActivity {
        name: "list_files".to_string(),
        path: "src/".to_string(),
        succeeded: false,
    };

    let output = MockRunOutput {
        user_message: "two tool turn".to_string(),
        assistant_response: "Done with two tools.".to_string(),
        run_id: "run-two".to_string(),
        turn_id: "turn-two".to_string(),
        detected_model_tool_request: None,
        tool_activities: vec![a1, a2],
    };

    app.push_run_output_to_log(&output);

    // Both first-activity lines must be present.
    let tool1_line = "Tool: read_file first.txt".to_string();
    let completed1_line = "Tool completed: read_file first.txt".to_string();
    // Both second-activity lines must be present.
    let tool2_line = "Tool: list_files src/".to_string();
    let failed2_line = "Tool failed: list_files src/".to_string();

    assert!(
        app.log.contains(&tool1_line),
        "log must contain '{tool1_line}'; log={:?}",
        app.log
    );
    assert!(
        app.log.contains(&completed1_line),
        "log must contain '{completed1_line}'; log={:?}",
        app.log
    );
    assert!(
        app.log.contains(&tool2_line),
        "log must contain '{tool2_line}'; log={:?}",
        app.log
    );
    assert!(
        app.log.contains(&failed2_line),
        "log must contain '{failed2_line}'; log={:?}",
        app.log
    );

    let idx = |needle: &str| app.log.iter().position(|l| l == needle).unwrap();
    let tool1_idx = idx(&tool1_line);
    let completed1_idx = idx(&completed1_line);
    let tool2_idx = idx(&tool2_line);
    let failed2_idx = idx(&failed2_line);
    let assistant_idx = app
        .log
        .iter()
        .position(|l| l.starts_with("Assistant:"))
        .unwrap();

    // Vec order: first activity block fully precedes second activity block.
    assert!(
        tool1_idx < completed1_idx,
        "Tool: must precede Tool completed: for first activity"
    );
    assert!(
        completed1_idx < tool2_idx,
        "first activity block must precede second activity block"
    );
    assert!(
        tool2_idx < failed2_idx,
        "Tool: must precede Tool failed: for second activity"
    );

    // All activity lines must precede the Assistant: line.
    assert!(
        failed2_idx < assistant_idx,
        "all activity lines must precede Assistant:"
    );
}

// --- Test (g): two_activities do not set last_tool_output_candidate ----------
// Verifies the state-isolation invariant: push_run_output_to_log MUST NOT
// touch last_tool_output_candidate or pending_manual_tool_context even when
// tool_activities is non-empty.

#[test]
fn two_activities_rendering_does_not_set_last_tool_output_candidate_or_pending_context() {
    let mut app = App::new();

    // Confirm both fields start as None.
    assert!(app.last_tool_output_candidate.is_none());
    assert!(app.pending_manual_tool_context.is_none());

    let output = MockRunOutput {
        user_message: "iso two".to_string(),
        assistant_response: "iso response".to_string(),
        run_id: "run-iso".to_string(),
        turn_id: "turn-iso".to_string(),
        detected_model_tool_request: None,
        tool_activities: vec![
            ModelToolActivity {
                name: "read_file".to_string(),
                path: "iso-a.txt".to_string(),
                succeeded: true,
            },
            ModelToolActivity {
                name: "list_files".to_string(),
                path: ".".to_string(),
                succeeded: true,
            },
        ],
    };

    app.push_run_output_to_log(&output);

    // Both fields must remain None after processing native tool activities.
    assert!(
        app.last_tool_output_candidate.is_none(),
        "last_tool_output_candidate must remain None after push_run_output_to_log with two activities"
    );
    assert!(
        app.pending_manual_tool_context.is_none(),
        "pending_manual_tool_context must remain None after push_run_output_to_log with two activities"
    );
}
