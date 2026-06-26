use super::*;
use crate::events::{EventKind, EventLog};
use crate::model::ModelError;
use crate::model::openai::http::{OpenAIHttpClient, OpenAIHttpError, OpenAIHttpResult};
use crate::model::openai::request::OpenAIRequestPlan;
use crate::model::openai::types::{
    OpenAIChatChoice, OpenAIChatMessage, OpenAIChatResponse, OpenAIToolCall,
    OpenAIToolCallFunction, OpenAIUsage,
};
use crate::model_config::{ModelConfig, ModelProfile};
use crate::model_gateway::ModelGateway;
use crate::model_types::{ModelAdapterKind, ModelProvider};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

#[test]
fn run_mock_turn_appends_correct_event_sequence() {
    let mut event_log = EventLog::new();
    let gateway = ModelGateway::default();
    let output = run_mock_turn(
        &mut event_log,
        "hello",
        &gateway,
        std::path::Path::new("."),
        None,
        None,
    );

    let events = event_log.events();
    let n_tokens = "Mock response for: hello".split_whitespace().count();

    let mut expected_kinds = vec![
        EventKind::RunCreate,
        EventKind::RunStart,
        EventKind::TurnStart,
        EventKind::PromptCompile,
        EventKind::ModelRoute,
    ];
    for _ in 0..n_tokens {
        expected_kinds.push(EventKind::ModelOutputChunk);
    }
    expected_kinds.push(EventKind::AssistantMessage);
    expected_kinds.push(EventKind::RunComplete);

    assert_eq!(events.len(), expected_kinds.len());
    for (ev, expected) in events.iter().zip(expected_kinds.iter()) {
        assert_eq!(ev.kind, *expected);
    }

    // No UserMessage event in the runner output.
    assert!(!events.iter().any(|e| e.kind == EventKind::UserMessage));

    // Suppress unused variable warning — output is used to verify the sequence compiles.
    let _ = output;
}

#[test]
fn run_mock_turn_returns_correct_output_fields() {
    let mut event_log = EventLog::new();
    let gateway = ModelGateway::default();
    let output = run_mock_turn(
        &mut event_log,
        "hello",
        &gateway,
        std::path::Path::new("."),
        None,
        None,
    );

    assert_eq!(output.user_message, "hello");
    assert_eq!(output.assistant_response, "Mock response for: hello");
}

#[test]
fn run_mock_turn_prompt_compile_detail_matches() {
    let mut event_log = EventLog::new();
    let gateway = ModelGateway::default();
    run_mock_turn(
        &mut event_log,
        "hello",
        &gateway,
        std::path::Path::new("."),
        None,
        None,
    );

    let events = event_log.events();
    let pc = events
        .iter()
        .find(|e| e.kind == EventKind::PromptCompile)
        .expect("PromptCompile event should exist");

    assert_eq!(pc.detail, crate::prompt::compile_prompt("hello"));
    // Pin the empty-history delegation contract: with no prior UserMessage
    // in the log, the compiled prompt must carry the no-prior marker.
    assert!(pc.detail.contains("No prior conversation context."));
}

#[test]
fn run_mock_turn_first_message_prompt_has_no_prior_context() {
    let mut event_log = EventLog::new();
    let gateway = ModelGateway::default();
    run_mock_turn(
        &mut event_log,
        "hello",
        &gateway,
        std::path::Path::new("."),
        None,
        None,
    );

    let pc = event_log
        .events()
        .iter()
        .find(|e| e.kind == EventKind::PromptCompile)
        .expect("PromptCompile event should exist");
    assert!(pc.detail.contains("No prior conversation context."));
}

#[test]
fn run_mock_turn_second_message_includes_prior_transcript() {
    let mut event_log = EventLog::new();
    let gateway = ModelGateway::default();

    // The app appends the UserMessage before the runner runs; mirror that.
    event_log.append(EventKind::UserMessage, "first");
    run_mock_turn(
        &mut event_log,
        "first",
        &gateway,
        std::path::Path::new("."),
        None,
        None,
    );

    event_log.append(EventKind::UserMessage, "second");
    run_mock_turn(
        &mut event_log,
        "second",
        &gateway,
        std::path::Path::new("."),
        None,
        None,
    );

    let events = event_log.events();
    let second_pc = events
        .iter()
        .filter(|e| e.kind == EventKind::PromptCompile)
        .nth(1)
        .expect("second PromptCompile event should exist");

    assert!(second_pc.detail.contains("User: first"));
    assert!(
        second_pc
            .detail
            .contains("Assistant: Mock response for: first")
    );
    assert!(second_pc.detail.contains("Current User:\nsecond"));
    // The current message must not be duplicated into the Conversation section.
    assert!(!second_pc.detail.contains("User: second"));
}

#[test]
fn run_mock_turn_excludes_non_conversation_events_from_context() {
    let mut event_log = EventLog::new();
    let gateway = ModelGateway::default();

    event_log.append(EventKind::UserMessage, "first");
    run_mock_turn(
        &mut event_log,
        "first",
        &gateway,
        std::path::Path::new("."),
        None,
        None,
    );

    // A slash command and other trace events must never enter the context.
    event_log.append(EventKind::SlashCommand, "/help");
    event_log.append(EventKind::UserMessage, "second");
    run_mock_turn(
        &mut event_log,
        "second",
        &gateway,
        std::path::Path::new("."),
        None,
        None,
    );

    let events = event_log.events();
    let second_pc = events
        .iter()
        .filter(|e| e.kind == EventKind::PromptCompile)
        .nth(1)
        .expect("second PromptCompile event should exist");

    assert!(!second_pc.detail.contains("/help"));
    assert!(!second_pc.detail.contains("ModelRoute"));
    assert!(!second_pc.detail.contains("ModelOutputChunk"));
    assert!(!second_pc.detail.contains("ModelUsage"));
}

#[test]
fn run_mock_turn_ids_match_event_seq_details() {
    let mut event_log = EventLog::new();
    let gateway = ModelGateway::default();
    let output = run_mock_turn(
        &mut event_log,
        "hello",
        &gateway,
        std::path::Path::new("."),
        None,
        None,
    );

    let events = event_log.events();

    let run_created = events
        .iter()
        .find(|e| e.kind == EventKind::RunCreate)
        .expect("RunCreate event should exist");
    // The run_id in the output must match what is embedded in the RunCreate detail.
    assert!(
        run_created
            .detail
            .contains(&format!("run_id={}", output.run_id)),
        "RunCreate detail should contain run_id={}: {}",
        output.run_id,
        run_created.detail
    );
    // Verify run_id is seq-derived (run-{seq}).
    assert_eq!(
        output.run_id,
        format!("run-{}", run_created.seq),
        "run_id should equal run-{{seq of RunCreate event}}"
    );

    let turn_started = events
        .iter()
        .find(|e| e.kind == EventKind::TurnStart)
        .expect("TurnStart event should exist");
    assert!(
        turn_started
            .detail
            .contains(&format!("turn_id={}", output.turn_id)),
        "TurnStart detail should contain turn_id={}: {}",
        output.turn_id,
        turn_started.detail
    );
    // Verify turn_id is seq-derived (turn-{seq}).
    assert_eq!(
        output.turn_id,
        format!("turn-{}", turn_started.seq),
        "turn_id should equal turn-{{seq of TurnStart event}}"
    );
}

