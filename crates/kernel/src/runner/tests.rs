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

    // Fake recorded exactly 2 requests; second carries tools (budget still remains
    // after exec 1 success: exec_count=1 < MAX_NATIVE_TOOL_CALLS_PER_TURN=2).
    assert_eq!(client.request_count(), 2, "exactly 2 HTTP requests");
    assert!(
        client.request_had_tools(0),
        "first request must carry tools"
    );
    assert!(
        client.request_had_tools(1),
        "second request must carry tools (budget remains after exec 1 success)"
    );

    // tool_activities has one entry and it succeeded.
    assert_eq!(
        output.tool_activities.len(),
        1,
        "tool_activities must have one entry"
    );
    let activity = &output.tool_activities[0];
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

    assert_eq!(
        output.tool_activities.len(),
        1,
        "tool_activities must have one entry"
    );
    let activity = &output.tool_activities[0];
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

    // tool_activities reflects the failure.
    assert_eq!(
        output.tool_activities.len(),
        1,
        "tool_activities must have one entry"
    );
    let activity = &output.tool_activities[0];
    assert_eq!(activity.name, "read_file");
    assert!(
        !activity.succeeded,
        "tool_activities[0].succeeded must be false"
    );

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

// --- Test (f): second response is a tool call — SECOND EXECUTION PATH -------
// Migrated from the old second_tool_call_not_supported rejection test.
// The second tool call is now EXECUTED (exec 2), followed by a third model
// call that returns an assistant message (RunComplete).

