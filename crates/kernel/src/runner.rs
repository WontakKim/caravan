use crate::events::{EventKind, EventLog, RunId, TurnId};
use crate::manual_context::ManualToolContext;
use crate::model::ModelRequest;
use crate::model_gateway::ModelGateway;

pub struct MockRunOutput {
    pub user_message: String,
    pub assistant_response: String,
    pub run_id: String,
    pub turn_id: String,
}

pub fn run_mock_turn(
    event_log: &mut EventLog,
    message: &str,
    gateway: &ModelGateway,
    manual_tool_context: Option<&ManualToolContext>,
) -> MockRunOutput {
    let run_id = RunId(format!("run-{}", event_log.next_seq_value()));
    event_log.append(
        EventKind::RunCreate,
        format!("run_id={} input=\"{}\"", run_id, message),
    );
    event_log.append(EventKind::RunStart, format!("run_id={}", run_id));
    let turn_id = TurnId(format!("turn-{}", event_log.next_seq_value()));
    event_log.append(
        EventKind::TurnStart,
        format!("run_id={} turn_id={}", run_id, turn_id),
    );
    // Project the recent conversation history and drop the current (trailing)
    // user message, which the app appended before this runner ran. The
    // projection clones into an owned transcript, so no borrow of `event_log`
    // survives the subsequent PromptCompile append.
    let transcript = crate::transcript::ConversationTranscript::from_event_log(event_log);
    let history = transcript.without_trailing_user_message();
    let prompt = crate::prompt::compile_prompt_with_context(message, history, manual_tool_context);
    event_log.append(EventKind::PromptCompile, prompt.clone());
    let request = ModelRequest {
        prompt,
        user_message: message.to_string(),
    };
    match gateway.complete(request) {
        Ok(response) => {
            event_log.append(EventKind::ModelRoute, response.route.detail());
            for chunk in &response.chunks {
                event_log.append(
                    EventKind::ModelOutputChunk,
                    format!("run_id={} turn_id={} text=\"{}\"", run_id, turn_id, chunk),
                );
            }
            event_log.append(
                EventKind::AssistantMessage,
                response.assistant_response.clone(),
            );
            if let Some(usage) = response.usage {
                event_log.append(
                    EventKind::ModelUsage,
                    format!(
                        "prompt_tokens={} completion_tokens={} total_tokens={}",
                        usage.prompt_tokens, usage.completion_tokens, usage.total_tokens
                    ),
                );
            }
            event_log.append(
                EventKind::RunComplete,
                format!("run_id={} outcome=ok", run_id),
            );
            MockRunOutput {
                user_message: message.to_string(),
                assistant_response: response.assistant_response,
                run_id: run_id.to_string(),
                turn_id: turn_id.to_string(),
            }
        }
        Err(err) => {
            event_log.append(EventKind::ModelError, err.to_string());
            event_log.append(
                EventKind::RunFail,
                format!("run_id={} outcome=error", run_id),
            );
            MockRunOutput {
                user_message: message.to_string(),
                assistant_response: String::new(),
                run_id: run_id.to_string(),
                turn_id: turn_id.to_string(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{EventKind, EventLog};
    use crate::model::ModelError;
    use crate::model_config::{ModelConfig, ModelProfile};
    use crate::model_gateway::ModelGateway;
    use crate::model_openai_http::{OpenAIHttpClient, OpenAIHttpResult};
    use crate::model_openai_request::OpenAIRequestPlan;
    use crate::model_openai_types::{
        OpenAIChatChoice, OpenAIChatMessage, OpenAIChatResponse, OpenAIUsage,
    };
    use crate::model_types::{ModelAdapterKind, ModelProvider};

    #[test]
    fn run_mock_turn_appends_correct_event_sequence() {
        let mut event_log = EventLog::new();
        let gateway = ModelGateway::default();
        let output = run_mock_turn(&mut event_log, "hello", &gateway, None);

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
        let output = run_mock_turn(&mut event_log, "hello", &gateway, None);

        assert_eq!(output.user_message, "hello");
        assert_eq!(output.assistant_response, "Mock response for: hello");
    }

    #[test]
    fn run_mock_turn_prompt_compile_detail_matches() {
        let mut event_log = EventLog::new();
        let gateway = ModelGateway::default();
        run_mock_turn(&mut event_log, "hello", &gateway, None);

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
        run_mock_turn(&mut event_log, "hello", &gateway, None);

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
        run_mock_turn(&mut event_log, "first", &gateway, None);

        event_log.append(EventKind::UserMessage, "second");
        run_mock_turn(&mut event_log, "second", &gateway, None);

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
        run_mock_turn(&mut event_log, "first", &gateway, None);

        // A slash command and other trace events must never enter the context.
        event_log.append(EventKind::SlashCommand, "/help");
        event_log.append(EventKind::UserMessage, "second");
        run_mock_turn(&mut event_log, "second", &gateway, None);

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
        let output = run_mock_turn(&mut event_log, "hello", &gateway, None);

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
        run_mock_turn(&mut event_log, "hello", &gateway, None);

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
        let output = run_mock_turn(&mut event_log, "hello", &gateway, None);

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
        run_mock_turn(&mut event_log, "hello", &gateway, None);

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
        let output = run_mock_turn(&mut event_log, "hello", &gateway, None);

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
                        content: "Hello from fake OpenAI".to_string(),
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
        let _output = run_mock_turn(&mut event_log, "hello", &gateway, None);

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
        run_mock_turn(&mut event_log, "hello", &gateway, None);

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
        run_mock_turn(&mut event_log, "hello", &gateway, None);

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
        run_mock_turn(&mut event_log, "hello caravan", &gateway, None);

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
}