#[test]
fn run_mock_turn_chunk_count_matches_model_output() {
    let mut event_log = EventLog::new();
    let gateway = ModelGateway::default();
    run_mock_turn(
        &mut event_log,
        "hello",
        &gateway,
        std::path::Path::new("."),
        None,
        None,
    );

    let token_events = event_log
        .events()
        .iter()
        .filter(|e| e.kind == EventKind::ModelOutputChunk)
        .count();
    let expected = ModelGateway::default()
        .complete(ModelRequest {
            prompt: crate::prompt::compile_prompt("hello"),
            user_message: "hello".to_string(),
        })
        .unwrap()
        .chunks
        .len();
    assert_eq!(token_events, expected);
}

#[test]
fn run_mock_turn_response_matches_model_output() {
    let mut event_log = EventLog::new();
    let gateway = ModelGateway::default();
    let output = run_mock_turn(
        &mut event_log,
        "hello",
        &gateway,
        std::path::Path::new("."),
        None,
        None,
    );

    assert_eq!(
        output.assistant_response,
        ModelGateway::default()
            .complete(ModelRequest {
                prompt: crate::prompt::compile_prompt("hello"),
                user_message: "hello".to_string(),
            })
            .unwrap()
            .assistant_response
    );
}

#[test]
fn run_mock_turn_model_route_event_has_correct_detail() {
    let mut event_log = EventLog::new();
    let gateway = ModelGateway::default();
    run_mock_turn(
        &mut event_log,
        "hello",
        &gateway,
        std::path::Path::new("."),
        None,
        None,
    );

    let events = event_log.events();

    // Find the PromptCompile and first ModelOutputChunk indices.
    let prompt_compile_idx = events
        .iter()
        .position(|e| e.kind == EventKind::PromptCompile)
        .expect("PromptCompile event should exist");
    let first_model_token_idx = events
        .iter()
        .position(|e| e.kind == EventKind::ModelOutputChunk)
        .expect("ModelOutputChunk event should exist");

    // Exactly one ModelRoute event.
    let model_route_events: Vec<_> = events
        .iter()
        .filter(|e| e.kind == EventKind::ModelRoute)
        .collect();
    assert_eq!(
        model_route_events.len(),
        1,
        "expected exactly one ModelRoute event"
    );

    let route_event = model_route_events[0];

    // ModelRoute must be immediately after PromptCompile and before first ModelOutputChunk.
    let route_idx = events
        .iter()
        .position(|e| e.kind == EventKind::ModelRoute)
        .expect("ModelRoute event should exist");
    assert_eq!(
        route_idx,
        prompt_compile_idx + 1,
        "ModelRoute should be immediately after PromptCompile"
    );
    assert!(
        route_idx < first_model_token_idx,
        "ModelRoute should be before the first ModelOutputChunk"
    );

    // Derive the expected detail at runtime from the default gateway so
    // this file does not need to hard-code adapter type names.
    let expected = ModelGateway::default()
        .complete(ModelRequest {
            prompt: String::new(),
            user_message: String::new(),
        })
        .unwrap()
        .route
        .detail();
    assert_eq!(route_event.detail, expected);
}

#[test]
fn run_mock_turn_error_path_emits_model_error_and_run_fail_events() {
    let mut event_log = EventLog::new();
    let gateway = ModelGateway::failing_for_test(ModelError::AdapterFailure {
        message: "injected failure".into(),
    });
    let output = run_mock_turn(
        &mut event_log,
        "hello",
        &gateway,
        std::path::Path::new("."),
        None,
        None,
    );

    let events = event_log.events();

    // assistant_response is empty on the error path.
    assert_eq!(output.assistant_response, "");

    // Must contain ModelError followed immediately by RunFail.
    let model_error_idx = events
        .iter()
        .position(|e| e.kind == EventKind::ModelError)
        .expect("ModelError event should exist");
    let run_fail_idx = events
        .iter()
        .position(|e| e.kind == EventKind::RunFail)
        .expect("RunFail event should exist");
    assert_eq!(
        run_fail_idx,
        model_error_idx + 1,
        "RunFail should be immediately after ModelError"
    );

    // Must NOT contain ModelOutputChunk or RunComplete.
    assert!(
        !events.iter().any(|e| e.kind == EventKind::ModelOutputChunk),
        "error path must not emit ModelOutputChunk events"
    );
    assert!(
        !events.iter().any(|e| e.kind == EventKind::RunComplete),
        "error path must not emit RunComplete event"
    );
    assert!(
        !events.iter().any(|e| e.kind == EventKind::ModelUsage),
        "error path must not emit ModelUsage events"
    );
    assert!(
        !events.iter().any(|e| e.kind == EventKind::AssistantMessage),
        "error path must not emit AssistantMessage events"
    );
}

// This struct exists to test ModelUsage event sequencing. Its content does not contain a
// CARAVAN_TOOL_REQUEST block, and as of this commit detection is disabled in the default
// runtime path regardless.
struct FakeUsageOpenAIClient;

impl OpenAIHttpClient for FakeUsageOpenAIClient {
    fn send_chat_completion(
        &self,
        _plan: &OpenAIRequestPlan,
    ) -> OpenAIHttpResult<OpenAIChatResponse> {
        Ok(OpenAIChatResponse {
            choices: vec![OpenAIChatChoice {
                message: OpenAIChatMessage {
                    role: "assistant".to_string(),
                    content: Some("Hello from fake OpenAI".to_string()),
                    tool_calls: None,
                    tool_call_id: None,
                },
            }],
            usage: Some(OpenAIUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            }),
        })
    }
}

#[test]
fn run_mock_turn_with_usage_some_emits_model_usage_event_in_correct_position() {
    let config = ModelConfig {
        active_profile: ModelProfile {
            provider: ModelProvider::OpenAI,
            model: "gpt-4o".into(),
            adapter: ModelAdapterKind::OpenAICompatibleAdapter,
        },
    };
    let gateway =
        ModelGateway::with_openai_http_client_for_test(config, Box::new(FakeUsageOpenAIClient));
    let mut event_log = EventLog::new();
    let _output = run_mock_turn(
        &mut event_log,
        "hello",
        &gateway,
        std::path::Path::new("."),
        None,
        None,
    );

    let events = event_log.events();

    // Find the last ModelOutputChunk index and the ModelUsage index.
    let last_model_token_idx = events
        .iter()
        .rposition(|e| e.kind == EventKind::ModelOutputChunk)
        .expect("ModelOutputChunk events should exist");
    let model_usage_idx = events
        .iter()
        .position(|e| e.kind == EventKind::ModelUsage)
        .expect("ModelUsage event should exist");
    let run_complete_idx = events
        .iter()
        .position(|e| e.kind == EventKind::RunComplete)
        .expect("RunComplete event should exist");

    // ModelUsage must be after the last ModelOutputChunk and immediately before RunComplete.
    assert!(
        model_usage_idx > last_model_token_idx,
        "ModelUsage must be after the last ModelOutputChunk"
    );
    assert_eq!(
        model_usage_idx + 1,
        run_complete_idx,
        "ModelUsage must be immediately before RunComplete"
    );

    // Verify the full kind sequence.
    let n_tokens = events
        .iter()
        .filter(|e| e.kind == EventKind::ModelOutputChunk)
        .count();
    let mut expected_kinds = vec![
        EventKind::RunCreate,
        EventKind::RunStart,
        EventKind::TurnStart,
        EventKind::PromptCompile,
        EventKind::ModelRoute,
    ];
    for _ in 0..n_tokens {
        expected_kinds.push(EventKind::ModelOutputChunk);
    }
    expected_kinds.push(EventKind::AssistantMessage);
    expected_kinds.push(EventKind::ModelUsage);
    expected_kinds.push(EventKind::RunComplete);

    assert_eq!(events.len(), expected_kinds.len());
    for (ev, expected) in events.iter().zip(expected_kinds.iter()) {
        assert_eq!(ev.kind, *expected);
    }
}