#[test]
fn native_tool_second_tool_call_executed_as_exec_2_run_completes() {
    let workspace = make_temp_workspace("note.txt", "hi");
    let client = Arc::new(QueuedFakeClient::new(vec![
        tool_call_resp("read_file", "call-f1", r#"{"path":"note.txt"}"#, None),
        // Second response is another tool call — now EXECUTED as exec 2.
        tool_call_resp("list_files", "call-f2", "{}", None),
        // Third response is the assistant answer.
        assistant_resp("Read the file and listed files.", None),
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

    // Three ModelRoute events (first tool call + second tool call + third assistant call).
    assert_eq!(
        kinds
            .iter()
            .filter(|&&k| k == EventKind::ModelRoute)
            .count(),
        3,
        "three ModelRoute events for two-tool-exec path"
    );
    // TWO ToolCall events (both tools executed).
    assert_eq!(
        kinds.iter().filter(|&&k| k == EventKind::ToolCall).count(),
        2,
        "exactly two ToolCall events — both executions ran"
    );
    // TWO ToolResult events.
    assert_eq!(
        kinds
            .iter()
            .filter(|&&k| k == EventKind::ToolResult)
            .count(),
        2,
        "exactly two ToolResult events"
    );
    // Turn must succeed.
    assert!(kinds.contains(&EventKind::RunComplete), "must RunComplete");
    assert!(!kinds.contains(&EventKind::RunFail), "must not RunFail");
    assert!(
        !kinds.contains(&EventKind::ModelError),
        "must not ModelError"
    );

    // Recorded request count is exactly 3.
    assert_eq!(client.request_count(), 3, "exactly 3 HTTP requests");

    // tool_activities has two entries.
    assert_eq!(output.tool_activities.len(), 2, "two tool activities");
    assert_eq!(output.tool_activities[0].name, "read_file");
    assert!(output.tool_activities[0].succeeded);
    assert_eq!(output.tool_activities[1].name, "list_files");
    assert!(output.tool_activities[1].succeeded);

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

    // tool_activities is non-empty even on second-call failure.
    assert!(!output.tool_activities.is_empty());

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
    // tool_activities must be empty.
    assert!(
        output.tool_activities.is_empty(),
        "tool_activities must be empty for plain assistant turn"
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

// --- Test: search_text one-call round-trip -----------------------------------

#[test]
fn native_tool_search_text_one_call_round_trip_event_order() {
    let workspace = make_temp_workspace("notes.txt", "TODO: fix this later");
    let client = Arc::new(QueuedFakeClient::new(vec![
        tool_call_resp("search_text", "call-st1", r#"{"query":"TODO"}"#, None),
        assistant_resp("I found the TODO comment.", None),
    ]));
    let gateway = ModelGateway::with_openai_http_client_for_test(
        openai_config(),
        Box::new(Arc::clone(&client)),
    );
    let mut event_log = EventLog::new();
    let output = run_mock_turn(
        &mut event_log,
        "search for TODO",
        &gateway,
        &workspace,
        None,
        None,
    );

    let events = event_log.events();
    let kinds: Vec<EventKind> = events.iter().map(|e| e.kind).collect();

    // Assert the fixed prefix of the event sequence.
    assert_eq!(kinds[0], EventKind::RunCreate);
    assert_eq!(kinds[1], EventKind::RunStart);
    assert_eq!(kinds[2], EventKind::TurnStart);
    assert_eq!(kinds[3], EventKind::PromptCompile);
    assert_eq!(kinds[4], EventKind::ModelRoute);
    assert_eq!(kinds[5], EventKind::ToolPolicy);
    assert_eq!(kinds[6], EventKind::ToolCall);
    assert_eq!(kinds[7], EventKind::ToolResult);

    // After ToolResult: second ModelRoute, AssistantMessage, RunComplete.
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
    assert_eq!(*kinds.last().unwrap(), EventKind::RunComplete);

    let assistant_msg_idx = kinds
        .iter()
        .position(|&k| k == EventKind::AssistantMessage)
        .unwrap();
    assert!(assistant_msg_idx > second_route_idx);
    assert!(assistant_msg_idx < run_complete_idx);

    // Exactly TWO ModelRoute events.
    assert_eq!(
        kinds
            .iter()
            .filter(|&&k| k == EventKind::ModelRoute)
            .count(),
        2,
        "exactly two ModelRoute events"
    );

    // Exactly ONE ToolCall and ONE ToolResult.
    assert_eq!(
        kinds.iter().filter(|&&k| k == EventKind::ToolCall).count(),
        1,
        "exactly one ToolCall"
    );
    assert_eq!(
        kinds
            .iter()
            .filter(|&&k| k == EventKind::ToolResult)
            .count(),
        1,
        "exactly one ToolResult"
    );

    // No RunFail.
    assert!(
        !kinds.contains(&EventKind::RunFail),
        "no RunFail on success"
    );

    // tool_activities has one entry and it succeeded.
    assert_eq!(
        output.tool_activities.len(),
        1,
        "tool_activities must have one entry"
    );
    let activity = &output.tool_activities[0];
    assert!(activity.succeeded);
    assert_eq!(activity.name, "search_text");

    let _ = std::fs::remove_dir_all(&workspace);
}

// --- Test: search_text → second-tool-call EXECUTED as exec 2 ----------------
// Migrated from the old second_tool_call_not_supported rejection test.
// search_text (exec 1) → list_files (exec 2) → assistant (RunComplete).

#[test]
fn native_tool_search_text_second_tool_call_executed_as_exec_2() {
    let workspace = make_temp_workspace("data.txt", "some content");
    let client = Arc::new(QueuedFakeClient::new(vec![
        tool_call_resp("search_text", "call-st2a", r#"{"query":"content"}"#, None),
        // Second response is another tool call — now EXECUTED as exec 2.
        tool_call_resp("list_files", "call-st2b", "{}", None),
        // Third response is the assistant answer.
        assistant_resp("Found content and listed files.", None),
    ]));
    let gateway = ModelGateway::with_openai_http_client_for_test(
        openai_config(),
        Box::new(Arc::clone(&client)),
    );
    let mut event_log = EventLog::new();
    let output = run_mock_turn(
        &mut event_log,
        "search then list",
        &gateway,
        &workspace,
        None,
        None,
    );

    let events = event_log.events();
    let kinds: Vec<EventKind> = events.iter().map(|e| e.kind).collect();

    // Three ModelRoute events (exec1 + exec2 + final assistant).
    assert_eq!(
        kinds
            .iter()
            .filter(|&&k| k == EventKind::ModelRoute)
            .count(),
        3,
        "three ModelRoute events"
    );

    // TWO ToolCall events (both tools executed).
    assert_eq!(
        kinds.iter().filter(|&&k| k == EventKind::ToolCall).count(),
        2,
        "exactly two ToolCall events — both executions ran"
    );

    // Turn must succeed.
    assert!(kinds.contains(&EventKind::RunComplete), "must RunComplete");
    assert!(!kinds.contains(&EventKind::RunFail), "must not RunFail");
    assert!(
        !kinds.contains(&EventKind::ModelError),
        "must not ModelError"
    );

    // Recorded request count is exactly 3.
    assert_eq!(client.request_count(), 3);

    // tool_activities has two entries from both executions.
    assert_eq!(output.tool_activities.len(), 2);
    assert_eq!(output.tool_activities[0].name, "search_text");
    assert!(output.tool_activities[0].succeeded);
    assert_eq!(output.tool_activities[1].name, "list_files");
    assert!(output.tool_activities[1].succeeded);

    let _ = std::fs::remove_dir_all(&workspace);
}

// ============================================================================
// NEW TESTS — bounded two-tool-exec / three-model-call pipeline (T-3)
// ============================================================================

// --- search_text → read_file → assistant (three-call success) ---------------

#[test]
fn search_text_then_read_file_three_call_success_event_order() {
    let workspace = make_temp_workspace("notes.txt", "TODO: fix this later");
    let client = Arc::new(QueuedFakeClient::new(vec![
        // Step 1: model calls search_text (exec 1).
        tool_call_resp(
            "search_text",
            "call-s1",
            r#"{"query":"TODO"}"#,
            Some(OpenAIUsage {
                prompt_tokens: 10,
                completion_tokens: 3,
                total_tokens: 13,
            }),
        ),
        // Step 2: model calls read_file (exec 2, tools re-offered after exec 1 success).
        tool_call_resp(
            "read_file",
            "call-s2",
            r#"{"path":"notes.txt"}"#,
            Some(OpenAIUsage {
                prompt_tokens: 20,
                completion_tokens: 4,
                total_tokens: 24,
            }),
        ),
        // Step 3: model returns assistant answer (no tools offered, budget exhausted).
        assistant_resp(
            "Found TODO and read the file.",
            Some(OpenAIUsage {
                prompt_tokens: 30,
                completion_tokens: 8,
                total_tokens: 38,
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
        "search then read",
        &gateway,
        &workspace,
        None,
        None,
    );

    let events = event_log.events();
    let kinds: Vec<EventKind> = events.iter().map(|e| e.kind).collect();

    // Fixed prefix: RunCreate, RunStart, TurnStart, PromptCompile.
    assert_eq!(kinds[0], EventKind::RunCreate);
    assert_eq!(kinds[1], EventKind::RunStart);
    assert_eq!(kinds[2], EventKind::TurnStart);
    assert_eq!(kinds[3], EventKind::PromptCompile);

    // Three ModelRoute events (one per model call).
    assert_eq!(
        kinds
            .iter()
            .filter(|&&k| k == EventKind::ModelRoute)
            .count(),
        3,
        "exactly three ModelRoute events"
    );

    // Two ToolPolicy / ToolCall / ToolResult groups.
    assert_eq!(
        kinds
            .iter()
            .filter(|&&k| k == EventKind::ToolPolicy)
            .count(),
        2,
        "exactly two ToolPolicy events"
    );
    assert_eq!(
        kinds.iter().filter(|&&k| k == EventKind::ToolCall).count(),
        2,
        "exactly two ToolCall events"
    );
    assert_eq!(
        kinds
            .iter()
            .filter(|&&k| k == EventKind::ToolResult)
            .count(),
        2,
        "exactly two ToolResult events"
    );

    // Exactly ONE PromptCompile (base prompt compiled once per turn).
    assert_eq!(
        kinds
            .iter()
            .filter(|&&k| k == EventKind::PromptCompile)
            .count(),
        1,
        "exactly one PromptCompile"
    );

    // Turn must succeed.
    assert!(
        kinds.contains(&EventKind::AssistantMessage),
        "must have AssistantMessage"
    );
    assert!(kinds.contains(&EventKind::RunComplete), "must RunComplete");
    assert!(!kinds.contains(&EventKind::RunFail), "must not RunFail");
    assert!(
        !kinds.contains(&EventKind::ModelError),
        "must not ModelError"
    );

    // Usage aggregated: 10+20+30=60, 3+4+8=15, 13+24+38=75.
    let usage_event = events
        .iter()
        .find(|e| e.kind == EventKind::ModelUsage)
        .expect("ModelUsage");
    assert_eq!(
        usage_event.detail,
        "prompt_tokens=60 completion_tokens=15 total_tokens=75"
    );

    // Request tools policy: [true, true, false].
    assert_eq!(client.request_count(), 3, "exactly 3 HTTP requests");
    assert!(client.request_had_tools(0), "step 1 must carry tools");
    assert!(
        client.request_had_tools(1),
        "step 2 must carry tools (budget remains)"
    );
    assert!(
        !client.request_had_tools(2),
        "step 3 must NOT carry tools (budget exhausted)"
    );

    // tool_activities: [search_text, read_file] both succeeded.
    assert_eq!(output.tool_activities.len(), 2);
    assert_eq!(output.tool_activities[0].name, "search_text");
    assert!(output.tool_activities[0].succeeded);
    assert_eq!(output.tool_activities[1].name, "read_file");
    assert!(output.tool_activities[1].succeeded);

    let _ = std::fs::remove_dir_all(&workspace);
}

// --- list_files → read_file → assistant (three-call success) ----------------

#[test]
fn list_files_then_read_file_three_call_success_event_order() {
    let workspace = make_temp_workspace("info.txt", "project info");
    let client = Arc::new(QueuedFakeClient::new(vec![
        // Step 1: model calls list_files (exec 1).
        tool_call_resp("list_files", "call-l1", "{}", None),
        // Step 2: model calls read_file (exec 2).
        tool_call_resp("read_file", "call-l2", r#"{"path":"info.txt"}"#, None),
        // Step 3: model returns assistant answer.
        assistant_resp("Listed and read the file.", None),
    ]));
    let gateway = ModelGateway::with_openai_http_client_for_test(
        openai_config(),
        Box::new(Arc::clone(&client)),
    );
    let mut event_log = EventLog::new();
    let output = run_mock_turn(
        &mut event_log,
        "list then read",
        &gateway,
        &workspace,
        None,
        None,
    );

    let events = event_log.events();
    let kinds: Vec<EventKind> = events.iter().map(|e| e.kind).collect();

    // Three ModelRoute events.
    assert_eq!(
        kinds
            .iter()
            .filter(|&&k| k == EventKind::ModelRoute)
            .count(),
        3,
        "exactly three ModelRoute events"
    );

    // Two ToolCall events (both executions ran).
    assert_eq!(
        kinds.iter().filter(|&&k| k == EventKind::ToolCall).count(),
        2,
        "exactly two ToolCall events"
    );

    // Turn must succeed.
    assert!(kinds.contains(&EventKind::RunComplete), "must RunComplete");
    assert!(!kinds.contains(&EventKind::RunFail), "must not RunFail");

    // tool_activities: [list_files, read_file] both succeeded.
    assert_eq!(output.tool_activities.len(), 2);
    assert_eq!(output.tool_activities[0].name, "list_files");
    assert!(output.tool_activities[0].succeeded);
    assert_eq!(output.tool_activities[1].name, "read_file");
    assert!(output.tool_activities[1].succeeded);

    assert_eq!(client.request_count(), 3, "exactly 3 HTTP requests");

    let _ = std::fs::remove_dir_all(&workspace);
}

// --- Third-response tool call rejected with ModelError + RunFail -------------

#[test]
fn third_tool_call_rejected_with_model_error_and_run_fail() {
    let workspace = make_temp_workspace("a.txt", "aaa");
    let client = Arc::new(QueuedFakeClient::new(vec![
        // Step 1: exec 1.
        tool_call_resp("read_file", "call-t1", r#"{"path":"a.txt"}"#, None),
        // Step 2: exec 2.
        tool_call_resp("list_files", "call-t2", "{}", None),
        // Step 3: model STILL returns a tool call — must be rejected.
        tool_call_resp("search_text", "call-t3", r#"{"query":"aaa"}"#, None),
    ]));
    let gateway = ModelGateway::with_openai_http_client_for_test(
        openai_config(),
        Box::new(Arc::clone(&client)),
    );
    let mut event_log = EventLog::new();
    let output = run_mock_turn(
        &mut event_log,
        "three tool calls",
        &gateway,
        &workspace,
        None,
        None,
    );

    let events = event_log.events();
    let kinds: Vec<EventKind> = events.iter().map(|e| e.kind).collect();

    // Three ModelRoute events (one per model step; third is emitted before rejection).
    assert_eq!(
        kinds
            .iter()
            .filter(|&&k| k == EventKind::ModelRoute)
            .count(),
        3,
        "three ModelRoute events including the rejected third"
    );

    // Only TWO ToolCall events — the third model step's tool call is NOT executed.
    assert_eq!(
        kinds.iter().filter(|&&k| k == EventKind::ToolCall).count(),
        2,
        "exactly two ToolCall events — third tool call not executed"
    );

    // Must RunFail with ModelError containing the sentinel.
    assert!(
        kinds.contains(&EventKind::ModelError),
        "must have ModelError"
    );
    assert!(kinds.contains(&EventKind::RunFail), "must have RunFail");
    assert!(
        !kinds.contains(&EventKind::RunComplete),
        "must not RunComplete"
    );

    let model_error_event = events
        .iter()
        .find(|e| e.kind == EventKind::ModelError)
        .unwrap();
    assert!(
        model_error_event
            .detail
            .contains("third_tool_call_not_supported"),
        "ModelError must contain 'third_tool_call_not_supported': {}",
        model_error_event.detail
    );

    // tool_activities must contain both executed tool activities.
    assert_eq!(
        output.tool_activities.len(),
        2,
        "two tool activities before rejection"
    );
    assert_eq!(output.tool_activities[0].name, "read_file");
    assert_eq!(output.tool_activities[1].name, "list_files");

    let _ = std::fs::remove_dir_all(&workspace);
}

// --- Usage aggregation across three model responses --------------------------

#[test]
fn usage_aggregation_three_model_responses_sums_all_fields() {
    // usage1(prompt=10, completion=5, total=15)
    // + usage2(prompt=20, completion=6, total=26)
    // + usage3(prompt=30, completion=7, total=37)
    // = prompt=60, completion=18, total=78 (each field summed independently).
    let workspace = make_temp_workspace("b.txt", "b");
    let client = Arc::new(QueuedFakeClient::new(vec![
        tool_call_resp(
            "read_file",
            "call-u1",
            r#"{"path":"b.txt"}"#,
            Some(OpenAIUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            }),
        ),
        tool_call_resp(
            "list_files",
            "call-u2",
            "{}",
            Some(OpenAIUsage {
                prompt_tokens: 20,
                completion_tokens: 6,
                total_tokens: 26,
            }),
        ),
        assistant_resp(
            "Done.",
            Some(OpenAIUsage {
                prompt_tokens: 30,
                completion_tokens: 7,
                total_tokens: 37,
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
        "aggregate three",
        &gateway,
        &workspace,
        None,
        None,
    );

    let events = event_log.events();

    // RunComplete must be present.
    assert!(events.iter().any(|e| e.kind == EventKind::RunComplete));

    // Exactly ONE ModelUsage event.
    let usage_events: Vec<_> = events
        .iter()
        .filter(|e| e.kind == EventKind::ModelUsage)
        .collect();
    assert_eq!(usage_events.len(), 1, "exactly one ModelUsage");

    assert_eq!(
        usage_events[0].detail, "prompt_tokens=60 completion_tokens=18 total_tokens=78",
        "usage must be independently summed across all three model responses"
    );

    let _ = std::fs::remove_dir_all(&workspace);
}

// --- Tools re-offered on second request when budget remains, not after -------

#[test]
fn tools_offered_on_second_request_when_budget_remains() {
    // exec1=read_file(success) → second request MUST carry tools (budget=1<2)
    // exec2=list_files(success) → third request MUST NOT carry tools (budget=2=2)
    let workspace = make_temp_workspace("c.txt", "c");
    let client = Arc::new(QueuedFakeClient::new(vec![
        tool_call_resp("read_file", "call-b1", r#"{"path":"c.txt"}"#, None),
        tool_call_resp("list_files", "call-b2", "{}", None),
        assistant_resp("Done.", None),
    ]));
    let gateway = ModelGateway::with_openai_http_client_for_test(
        openai_config(),
        Box::new(Arc::clone(&client)),
    );
    let mut event_log = EventLog::new();
    run_mock_turn(
        &mut event_log,
        "tools policy check",
        &gateway,
        &workspace,
        None,
        None,
    );

    assert_eq!(client.request_count(), 3, "exactly 3 HTTP requests");
    // Request 0: first model call — always carries tool definitions.
    assert!(client.request_had_tools(0), "request 0 must carry tools");
    // Request 1: second model call — carries tools (exec 1 succeeded, budget remains).
    assert!(
        client.request_had_tools(1),
        "request 1 must carry tools (budget remains after successful exec 1)"
    );
    // Request 2: third model call — NO tools (budget exhausted after exec 2).
    assert!(
        !client.request_had_tools(2),
        "request 2 must NOT carry tools (budget exhausted)"
    );

    let _ = std::fs::remove_dir_all(&workspace);
}

// --- No ModelUsage emitted on RunFail path (three tool-call responses) -------

#[test]
fn third_tool_call_no_usage_on_run_fail() {
    // Three tool call responses → RunFail at the third step.
    // ModelUsage MUST NOT be emitted even though steps 1 and 2 carried usage.
    let workspace = make_temp_workspace("d.txt", "d");
    let client = Arc::new(QueuedFakeClient::new(vec![
        tool_call_resp(
            "read_file",
            "call-nu1",
            r#"{"path":"d.txt"}"#,
            Some(OpenAIUsage {
                prompt_tokens: 10,
                completion_tokens: 2,
                total_tokens: 12,
            }),
        ),
        tool_call_resp(
            "list_files",
            "call-nu2",
            "{}",
            Some(OpenAIUsage {
                prompt_tokens: 15,
                completion_tokens: 3,
                total_tokens: 18,
            }),
        ),
        // Third step returns a tool call → RunFail, no usage emitted.
        tool_call_resp("search_text", "call-nu3", r#"{"query":"d"}"#, None),
    ]));
    let gateway = ModelGateway::with_openai_http_client_for_test(
        openai_config(),
        Box::new(Arc::clone(&client)),
    );
    let mut event_log = EventLog::new();
    run_mock_turn(
        &mut event_log,
        "three tool calls no usage",
        &gateway,
        &workspace,
        None,
        None,
    );

    let events = event_log.events();

    // Must RunFail.
    assert!(
        events.iter().any(|e| e.kind == EventKind::RunFail),
        "must RunFail"
    );
    assert!(
        !events.iter().any(|e| e.kind == EventKind::RunComplete),
        "must not RunComplete"
    );

    // NO ModelUsage event on the RunFail path.
    assert!(
        !events.iter().any(|e| e.kind == EventKind::ModelUsage),
        "RunFail path must emit NO ModelUsage even though earlier calls had usage"
    );

    let _ = std::fs::remove_dir_all(&workspace);
}

// --- ToolResult event detail is summary-only (full content not in EventLog) --

#[test]
fn tool_result_detail_is_summary_only() {
    // Write a file whose content contains a distinctive sentinel that MUST NOT
    // appear in any ToolResult event's detail field.
    let sentinel = "SENTINEL_CONTENT_XYZZY_CARAVAN_T3_TEST_MARKER_12345";
    let workspace = make_temp_workspace("secret.txt", sentinel);

    let client = Arc::new(QueuedFakeClient::new(vec![
        // Exec 1: read the file containing the sentinel.
        tool_call_resp("read_file", "call-sr1", r#"{"path":"secret.txt"}"#, None),
        // Exec 2: list files (to exercise two-tool path, ensuring both ToolResult
        // events are checked).
        tool_call_resp("list_files", "call-sr2", "{}", None),
        assistant_resp("I can see the workspace.", None),
    ]));
    let gateway = ModelGateway::with_openai_http_client_for_test(
        openai_config(),
        Box::new(Arc::clone(&client)),
    );
    let mut event_log = EventLog::new();
    run_mock_turn(
        &mut event_log,
        "read secret file",
        &gateway,
        &workspace,
        None,
        None,
    );

    let events = event_log.events();

    // The turn must have succeeded (RunComplete) to ensure ToolResult events exist.
    assert!(
        events.iter().any(|e| e.kind == EventKind::RunComplete),
        "must RunComplete so ToolResult events are present"
    );

    // Assert that no ToolResult event's detail contains the sentinel string.
    for event in events.iter().filter(|e| e.kind == EventKind::ToolResult) {
        assert!(
            !event.detail.contains(sentinel),
            "ToolResult event detail must not contain full file content (sentinel found): {:?}",
            event.detail
        );
    }

    // Also assert no other event type leaks the sentinel through the event log.
    for event in events.iter() {
        if event.kind == EventKind::ToolResult
            || event.kind == EventKind::ModelOutputChunk
            || event.kind == EventKind::AssistantMessage
        {
            // ModelOutputChunk and AssistantMessage may theoretically include the
            // sentinel in a real run if the model echoed it back, but with a fake
            // client that returns fixed content, they won't. We skip these to avoid
            // coupling the test to fake-client response content.
            continue;
        }
        assert!(
            !event.detail.contains(sentinel),
            "Event {:?} must not contain full file content (sentinel found): {:?}",
            event.kind,
            event.detail
        );
    }

    let _ = std::fs::remove_dir_all(&workspace);
}

// --- Single read_file still works (single-tool path not regressed) -----------

#[test]
fn native_tool_single_read_file_still_works_after_bounded_pipeline() {
    let workspace = make_temp_workspace("single.txt", "single content");
    let client = Arc::new(QueuedFakeClient::new(vec![
        tool_call_resp("read_file", "call-sg1", r#"{"path":"single.txt"}"#, None),
        assistant_resp("Read the file.", None),
    ]));
    let gateway = ModelGateway::with_openai_http_client_for_test(
        openai_config(),
        Box::new(Arc::clone(&client)),
    );
    let mut event_log = EventLog::new();
    let output = run_mock_turn(
        &mut event_log,
        "read single",
        &gateway,
        &workspace,
        None,
        None,
    );

    let events = event_log.events();
    let kinds: Vec<EventKind> = events.iter().map(|e| e.kind).collect();

    // Two ModelRoute events (exec 1 + final assistant).
    assert_eq!(
        kinds
            .iter()
            .filter(|&&k| k == EventKind::ModelRoute)
            .count(),
        2,
        "two ModelRoute events for single-tool path"
    );
    // ONE ToolCall, ONE ToolResult.
    assert_eq!(
        kinds.iter().filter(|&&k| k == EventKind::ToolCall).count(),
        1
    );
    assert_eq!(
        kinds
            .iter()
            .filter(|&&k| k == EventKind::ToolResult)
            .count(),
        1
    );
    assert!(kinds.contains(&EventKind::RunComplete), "must RunComplete");
    assert!(!kinds.contains(&EventKind::RunFail), "must not RunFail");

    assert_eq!(output.tool_activities.len(), 1);

    // Two HTTP requests; second carries tools (budget remains after exec 1 success).
    assert_eq!(client.request_count(), 2);
    assert!(client.request_had_tools(0), "request 0 must carry tools");
    assert!(
        client.request_had_tools(1),
        "request 1 must carry tools (budget remains)"
    );

    let _ = std::fs::remove_dir_all(&workspace);
}

// --- First-tool ToolError: second call offers NO tools (model explains) ------

#[test]
fn native_tool_first_exec_error_second_request_carries_no_tools() {
    // read_file on a missing file → ToolError → second call (assistant) offered NO tools.
    let workspace = make_temp_workspace("", "");
    let client = Arc::new(QueuedFakeClient::new(vec![
        tool_call_resp(
            "read_file",
            "call-e1",
            r#"{"path":"missing_file_xyz.txt"}"#,
            None,
        ),
        assistant_resp("The file was not found.", None),
    ]));
    let gateway = ModelGateway::with_openai_http_client_for_test(
        openai_config(),
        Box::new(Arc::clone(&client)),
    );
    let mut event_log = EventLog::new();
    let output = run_mock_turn(
        &mut event_log,
        "read missing",
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
        "must NOT have ToolResult"
    );

    // Turn still reaches RunComplete (model explains the error).
    assert!(kinds.contains(&EventKind::RunComplete), "must RunComplete");
    assert!(!kinds.contains(&EventKind::RunFail), "must not RunFail");

    // Two HTTP requests; second must NOT carry tools (tool error → no re-offer).
    assert_eq!(client.request_count(), 2);
    assert!(client.request_had_tools(0), "request 0 must carry tools");
    assert!(
        !client.request_had_tools(1),
        "request 1 must NOT carry tools (after tool error)"
    );

    // tool_activities: one entry, succeeded=false.
    assert_eq!(output.tool_activities.len(), 1);
    assert!(!output.tool_activities[0].succeeded);

    let _ = std::fs::remove_dir_all(&workspace);
}

// --- Two-tool success does not stage Workspace Context (no ToolContextAttach) -

#[test]
fn native_tool_two_tool_turn_does_not_stage_workspace_context() {
    let workspace = make_temp_workspace("e.txt", "e");
    let client = Arc::new(QueuedFakeClient::new(vec![
        tool_call_resp("read_file", "call-wc1", r#"{"path":"e.txt"}"#, None),
        tool_call_resp("list_files", "call-wc2", "{}", None),
        assistant_resp("Done.", None),
    ]));
    let gateway = ModelGateway::with_openai_http_client_for_test(
        openai_config(),
        Box::new(Arc::clone(&client)),
    );
    let mut event_log = EventLog::new();
    run_mock_turn(
        &mut event_log,
        "two tool no context",
        &gateway,
        &workspace,
        None,
        None,
    );

    let events = event_log.events();

    // ToolContextAttach must NOT be emitted by the native tool flow.
    assert!(
        !events
            .iter()
            .any(|e| e.kind == EventKind::ToolContextAttach),
        "native tool flow must not emit ToolContextAttach"
    );

    // Must still succeed.
    assert!(events.iter().any(|e| e.kind == EventKind::RunComplete));

    let _ = std::fs::remove_dir_all(&workspace);
}

// --- Adapter-level multi-tool in second response still rejected --------------

#[test]
fn native_tool_two_tool_calls_in_second_response_is_adapter_error() {
    // The SECOND response has multiple tool calls in a single response
    // (rejected at the adapter level before we even see a ToolCall).
    // This should cause a RunFail without ModelRoute for the second call.
    let workspace = make_temp_workspace("f.txt", "f");
    let client = Arc::new(QueuedFakeClient::new(vec![
        tool_call_resp("read_file", "call-ae1", r#"{"path":"f.txt"}"#, None),
        // Multiple tool calls in one response → adapter error.
        two_tool_calls_resp(),
    ]));
    let gateway = ModelGateway::with_openai_http_client_for_test(
        openai_config(),
        Box::new(Arc::clone(&client)),
    );
    let mut event_log = EventLog::new();
    run_mock_turn(
        &mut event_log,
        "adapter error on second",
        &gateway,
        &workspace,
        None,
        None,
    );

    let events = event_log.events();
    let kinds: Vec<EventKind> = events.iter().map(|e| e.kind).collect();

    // Only ONE ModelRoute (from exec 1); second call returns adapter error.
    assert_eq!(
        kinds
            .iter()
            .filter(|&&k| k == EventKind::ModelRoute)
            .count(),
        1,
        "only one ModelRoute — second call adapter error"
    );
    assert!(
        kinds.contains(&EventKind::ToolResult),
        "exec 1 must have ToolResult"
    );
    assert!(
        kinds.contains(&EventKind::ModelError),
        "must have ModelError"
    );
    assert!(kinds.contains(&EventKind::RunFail), "must have RunFail");
    assert!(
        !kinds.contains(&EventKind::RunComplete),
        "must not RunComplete"
    );

    let _ = std::fs::remove_dir_all(&workspace);
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

    // tool_activities is empty on the direct-assistant path.
    assert!(
        output.tool_activities.is_empty(),
        "tool_activities must be empty for Mock gateway"
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

// ============================================================================
// @-reference resolution tests — parse_workspace_references /
// resolve_workspace_references wired into run_mock_turn (T-3).
// ============================================================================

// --- @reference resolves into PromptCompile; raw message stays byte-identical ---

#[test]
fn reference_to_existing_file_renders_workspace_context_and_preserves_raw_message() {
    let sentinel = "SENTINEL_WORKSPACE_REFERENCE_CONTENT_T3_9F2C";
    let workspace = make_temp_workspace("doc.txt", sentinel);

    let mut event_log = EventLog::new();
    let gateway = ModelGateway::default();
    let raw_message = "@doc.txt explain";
    let output = run_mock_turn(
        &mut event_log,
        raw_message,
        &gateway,
        &workspace,
        None,
        None,
    );

    let events = event_log.events();
    let pc = events
        .iter()
        .find(|e| e.kind == EventKind::PromptCompile)
        .expect("PromptCompile event should exist");

    assert!(
        pc.detail.contains("Referenced Workspace Context:"),
        "PromptCompile detail should contain 'Referenced Workspace Context:': {}",
        pc.detail
    );
    assert!(
        pc.detail.contains("   1 | "),
        "PromptCompile detail should contain a line-numbered content line: {}",
        pc.detail
    );

    // Slice the compiled prompt into its Conversation:/Current User:/Workspace
    // Context: sections so raw-message and sentinel placement can be checked
    // precisely.
    let conversation_idx = pc
        .detail
        .find("Conversation:\n")
        .expect("missing Conversation: section");
    let current_user_idx = pc
        .detail
        .find("Current User:\n")
        .expect("missing Current User: section");
    let workspace_context_idx = pc
        .detail
        .find("Workspace Context:\n")
        .expect("missing Workspace Context: section");
    let conversation_slice = &pc.detail[conversation_idx..current_user_idx];
    let current_user_slice = &pc.detail[current_user_idx..workspace_context_idx];
    let workspace_context_slice = &pc.detail[workspace_context_idx..];

    // The exact raw `@doc.txt` token must appear verbatim in Current User:.
    assert!(
        current_user_slice.contains(raw_message),
        "Current User: section must contain the raw message verbatim: {}",
        current_user_slice
    );

    // The sentinel must appear ONLY in the Workspace Context: slice.
    assert!(
        !conversation_slice.contains(sentinel),
        "sentinel must not leak into Conversation:"
    );
    assert!(
        !current_user_slice.contains(sentinel),
        "sentinel must not leak into Current User:"
    );
    assert!(
        workspace_context_slice.contains(sentinel),
        "sentinel must appear in Workspace Context:"
    );

    // The runner must never rewrite the raw message.
    assert_eq!(output.user_message, raw_message);

    let _ = std::fs::remove_dir_all(&workspace);
}

// --- @reference resolution never touches ToolEventRunner / emits Tool* events ---

#[test]
fn reference_emits_no_tool_events() {
    let workspace = make_temp_workspace("doc2.txt", "reference content");
    let mut event_log = EventLog::new();
    let gateway = ModelGateway::default();
    run_mock_turn(
        &mut event_log,
        "@doc2.txt summarize",
        &gateway,
        &workspace,
        None,
        None,
    );

    let events = event_log.events();
    let tool_events: Vec<EventKind> = events
        .iter()
        .filter(|e| {
            matches!(
                e.kind,
                EventKind::ToolPolicy
                    | EventKind::ToolCall
                    | EventKind::ToolResult
                    | EventKind::ToolError
                    | EventKind::ToolContextAttach
            )
        })
        .map(|e| e.kind)
        .collect();
    assert!(
        tool_events.is_empty(),
        "reference resolution must not emit any Tool* event, found: {:?}",
        tool_events
    );

    let _ = std::fs::remove_dir_all(&workspace);
}

// --- @reference to a missing path still completes the run --------------------

#[test]
fn reference_to_missing_path_still_completes_run_with_not_ok_summary() {
    let workspace = make_temp_workspace("", "");
    let mut event_log = EventLog::new();
    let gateway = ModelGateway::default();
    let output = run_mock_turn(
        &mut event_log,
        "@does_not_exist.txt summarize",
        &gateway,
        &workspace,
        None,
        None,
    );

    let events = event_log.events();
    assert!(
        events.iter().any(|e| e.kind == EventKind::RunComplete),
        "a missing @reference must not prevent the run from completing"
    );
    assert!(
        !events.iter().any(|e| e.kind == EventKind::RunFail),
        "a missing @reference alone must not cause RunFail"
    );

    assert_eq!(
        output.workspace_references.len(),
        1,
        "expected exactly one workspace_references summary entry"
    );
    assert!(
        !output.workspace_references[0].ok,
        "a missing path must summarize as ok == false"
    );
    assert_eq!(output.workspace_references[0].raw, "does_not_exist.txt");

    let _ = std::fs::remove_dir_all(&workspace);
}

// ============================================================================
// @reference line-range integration tests (T-2): run_mock_turn wiring for
// @file:N-M / @file#LN-LM range suffixes.
// ============================================================================

/// Builds `count` numbered lines (`line1\n`, `line2\n`, ...) for a temp
/// workspace file large enough to exercise a `10-15` range read.
fn numbered_lines(count: usize) -> String {
    (1..=count).map(|n| format!("line{n}\n")).collect()
}

// --- @file:N-M range resolves into Referenced Workspace Context only -------

#[test]
fn colon_range_reference_renders_range_and_numbered_snippet_isolated_to_workspace_context() {
    let workspace = make_temp_workspace("doc.txt", &numbered_lines(20));

    let mut event_log = EventLog::new();
    let gateway = ModelGateway::default();
    let raw_message = "@doc.txt:10-15 explain";
    // The app appends the UserMessage before the runner runs; mirror that so
    // the event's detail can be checked against the raw message.
    event_log.append(EventKind::UserMessage, raw_message);
    let output = run_mock_turn(
        &mut event_log,
        raw_message,
        &gateway,
        &workspace,
        None,
        None,
    );

    let events = event_log.events();

    let user_message_event = events
        .iter()
        .find(|e| e.kind == EventKind::UserMessage)
        .expect("UserMessage event should exist");
    assert_eq!(user_message_event.detail, raw_message);
    assert_eq!(output.user_message, raw_message);

    let pc = events
        .iter()
        .find(|e| e.kind == EventKind::PromptCompile)
        .expect("PromptCompile event should exist");

    assert!(
        pc.detail.contains("Referenced Workspace Context:"),
        "PromptCompile detail should contain 'Referenced Workspace Context:': {}",
        pc.detail
    );
    assert!(
        pc.detail.contains("range: 10-15"),
        "PromptCompile detail should contain a 'range: 10-15' line: {}",
        pc.detail
    );
    assert!(
        pc.detail.contains("  10 | "),
        "PromptCompile detail should contain a numbered snippet line starting at 10: {}",
        pc.detail
    );

    // Slice the compiled prompt into its Conversation:/Current User:/Workspace
    // Context: sections so the range/snippet content's placement can be
    // checked precisely.
    let conversation_idx = pc
        .detail
        .find("Conversation:\n")
        .expect("missing Conversation: section");
    let current_user_idx = pc
        .detail
        .find("Current User:\n")
        .expect("missing Current User: section");
    let workspace_context_idx = pc
        .detail
        .find("Workspace Context:\n")
        .expect("missing Workspace Context: section");
    let conversation_slice = &pc.detail[conversation_idx..current_user_idx];
    let current_user_slice = &pc.detail[current_user_idx..workspace_context_idx];
    let workspace_context_slice = &pc.detail[workspace_context_idx..];

    // The exact raw `@doc.txt:10-15` token must appear verbatim in Current User:.
    assert!(
        current_user_slice.contains(raw_message),
        "Current User: section must contain the raw message verbatim: {}",
        current_user_slice
    );

    for marker in ["range: 10-15", "  10 | "] {
        assert!(
            !conversation_slice.contains(marker),
            "range/snippet marker {marker:?} must not leak into Conversation:"
        );
        assert!(
            !current_user_slice.contains(marker),
            "range/snippet marker {marker:?} must not leak into Current User:"
        );
        assert!(
            workspace_context_slice.contains(marker),
            "range/snippet marker {marker:?} must appear in Workspace Context:"
        );
    }

    // No Tool* event of any kind is appended for a range reference, and the
    // native tool budget (observable via `output.tool_activities`) is left
    // completely untouched.
    let tool_events: Vec<EventKind> = events
        .iter()
        .filter(|e| {
            matches!(
                e.kind,
                EventKind::ToolPolicy
                    | EventKind::ToolCall
                    | EventKind::ToolResult
                    | EventKind::ToolError
                    | EventKind::ToolContextAttach
            )
        })
        .map(|e| e.kind)
        .collect();
    assert!(
        tool_events.is_empty(),
        "range reference resolution must not emit any Tool* event, found: {:?}",
        tool_events
    );
    assert!(
        output.tool_activities.is_empty(),
        "range reference resolution must leave the native tool budget untouched"
    );

    let _ = std::fs::remove_dir_all(&workspace);
}

// --- @file#LN-LM (GitHub-style) range starts the numbered snippet at N -----

#[test]
fn github_style_range_reference_numbered_snippet_starts_at_range_start() {
    let workspace = make_temp_workspace("doc.txt", &numbered_lines(20));

    let mut event_log = EventLog::new();
    let gateway = ModelGateway::default();
    let output = run_mock_turn(
        &mut event_log,
        "@doc.txt#L10-L15 explain",
        &gateway,
        &workspace,
        None,
        None,
    );

    let events = event_log.events();
    let pc = events
        .iter()
        .find(|e| e.kind == EventKind::PromptCompile)
        .expect("PromptCompile event should exist");

    assert!(
        pc.detail.contains("range: 10-15"),
        "PromptCompile detail should contain a 'range: 10-15' line: {}",
        pc.detail
    );
    assert!(
        pc.detail.contains("  10 | "),
        "numbered snippet must start at line 10: {}",
        pc.detail
    );
    assert!(
        !pc.detail.contains("   1 | "),
        "numbered snippet must not start at line 1 for a ranged reference: {}",
        pc.detail
    );

    let _ = output;
    let _ = std::fs::remove_dir_all(&workspace);
}

// --- a second turn without an @range reference does not reuse the prior ---
// --- range content in its PromptCompile ------------------------------------

#[test]
fn second_turn_without_range_reference_does_not_reuse_prior_range_content() {
    let workspace = make_temp_workspace("doc.txt", &numbered_lines(20));

    let mut event_log = EventLog::new();
    let gateway = ModelGateway::default();

    event_log.append(EventKind::UserMessage, "@doc.txt:10-15 explain");
    run_mock_turn(
        &mut event_log,
        "@doc.txt:10-15 explain",
        &gateway,
        &workspace,
        None,
        None,
    );

    event_log.append(EventKind::UserMessage, "no reference this time");
    run_mock_turn(
        &mut event_log,
        "no reference this time",
        &gateway,
        &workspace,
        None,
        None,
    );

    let events = event_log.events();
    let second_pc = events
        .iter()
        .filter(|e| e.kind == EventKind::PromptCompile)
        .nth(1)
        .expect("second PromptCompile event should exist");

    assert!(
        !second_pc.detail.contains("Referenced Workspace Context:"),
        "second PromptCompile must not reintroduce Referenced Workspace Context: {}",
        second_pc.detail
    );
    assert!(
        !second_pc.detail.contains("range: 10-15"),
        "second PromptCompile must not reuse the prior turn's range: {}",
        second_pc.detail
    );
    assert!(
        !second_pc.detail.contains("  10 | "),
        "second PromptCompile must not reuse the prior turn's numbered snippet: {}",
        second_pc.detail
    );

    let _ = std::fs::remove_dir_all(&workspace);
}