#[test]
fn run_mock_turn_with_usage_some_model_usage_detail_is_exact() {
    let config = ModelConfig {
        active_profile: ModelProfile {
            provider: ModelProvider::OpenAI,
            model: "gpt-4o".into(),
            adapter: ModelAdapterKind::OpenAICompatibleAdapter,
        },
    };
    let gateway =
        ModelGateway::with_openai_http_client_for_test(config, Box::new(FakeUsageOpenAIClient));
    let mut event_log = EventLog::new();
    run_mock_turn(
        &mut event_log,
        "hello",
        &gateway,
        std::path::Path::new("."),
        None,
        None,
    );

    let events = event_log.events();
    let usage_event = events
        .iter()
        .find(|e| e.kind == EventKind::ModelUsage)
        .expect("ModelUsage event should exist");

    assert_eq!(
        usage_event.detail,
        "prompt_tokens=10 completion_tokens=5 total_tokens=15"
    );
}

#[test]
fn run_mock_turn_with_usage_none_emits_no_model_usage_event() {
    let mut event_log = EventLog::new();
    let gateway = ModelGateway::default();
    run_mock_turn(
        &mut event_log,
        "hello",
        &gateway,
        std::path::Path::new("."),
        None,
        None,
    );

    let events = event_log.events();

    assert!(
        !events.iter().any(|e| e.kind == EventKind::ModelUsage),
        "usage:None path must not emit ModelUsage events"
    );

    // Verify the event sequence is unchanged (RunCreate, RunStart, TurnStart,
    // PromptCompile, ModelRoute, ModelOutputChunk*, AssistantMessage, RunComplete).
    let n_tokens = events
        .iter()
        .filter(|e| e.kind == EventKind::ModelOutputChunk)
        .count();
    let mut expected_kinds = vec![
        EventKind::RunCreate,
        EventKind::RunStart,
        EventKind::TurnStart,
        EventKind::PromptCompile,
        EventKind::ModelRoute,
    ];
    for _ in 0..n_tokens {
        expected_kinds.push(EventKind::ModelOutputChunk);
    }
    expected_kinds.push(EventKind::AssistantMessage);
    expected_kinds.push(EventKind::RunComplete);

    assert_eq!(events.len(), expected_kinds.len());
    for (ev, expected) in events.iter().zip(expected_kinds.iter()) {
        assert_eq!(ev.kind, *expected);
    }
}

#[test]
fn run_mock_turn_assistant_message_detail_and_position_for_hello_caravan() {
    let mut event_log = EventLog::new();
    let gateway = ModelGateway::default();
    run_mock_turn(
        &mut event_log,
        "hello caravan",
        &gateway,
        std::path::Path::new("."),
        None,
        None,
    );

    let events = event_log.events();

    let assistant_msg_idx = events
        .iter()
        .position(|e| e.kind == EventKind::AssistantMessage)
        .expect("AssistantMessage event should exist");
    assert_eq!(
        events[assistant_msg_idx].detail,
        "Mock response for: hello caravan"
    );

    let last_chunk_idx = events
        .iter()
        .rposition(|e| e.kind == EventKind::ModelOutputChunk)
        .expect("ModelOutputChunk events should exist");
    let run_complete_idx = events
        .iter()
        .position(|e| e.kind == EventKind::RunComplete)
        .expect("RunComplete event should exist");

    assert!(
        assistant_msg_idx > last_chunk_idx,
        "AssistantMessage must be after the last ModelOutputChunk"
    );
    assert!(
        assistant_msg_idx < run_complete_idx,
        "AssistantMessage must be before RunComplete"
    );
}

struct FakeReadFileToolRequestClient;

impl OpenAIHttpClient for FakeReadFileToolRequestClient {
    fn send_chat_completion(
        &self,
        _plan: &OpenAIRequestPlan,
    ) -> OpenAIHttpResult<OpenAIChatResponse> {
        Ok(OpenAIChatResponse {
            choices: vec![OpenAIChatChoice {
                message: OpenAIChatMessage {
                    role: "assistant".to_string(),
                    content: Some("preamble\nCARAVAN_TOOL_REQUEST\ntool=read_file\npath=README.md\nEND_CARAVAN_TOOL_REQUEST\npostamble".to_string()),
                    tool_calls: None,
                    tool_call_id: None,
                },
            }],
            usage: Some(OpenAIUsage {
                prompt_tokens: 8,
                completion_tokens: 4,
                total_tokens: 12,
            }),
        })
    }
}

#[test]
fn run_mock_turn_does_not_detect_read_file_tool_request_by_default() {
    let config = ModelConfig {
        active_profile: ModelProfile {
            provider: ModelProvider::OpenAI,
            model: "gpt-4o".into(),
            adapter: ModelAdapterKind::OpenAICompatibleAdapter,
        },
    };
    let gateway = ModelGateway::with_openai_http_client_for_test(
        config,
        Box::new(FakeReadFileToolRequestClient),
    );
    let mut event_log = EventLog::new();
    run_mock_turn(
        &mut event_log,
        "hello",
        &gateway,
        std::path::Path::new("."),
        None,
        None,
    );

    let events = event_log.events();

    assert!(
        !events.iter().any(|e| e.kind == EventKind::ModelToolRequest),
        "ModelToolRequest must not be emitted in default path even when CARAVAN_TOOL_REQUEST block is present"
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
}

struct FakeListFilesToolRequestClient;

impl OpenAIHttpClient for FakeListFilesToolRequestClient {
    fn send_chat_completion(
        &self,
        _plan: &OpenAIRequestPlan,
    ) -> OpenAIHttpResult<OpenAIChatResponse> {
        Ok(OpenAIChatResponse {
            choices: vec![OpenAIChatChoice {
                message: OpenAIChatMessage {
                    role: "assistant".to_string(),
                    content: Some("preamble\nCARAVAN_TOOL_REQUEST\ntool=list_files\npath=.\nEND_CARAVAN_TOOL_REQUEST\npostamble".to_string()),
                    tool_calls: None,
                    tool_call_id: None,
                },
            }],
            usage: None,
        })
    }
}

#[test]
fn run_mock_turn_does_not_detect_list_files_tool_request_by_default() {
    let config = ModelConfig {
        active_profile: ModelProfile {
            provider: ModelProvider::OpenAI,
            model: "gpt-4o".into(),
            adapter: ModelAdapterKind::OpenAICompatibleAdapter,
        },
    };
    let gateway = ModelGateway::with_openai_http_client_for_test(
        config,
        Box::new(FakeListFilesToolRequestClient),
    );
    let mut event_log = EventLog::new();
    run_mock_turn(
        &mut event_log,
        "hello",
        &gateway,
        std::path::Path::new("."),
        None,
        None,
    );

    let events = event_log.events();

    assert!(
        !events.iter().any(|e| e.kind == EventKind::ModelToolRequest),
        "ModelToolRequest must not be emitted in default path even when CARAVAN_TOOL_REQUEST block is present"
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
}

struct FakeSentinelPathClient;

impl OpenAIHttpClient for FakeSentinelPathClient {
    fn send_chat_completion(
        &self,
        _plan: &OpenAIRequestPlan,
    ) -> OpenAIHttpResult<OpenAIChatResponse> {
        Ok(OpenAIChatResponse {
            choices: vec![OpenAIChatChoice {
                message: OpenAIChatMessage {
                    role: "assistant".to_string(),
                    content: Some("preamble\nCARAVAN_TOOL_REQUEST\ntool=read_file\npath=/definitely/not/a/real/caravan/sentinel/file\nEND_CARAVAN_TOOL_REQUEST\npostamble".to_string()),
                    tool_calls: None,
                    tool_call_id: None,
                },
            }],
            usage: None,
        })
    }
}

#[test]
fn run_mock_turn_sentinel_path_does_not_touch_filesystem_by_default() {
    let config = ModelConfig {
        active_profile: ModelProfile {
            provider: ModelProvider::OpenAI,
            model: "gpt-4o".into(),
            adapter: ModelAdapterKind::OpenAICompatibleAdapter,
        },
    };
    let gateway =
        ModelGateway::with_openai_http_client_for_test(config, Box::new(FakeSentinelPathClient));
    let mut event_log = EventLog::new();
    run_mock_turn(
        &mut event_log,
        "hello",
        &gateway,
        std::path::Path::new("."),
        None,
        None,
    );

    let events = event_log.events();

    assert!(
        !events.iter().any(|e| e.kind == EventKind::ModelToolRequest),
        "ModelToolRequest must not be emitted in default path even when CARAVAN_TOOL_REQUEST block is present"
    );
    assert!(
        !events.iter().any(|e| e.kind == EventKind::ModelError),
        "must not emit ModelError events"
    );
    assert!(
        !events.iter().any(|e| e.kind == EventKind::RunFail),
        "must not emit RunFail events"
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
}

#[test]
fn run_mock_turn_without_tool_request_block_emits_no_model_tool_request_event() {
    let mut event_log = EventLog::new();
    let gateway = ModelGateway::default();
    run_mock_turn(
        &mut event_log,
        "hello",
        &gateway,
        std::path::Path::new("."),
        None,
        None,
    );

    let events = event_log.events();

    assert!(
        !events.iter().any(|e| e.kind == EventKind::ModelToolRequest),
        "must not emit ModelToolRequest when response has no block"
    );
}

#[test]
fn run_mock_turn_error_path_emits_no_model_tool_request_event() {
    let mut event_log = EventLog::new();
    let gateway = ModelGateway::failing_for_test(ModelError::AdapterFailure {
        message: "injected failure for tool request test".into(),
    });
    run_mock_turn(
        &mut event_log,
        "hello",
        &gateway,
        std::path::Path::new("."),
        None,
        None,
    );

    let events = event_log.events();

    assert!(
        !events.iter().any(|e| e.kind == EventKind::ModelToolRequest),
        "error path must not emit ModelToolRequest events"
    );
}

#[test]
fn run_mock_turn_does_not_detect_model_tool_request_read_file_by_default() {
    let config = ModelConfig {
        active_profile: ModelProfile {
            provider: ModelProvider::OpenAI,
            model: "gpt-4o".into(),
            adapter: ModelAdapterKind::OpenAICompatibleAdapter,
        },
    };
    let gateway = ModelGateway::with_openai_http_client_for_test(
        config,
        Box::new(FakeReadFileToolRequestClient),
    );
    let mut event_log = EventLog::new();
    let output = run_mock_turn(
        &mut event_log,
        "hello",
        &gateway,
        std::path::Path::new("."),
        None,
        None,
    );

    assert!(
        output.detected_model_tool_request.is_none(),
        "detected_model_tool_request must be None in default path even when CARAVAN_TOOL_REQUEST block is present"
    );

    let events = event_log.events();
    assert!(
        !events.iter().any(|e| e.kind == EventKind::ModelToolRequest),
        "must not emit ModelToolRequest events in default path"
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
}

#[test]
fn run_mock_turn_does_not_detect_model_tool_request_list_files_by_default() {
    let config = ModelConfig {
        active_profile: ModelProfile {
            provider: ModelProvider::OpenAI,
            model: "gpt-4o".into(),
            adapter: ModelAdapterKind::OpenAICompatibleAdapter,
        },
    };
    let gateway = ModelGateway::with_openai_http_client_for_test(
        config,
        Box::new(FakeListFilesToolRequestClient),
    );
    let mut event_log = EventLog::new();
    let output = run_mock_turn(
        &mut event_log,
        "hello",
        &gateway,
        std::path::Path::new("."),
        None,
        None,
    );

    assert!(
        output.detected_model_tool_request.is_none(),
        "detected_model_tool_request must be None in default path even when CARAVAN_TOOL_REQUEST block is present"
    );

    let events = event_log.events();
    assert!(
        !events.iter().any(|e| e.kind == EventKind::ModelToolRequest),
        "must not emit ModelToolRequest events in default path"
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
}

#[test]
fn run_mock_turn_detected_model_tool_request_none_for_default_mock() {
    let mut event_log = EventLog::new();
    let gateway = ModelGateway::default();
    let output = run_mock_turn(
        &mut event_log,
        "hello",
        &gateway,
        std::path::Path::new("."),
        None,
        None,
    );

    assert!(
        output.detected_model_tool_request.is_none(),
        "default mock path must return None for detected_model_tool_request"
    );
}

#[test]
fn run_mock_turn_detected_model_tool_request_none_for_error_path() {
    let mut event_log = EventLog::new();
    let gateway = ModelGateway::failing_for_test(ModelError::AdapterFailure {
        message: "injected failure".into(),
    });
    let output = run_mock_turn(
        &mut event_log,
        "hello",
        &gateway,
        std::path::Path::new("."),
        None,
        None,
    );

    assert!(
        output.detected_model_tool_request.is_none(),
        "error path must return None for detected_model_tool_request"
    );
}

#[test]
fn run_mock_turn_with_project_memory_includes_memory_in_prompt_compile() {
    use std::io::Write as _;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("runner_pm_test_{}_{}", std::process::id(), id));
    std::fs::create_dir_all(&dir).unwrap();

    let claude_md_content = "# My Test Project\nBuild: cargo build";
    let mut f = std::fs::File::create(dir.join("CLAUDE.md")).unwrap();
    f.write_all(claude_md_content.as_bytes()).unwrap();
    drop(f);

    let project_memory = crate::project_memory::load_project_memory(&dir);

    let mut event_log = EventLog::new();
    let gateway = ModelGateway::default();
    run_mock_turn(
        &mut event_log,
        "hello",
        &gateway,
        std::path::Path::new("."),
        None,
        Some(&project_memory),
    );

    let events = event_log.events();
    let pc = events
        .iter()
        .find(|e| e.kind == EventKind::PromptCompile)
        .expect("PromptCompile event should exist");

    assert!(
        pc.detail.contains("Project Memory:"),
        "PromptCompile detail should contain 'Project Memory:'"
    );
    assert!(
        pc.detail.contains("# My Test Project"),
        "PromptCompile detail should contain CLAUDE.md heading"
    );
    assert!(
        pc.detail.contains("Build: cargo build"),
        "PromptCompile detail should contain CLAUDE.md body"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

// ============================================================================
// QueuedFakeClient — stateful scripted HTTP client for native tool round-trip
// integration tests.
//
// Interior mutability via Mutex is required because send_chat_completion takes
// &self while both the response queue and the request log must be mutated.
// ============================================================================

struct QueuedFakeClient {
    responses: Mutex<VecDeque<OpenAIChatResponse>>,
    /// Records whether each request carried a `tools` field (true = had tools).
    requests: Mutex<Vec<bool>>,
}

impl QueuedFakeClient {
    fn new(responses: Vec<OpenAIChatResponse>) -> Self {
        Self {
            responses: Mutex::new(VecDeque::from(responses)),
            requests: Mutex::new(Vec::new()),
        }
    }

    fn request_count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }

    fn request_had_tools(&self, idx: usize) -> bool {
        self.requests.lock().unwrap()[idx]
    }
}

// Implement the trait for Arc<QueuedFakeClient> so the gateway can own the
// Box<dyn OpenAIHttpClient> while the test keeps an Arc clone for inspection.
impl OpenAIHttpClient for Arc<QueuedFakeClient> {
    fn send_chat_completion(
        &self,
        plan: &OpenAIRequestPlan,
    ) -> OpenAIHttpResult<OpenAIChatResponse> {
        self.requests
            .lock()
            .unwrap()
            .push(plan.body.tools.is_some());
        self.responses
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| OpenAIHttpError::RequestFailure {
                message: "QueuedFakeClient: response queue exhausted".to_string(),
            })
    }
}

// --- Response builders -------------------------------------------------------

fn openai_config() -> ModelConfig {
    ModelConfig {
        active_profile: ModelProfile {
            provider: crate::model_types::ModelProvider::OpenAI,
            model: "gpt-4o".into(),
            adapter: crate::model_types::ModelAdapterKind::OpenAICompatibleAdapter,
        },
    }
}

fn assistant_resp(content: &str, usage: Option<OpenAIUsage>) -> OpenAIChatResponse {
    OpenAIChatResponse {
        choices: vec![OpenAIChatChoice {
            message: OpenAIChatMessage {
                role: "assistant".to_string(),
                content: Some(content.to_string()),
                tool_calls: None,
                tool_call_id: None,
            },
        }],
        usage,
    }
}

fn tool_call_resp(
    name: &str,
    id: &str,
    args: &str,
    usage: Option<OpenAIUsage>,
) -> OpenAIChatResponse {
    OpenAIChatResponse {
        choices: vec![OpenAIChatChoice {
            message: OpenAIChatMessage {
                role: "assistant".to_string(),
                content: None,
                tool_calls: Some(vec![OpenAIToolCall {
                    id: id.to_string(),
                    kind: "function".to_string(),
                    function: OpenAIToolCallFunction {
                        name: name.to_string(),
                        arguments: args.to_string(),
                    },
                }]),
                tool_call_id: None,
            },
        }],
        usage,
    }
}

/// A response with two identical tool calls — rejected at the adapter level.
fn two_tool_calls_resp() -> OpenAIChatResponse {
    let tc = OpenAIToolCall {
        id: "call-dup".to_string(),
        kind: "function".to_string(),
        function: OpenAIToolCallFunction {
            name: "list_files".to_string(),
            arguments: "{}".to_string(),
        },
    };
    OpenAIChatResponse {
        choices: vec![OpenAIChatChoice {
            message: OpenAIChatMessage {
                role: "assistant".to_string(),
                content: None,
                tool_calls: Some(vec![tc.clone(), tc]),
                tool_call_id: None,
            },
        }],
        usage: None,
    }
}

/// Create a new temp dir, write a small file, and return the dir path.
fn make_temp_workspace(filename: &str, content: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static CTR: AtomicU64 = AtomicU64::new(0);
    let id = CTR.fetch_add(1, Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("kernel_runner_test_{}_{}", std::process::id(), id));
    std::fs::create_dir_all(&dir).unwrap();
    if !filename.is_empty() {
        std::fs::write(dir.join(filename), content).unwrap();
    }
    dir
}

// --- Test (a): read_file two-call success -----------------------------------

#[test]
fn native_tool_read_file_two_call_success_event_order() {
    let workspace = make_temp_workspace("hello.txt", "hello content");
    let client = Arc::new(QueuedFakeClient::new(vec![
        tool_call_resp(
            "read_file",
            "call-1",
            r#"{"path":"hello.txt"}"#,
            Some(OpenAIUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 12,
            }),
        ),
        assistant_resp(
            "Here is what I found.",
            Some(OpenAIUsage {
                prompt_tokens: 8,
                completion_tokens: 3,
                total_tokens: 9,
            }),
        ),
    ]));
    let gateway = ModelGateway::with_openai_http_client_for_test(
        openai_config(),
        Box::new(Arc::clone(&client)),
    );
    let mut event_log = EventLog::new();
    let output = run_mock_turn(
        &mut event_log,
        "read hello.txt",
        &gateway,
        &workspace,
        None,
        None,
    );

    let events = event_log.events();
    let kinds: Vec<EventKind> = events.iter().map(|e| e.kind).collect();

    // Assert event order up to first variable-length block.
    assert_eq!(kinds[0], EventKind::RunCreate);
    assert_eq!(kinds[1], EventKind::RunStart);
    assert_eq!(kinds[2], EventKind::TurnStart);
    assert_eq!(kinds[3], EventKind::PromptCompile);
    assert_eq!(kinds[4], EventKind::ModelRoute);
    assert_eq!(kinds[5], EventKind::ToolPolicy);
    assert_eq!(kinds[6], EventKind::ToolCall);
    assert_eq!(kinds[7], EventKind::ToolResult);

    // After ToolResult: second ModelRoute, then chunks, AssistantMessage, RunComplete.
    let second_route_idx = kinds
        .iter()
        .rposition(|&k| k == EventKind::ModelRoute)
        .unwrap();
    assert!(
        second_route_idx > 4,
        "second ModelRoute must be after first"
    );

    let run_complete_idx = kinds
        .iter()
        .position(|&k| k == EventKind::RunComplete)
        .expect("RunComplete must be present");
    let last_event = kinds.last().unwrap();
    assert_eq!(*last_event, EventKind::RunComplete);

    let assistant_msg_idx = kinds
        .iter()
        .position(|&k| k == EventKind::AssistantMessage)
        .unwrap();
    assert!(assistant_msg_idx > second_route_idx);
    assert!(assistant_msg_idx < run_complete_idx);

    // Exactly ONE PromptCompile.
    let pc_count = kinds
        .iter()
        .filter(|&&k| k == EventKind::PromptCompile)
        .count();
    assert_eq!(pc_count, 1, "exactly one PromptCompile");

    // Exactly TWO ModelRoute.
    let mr_count = kinds
        .iter()
        .filter(|&&k| k == EventKind::ModelRoute)
        .count();
    assert_eq!(mr_count, 2, "exactly two ModelRoute events");

    // Exactly ONE ToolCall.
    let tc_count = kinds.iter().filter(|&&k| k == EventKind::ToolCall).count();
    assert_eq!(tc_count, 1, "exactly one ToolCall event");

    // No RunFail.
    assert!(
        !kinds.contains(&EventKind::RunFail),
        "no RunFail on success path"
    );

    // Fake recorded exactly 2 requests; second has tools == None.
    assert_eq!(client.request_count(), 2, "exactly 2 HTTP requests");
    assert!(
        client.request_had_tools(0),
        "first request must carry tools"
    );
    assert!(
        !client.request_had_tools(1),
        "second request must NOT carry tools"
    );

    // tool_activity is Some and succeeded.
    let activity = output.tool_activity.expect("tool_activity must be Some");
    assert!(activity.succeeded);
    assert_eq!(activity.name, "read_file");

    let _ = std::fs::remove_dir_all(&workspace);
}

// --- Test (a) continued: usage detail is exact ----------------------------

#[test]
fn native_tool_read_file_two_call_usage_detail_is_aggregated() {
    let workspace = make_temp_workspace("hello.txt", "content");
    let client = Arc::new(QueuedFakeClient::new(vec![
        tool_call_resp(
            "read_file",
            "call-1",
            r#"{"path":"hello.txt"}"#,
            Some(OpenAIUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 12,
            }),
        ),
        assistant_resp(
            "Done.",
            Some(OpenAIUsage {
                prompt_tokens: 8,
                completion_tokens: 3,
                total_tokens: 9,
            }),
        ),
    ]));
    let gateway = ModelGateway::with_openai_http_client_for_test(
        openai_config(),
        Box::new(Arc::clone(&client)),
    );
    let mut event_log = EventLog::new();
    run_mock_turn(
        &mut event_log,
        "read hello.txt",
        &gateway,
        &workspace,
        None,
        None,
    );

    let events = event_log.events();
    let usage_event = events
        .iter()
        .find(|e| e.kind == EventKind::ModelUsage)
        .expect("ModelUsage must be present");
    // Independent sums: 10+8=18, 5+3=8, 12+9=21 (NOT 18+8=26 for total).
    assert_eq!(
        usage_event.detail,
        "prompt_tokens=18 completion_tokens=8 total_tokens=21"
    );

    let _ = std::fs::remove_dir_all(&workspace);
}

// --- Test (b): list_files two-call success (path omitted -> default ".") ---

#[test]
fn native_tool_list_files_no_path_arg_defaults_to_dot() {
    let workspace = make_temp_workspace("", "");
    let client = Arc::new(QueuedFakeClient::new(vec![
        // list_files with no path argument — gateway will see arguments: {}
        tool_call_resp("list_files", "call-2", "{}", None),
        assistant_resp("Listed.", None),
    ]));
    let gateway = ModelGateway::with_openai_http_client_for_test(
        openai_config(),
        Box::new(Arc::clone(&client)),
    );
    let mut event_log = EventLog::new();
    let output = run_mock_turn(
        &mut event_log,
        "list files",
        &gateway,
        &workspace,
        None,
        None,
    );

    let events = event_log.events();
    let kinds: Vec<EventKind> = events.iter().map(|e| e.kind).collect();

    assert!(
        kinds.contains(&EventKind::RunComplete),
        "must reach RunComplete"
    );
    assert!(!kinds.contains(&EventKind::RunFail), "must not RunFail");
    assert!(kinds.contains(&EventKind::ToolCall), "must have ToolCall");
    assert!(
        kinds.contains(&EventKind::ToolResult),
        "must have ToolResult"
    );

    let activity = output.tool_activity.expect("tool_activity must be Some");
    assert_eq!(activity.name, "list_files");
    // The default path is captured from the validated ToolRequest, not the raw args.
    assert_eq!(activity.path, ".", "list_files default path must be '.'");
    assert!(activity.succeeded);

    let _ = std::fs::remove_dir_all(&workspace);
}

// --- Test (c): tool execution error (read_file missing -> NotFound) ---------

#[test]
fn native_tool_read_file_not_found_still_completes_second_call() {
    let workspace = make_temp_workspace("", "");
    let client = Arc::new(QueuedFakeClient::new(vec![
        tool_call_resp(
            "read_file",
            "call-3",
            r#"{"path":"missing_sentinel_file.txt"}"#,
            None,
        ),
        assistant_resp("Could not read the file.", None),
    ]));
    let gateway = ModelGateway::with_openai_http_client_for_test(
        openai_config(),
        Box::new(Arc::clone(&client)),
    );
    let mut event_log = EventLog::new();
    let output = run_mock_turn(
        &mut event_log,
        "read missing file",
        &gateway,
        &workspace,
        None,
        None,
    );

    let events = event_log.events();
    let kinds: Vec<EventKind> = events.iter().map(|e| e.kind).collect();

    // ToolError must be present (not ToolResult).
    assert!(kinds.contains(&EventKind::ToolError), "must have ToolError");
    assert!(
        !kinds.contains(&EventKind::ToolResult),
        "must NOT have ToolResult on error path"
    );

    // Second model call still happens — two ModelRoute events.
    let mr_count = kinds
        .iter()
        .filter(|&&k| k == EventKind::ModelRoute)
        .count();
    assert_eq!(mr_count, 2, "two ModelRoute events even on tool error");

    assert!(
        kinds.contains(&EventKind::RunComplete),
        "must reach RunComplete"
    );
    assert!(!kinds.contains(&EventKind::RunFail), "must not RunFail");

    // tool_activity reflects the failure.
    let activity = output.tool_activity.expect("tool_activity must be Some");
    assert_eq!(activity.name, "read_file");
    assert!(!activity.succeeded, "tool_activity.succeeded must be false");

    let _ = std::fs::remove_dir_all(&workspace);
}

// --- Test (d): bridge-level invalid first call (ONE ModelRoute, no tool events) ---

#[test]
fn native_tool_unsupported_tool_name_is_bridge_error_one_model_route() {
    let workspace = make_temp_workspace("", "");
    let client = Arc::new(QueuedFakeClient::new(vec![
        // unsupported tool name — passes gateway validation (object args), fails bridge
        tool_call_resp("shell_exec", "call-bad", r#"{"cmd":"ls"}"#, None),
    ]));
    let gateway = ModelGateway::with_openai_http_client_for_test(
        openai_config(),
        Box::new(Arc::clone(&client)),
    );
    let mut event_log = EventLog::new();
    run_mock_turn(
        &mut event_log,
        "run shell",
        &gateway,
        &workspace,
        None,
        None,
    );

    let events = event_log.events();
    let kinds: Vec<EventKind> = events.iter().map(|e| e.kind).collect();

    assert_eq!(
        kinds
            .iter()
            .filter(|&&k| k == EventKind::ModelRoute)
            .count(),
        1,
        "exactly one ModelRoute for bridge error"
    );
    assert!(
        kinds.contains(&EventKind::ModelError),
        "must have ModelError"
    );
    assert!(kinds.contains(&EventKind::RunFail), "must have RunFail");
    assert!(!kinds.contains(&EventKind::ToolPolicy), "no ToolPolicy");
    assert!(!kinds.contains(&EventKind::ToolCall), "no ToolCall");
    assert!(!kinds.contains(&EventKind::ToolResult), "no ToolResult");
    assert!(!kinds.contains(&EventKind::RunComplete), "no RunComplete");

    let _ = std::fs::remove_dir_all(&workspace);
}

#[test]
fn native_tool_read_file_missing_path_is_bridge_error_one_model_route() {
    let workspace = make_temp_workspace("", "");
    let client = Arc::new(QueuedFakeClient::new(vec![
        // read_file with no path — object args pass gateway, missing path fails bridge
        tool_call_resp("read_file", "call-nopath", "{}", None),
    ]));
    let gateway = ModelGateway::with_openai_http_client_for_test(
        openai_config(),
        Box::new(Arc::clone(&client)),
    );
    let mut event_log = EventLog::new();
    run_mock_turn(
        &mut event_log,
        "read without path",
        &gateway,
        &workspace,
        None,
        None,
    );

    let events = event_log.events();
    let kinds: Vec<EventKind> = events.iter().map(|e| e.kind).collect();

    assert_eq!(
        kinds
            .iter()
            .filter(|&&k| k == EventKind::ModelRoute)
            .count(),
        1,
        "exactly one ModelRoute for bridge error"
    );
    assert!(kinds.contains(&EventKind::ModelError));
    assert!(kinds.contains(&EventKind::RunFail));
    assert!(!kinds.contains(&EventKind::ToolPolicy));
    assert!(!kinds.contains(&EventKind::ToolCall));
    assert!(!kinds.contains(&EventKind::RunComplete));

    let _ = std::fs::remove_dir_all(&workspace);
}

// --- Test (e): adapter-level invalid first call (NO ModelRoute) -------------

#[test]
fn native_tool_two_tool_calls_in_first_response_is_adapter_error_no_model_route() {
    let workspace = make_temp_workspace("", "");
    let client = Arc::new(QueuedFakeClient::new(vec![two_tool_calls_resp()]));
    let gateway = ModelGateway::with_openai_http_client_for_test(
        openai_config(),
        Box::new(Arc::clone(&client)),
    );
    let mut event_log = EventLog::new();
    run_mock_turn(
        &mut event_log,
        "trigger adapter error",
        &gateway,
        &workspace,
        None,
        None,
    );

    let events = event_log.events();
    let kinds: Vec<EventKind> = events.iter().map(|e| e.kind).collect();

    assert_eq!(
        kinds
            .iter()
            .filter(|&&k| k == EventKind::ModelRoute)
            .count(),
        0,
        "NO ModelRoute for adapter-level error"
    );
    assert!(kinds.contains(&EventKind::ModelError));
    assert!(kinds.contains(&EventKind::RunFail));
    assert!(!kinds.contains(&EventKind::ToolPolicy));
    assert!(!kinds.contains(&EventKind::ToolCall));
    assert!(!kinds.contains(&EventKind::RunComplete));

    let _ = std::fs::remove_dir_all(&workspace);
}

#[test]
fn native_tool_non_object_arguments_in_first_response_is_adapter_error_no_model_route() {
    let workspace = make_temp_workspace("", "");
    // non-object arguments: `"not an object"` — rejected by to_model_step_output
    let client = Arc::new(QueuedFakeClient::new(vec![OpenAIChatResponse {
        choices: vec![OpenAIChatChoice {
            message: OpenAIChatMessage {
                role: "assistant".to_string(),
                content: None,
                tool_calls: Some(vec![OpenAIToolCall {
                    id: "call-bad-args".to_string(),
                    kind: "function".to_string(),
                    function: OpenAIToolCallFunction {
                        name: "list_files".to_string(),
                        arguments: r#""not_an_object""#.to_string(),
                    },
                }]),
                tool_call_id: None,
            },
        }],
        usage: None,
    }]));
    let gateway = ModelGateway::with_openai_http_client_for_test(
        openai_config(),
        Box::new(Arc::clone(&client)),
    );
    let mut event_log = EventLog::new();
    run_mock_turn(
        &mut event_log,
        "non-object args",
        &gateway,
        &workspace,
        None,
        None,
    );

    let events = event_log.events();
    let kinds: Vec<EventKind> = events.iter().map(|e| e.kind).collect();

    assert_eq!(
        kinds
            .iter()
            .filter(|&&k| k == EventKind::ModelRoute)
            .count(),
        0,
        "NO ModelRoute for adapter-level error"
    );
    assert!(kinds.contains(&EventKind::ModelError));
    assert!(kinds.contains(&EventKind::RunFail));
    assert!(!kinds.contains(&EventKind::ToolCall));

    let _ = std::fs::remove_dir_all(&workspace);
}

// --- Test (f): second response is exactly one tool call (rejected) ----------

#[test]
fn native_tool_second_tool_call_not_supported_sentinel_in_event_log() {
    let workspace = make_temp_workspace("note.txt", "hi");
    let client = Arc::new(QueuedFakeClient::new(vec![
        tool_call_resp("read_file", "call-f1", r#"{"path":"note.txt"}"#, None),
        // Second response is another tool call — must be rejected.
        tool_call_resp("list_files", "call-f2", "{}", None),
    ]));
    let gateway = ModelGateway::with_openai_http_client_for_test(
        openai_config(),
        Box::new(Arc::clone(&client)),
    );
    let mut event_log = EventLog::new();
    let output = run_mock_turn(
        &mut event_log,
        "read then list",
        &gateway,
        &workspace,
        None,
        None,
    );

    let events = event_log.events();
    let kinds: Vec<EventKind> = events.iter().map(|e| e.kind).collect();

    // Two ModelRoute events (first tool call + second model call).
    assert_eq!(
        kinds
            .iter()
            .filter(|&&k| k == EventKind::ModelRoute)
            .count(),
        2,
        "two ModelRoute events"
    );
    // Exactly ONE ToolCall (the first one).
    assert_eq!(
        kinds.iter().filter(|&&k| k == EventKind::ToolCall).count(),
        1,
        "exactly one ToolCall — no second execution"
    );
    // ModelError and RunFail must be present.
    assert!(kinds.contains(&EventKind::ModelError));
    assert!(kinds.contains(&EventKind::RunFail));
    assert!(!kinds.contains(&EventKind::RunComplete));

    // Recorded request count is exactly 2.
    assert_eq!(client.request_count(), 2);

    // Error detail must contain the sentinel string.
    let model_error_event = events
        .iter()
        .find(|e| e.kind == EventKind::ModelError)
        .unwrap();
    assert!(
        model_error_event
            .detail
            .contains("second_tool_call_not_supported"),
        "ModelError must contain 'second_tool_call_not_supported': {}",
        model_error_event.detail
    );

    // tool_activity is Some (from first tool execution).
    assert!(output.tool_activity.is_some());

    let _ = std::fs::remove_dir_all(&workspace);
}

// --- Test (g): second response is adapter/wire failure (NO second ModelRoute) ---

#[test]
fn native_tool_second_call_adapter_failure_no_second_model_route() {
    let workspace = make_temp_workspace("data.txt", "data");
    let client = Arc::new(QueuedFakeClient::new(vec![
        tool_call_resp("read_file", "call-g1", r#"{"path":"data.txt"}"#, None),
        // Second response has >=2 tool calls -> adapter returns Err -> no second ModelRoute.
        two_tool_calls_resp(),
    ]));
    let gateway = ModelGateway::with_openai_http_client_for_test(
        openai_config(),
        Box::new(Arc::clone(&client)),
    );
    let mut event_log = EventLog::new();
    let output = run_mock_turn(
        &mut event_log,
        "read data",
        &gateway,
        &workspace,
        None,
        None,
    );

    let events = event_log.events();
    let kinds: Vec<EventKind> = events.iter().map(|e| e.kind).collect();

    // Exactly ONE ModelRoute (first call succeeded, second call is adapter error).
    assert_eq!(
        kinds
            .iter()
            .filter(|&&k| k == EventKind::ModelRoute)
            .count(),
        1,
        "only one ModelRoute — second call is adapter error, no second ModelRoute"
    );
    assert!(
        kinds.contains(&EventKind::ToolResult),
        "ToolResult from first execution"
    );
    assert!(kinds.contains(&EventKind::ModelError));
    assert!(kinds.contains(&EventKind::RunFail));
    assert!(!kinds.contains(&EventKind::RunComplete));

    // tool_activity is Some even on second-call failure.
    assert!(output.tool_activity.is_some());

    let _ = std::fs::remove_dir_all(&workspace);
}

// --- Test (h): usage aggregation --------------------------------------------

#[test]
fn native_tool_usage_aggregated_independently_total_is_sum_of_totals() {
    // call1 usage(prompt=10,completion=5,total=12) + call2 usage(prompt=8,completion=3,total=9)
    // -> prompt_tokens=18 completion_tokens=8 total_tokens=21 (NOT 26 = 18+8)
    let workspace = make_temp_workspace("u.txt", "u");
    let client = Arc::new(QueuedFakeClient::new(vec![
        tool_call_resp(
            "read_file",
            "call-h1",
            r#"{"path":"u.txt"}"#,
            Some(OpenAIUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 12,
            }),
        ),
        assistant_resp(
            "Aggregated.",
            Some(OpenAIUsage {
                prompt_tokens: 8,
                completion_tokens: 3,
                total_tokens: 9,
            }),
        ),
    ]));
    let gateway = ModelGateway::with_openai_http_client_for_test(
        openai_config(),
        Box::new(Arc::clone(&client)),
    );
    let mut event_log = EventLog::new();
    run_mock_turn(
        &mut event_log,
        "aggregate usage",
        &gateway,
        &workspace,
        None,
        None,
    );

    let events = event_log.events();
    let usage_events: Vec<_> = events
        .iter()
        .filter(|e| e.kind == EventKind::ModelUsage)
        .collect();
    assert_eq!(usage_events.len(), 1, "exactly one ModelUsage");
    assert_eq!(
        usage_events[0].detail, "prompt_tokens=18 completion_tokens=8 total_tokens=21",
        "independent sum: 10+8=18, 5+3=8, 12+9=21 (not 26)"
    );

    let _ = std::fs::remove_dir_all(&workspace);
}

#[test]
fn native_tool_usage_call1_has_usage_call2_none_emits_call1_usage() {
    let workspace = make_temp_workspace("v.txt", "v");
    let client = Arc::new(QueuedFakeClient::new(vec![
        tool_call_resp(
            "read_file",
            "call-hi1",
            r#"{"path":"v.txt"}"#,
            Some(OpenAIUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            }),
        ),
        assistant_resp("No usage for second call.", None),
    ]));
    let gateway = ModelGateway::with_openai_http_client_for_test(
        openai_config(),
        Box::new(Arc::clone(&client)),
    );
    let mut event_log = EventLog::new();
    run_mock_turn(
        &mut event_log,
        "h-i usage",
        &gateway,
        &workspace,
        None,
        None,
    );

    let events = event_log.events();
    let usage_events: Vec<_> = events
        .iter()
        .filter(|e| e.kind == EventKind::ModelUsage)
        .collect();
    assert_eq!(usage_events.len(), 1, "exactly one ModelUsage");
    assert_eq!(
        usage_events[0].detail,
        "prompt_tokens=10 completion_tokens=5 total_tokens=15"
    );

    let _ = std::fs::remove_dir_all(&workspace);
}

#[test]
fn native_tool_usage_neither_call_has_usage_emits_no_model_usage() {
    let workspace = make_temp_workspace("w.txt", "w");
    let client = Arc::new(QueuedFakeClient::new(vec![
        tool_call_resp("read_file", "call-hii1", r#"{"path":"w.txt"}"#, None),
        assistant_resp("No usage anywhere.", None),
    ]));
    let gateway = ModelGateway::with_openai_http_client_for_test(
        openai_config(),
        Box::new(Arc::clone(&client)),
    );
    let mut event_log = EventLog::new();
    run_mock_turn(
        &mut event_log,
        "h-ii usage",
        &gateway,
        &workspace,
        None,
        None,
    );

    let events = event_log.events();
    assert!(
        !events.iter().any(|e| e.kind == EventKind::ModelUsage),
        "no ModelUsage when neither call has usage"
    );

    let _ = std::fs::remove_dir_all(&workspace);
}

#[test]
fn native_tool_run_fail_turn_emits_no_model_usage_even_if_first_call_had_usage() {
    let workspace = make_temp_workspace("x.txt", "x");
    let client = Arc::new(QueuedFakeClient::new(vec![
        tool_call_resp(
            "read_file",
            "call-hiii1",
            r#"{"path":"x.txt"}"#,
            Some(OpenAIUsage {
                prompt_tokens: 20,
                completion_tokens: 10,
                total_tokens: 30,
            }),
        ),
        // Second call is adapter error -> RunFail -> NO ModelUsage.
        two_tool_calls_resp(),
    ]));
    let gateway = ModelGateway::with_openai_http_client_for_test(
        openai_config(),
        Box::new(Arc::clone(&client)),
    );
    let mut event_log = EventLog::new();
    run_mock_turn(
        &mut event_log,
        "h-iii usage",
        &gateway,
        &workspace,
        None,
        None,
    );

    let events = event_log.events();
    assert!(
        events.iter().any(|e| e.kind == EventKind::RunFail),
        "must RunFail"
    );
    assert!(
        !events.iter().any(|e| e.kind == EventKind::ModelUsage),
        "RunFail path must emit NO ModelUsage"
    );

    let _ = std::fs::remove_dir_all(&workspace);
}

// --- Test (i): dormant text protocol (CARAVAN_TOOL_REQUEST in content, no native tool) ---

#[test]
fn native_tool_dormant_text_protocol_stays_plain_assistant_turn() {
    // The response content contains a CARAVAN_TOOL_REQUEST block but has NO
    // native tool_calls. This must be treated as a plain assistant turn.
    let content_with_block = "preamble\nCARAVAN_TOOL_REQUEST\ntool=read_file\npath=README.md\nEND_CARAVAN_TOOL_REQUEST\npostamble";
    let client = Arc::new(QueuedFakeClient::new(vec![assistant_resp(
        content_with_block,
        None,
    )]));
    let gateway = ModelGateway::with_openai_http_client_for_test(
        openai_config(),
        Box::new(Arc::clone(&client)),
    );
    let mut event_log = EventLog::new();
    let output = run_mock_turn(
        &mut event_log,
        "hello",
        &gateway,
        std::path::Path::new("."),
        None,
        None,
    );

    let events = event_log.events();
    let kinds: Vec<EventKind> = events.iter().map(|e| e.kind).collect();

    // Plain success path: no tool events.
    assert!(!kinds.contains(&EventKind::ToolPolicy), "no ToolPolicy");
    assert!(!kinds.contains(&EventKind::ToolCall), "no ToolCall");
    assert!(!kinds.contains(&EventKind::ToolResult), "no ToolResult");
    assert!(!kinds.contains(&EventKind::ToolError), "no ToolError");
    assert!(
        !kinds.contains(&EventKind::ModelToolRequest),
        "no ModelToolRequest"
    );
    assert!(kinds.contains(&EventKind::RunComplete), "must RunComplete");

    // detected_model_tool_request must be None.
    assert!(
        output.detected_model_tool_request.is_none(),
        "detected_model_tool_request must be None for dormant text protocol"
    );
    // tool_activity must be None.
    assert!(
        output.tool_activity.is_none(),
        "tool_activity must be None for plain assistant turn"
    );

    // Exactly ONE ModelRoute (direct assistant, no round-trip).
    assert_eq!(
        kinds
            .iter()
            .filter(|&&k| k == EventKind::ModelRoute)
            .count(),
        1,
        "exactly one ModelRoute for plain assistant response"
    );
}

// --- Test (j): default Mock gateway (unchanged assistant-only flow) ----------

#[test]
fn native_tool_default_mock_gateway_unchanged_assistant_only_flow() {
    let mut event_log = EventLog::new();
    let gateway = ModelGateway::default();
    let output = run_mock_turn(
        &mut event_log,
        "hello",
        &gateway,
        std::path::Path::new("."),
        None,
        None,
    );

    let events = event_log.events();
    let kinds: Vec<EventKind> = events.iter().map(|e| e.kind).collect();

    // No tool events.
    assert!(!kinds.contains(&EventKind::ToolPolicy), "no ToolPolicy");
    assert!(!kinds.contains(&EventKind::ToolCall), "no ToolCall");
    assert!(!kinds.contains(&EventKind::ToolResult), "no ToolResult");
    assert!(!kinds.contains(&EventKind::ToolError), "no ToolError");
    assert!(
        !kinds.contains(&EventKind::ModelToolRequest),
        "no ModelToolRequest"
    );
    assert!(!kinds.contains(&EventKind::RunFail), "no RunFail");
    assert!(kinds.contains(&EventKind::RunComplete), "must RunComplete");

    // tool_activity is None on the direct-assistant path.
    assert!(
        output.tool_activity.is_none(),
        "tool_activity must be None for Mock gateway"
    );
    assert!(
        output.detected_model_tool_request.is_none(),
        "detected_model_tool_request must be None"
    );

    // Exactly ONE ModelRoute.
    assert_eq!(
        kinds
            .iter()
            .filter(|&&k| k == EventKind::ModelRoute)
            .count(),
        1,
        "exactly one ModelRoute for Mock gateway"
    );
}
