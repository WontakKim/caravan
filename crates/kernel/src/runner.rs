use crate::events::{EventKind, EventLog, RunId, TurnId};
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
    let prompt = crate::prompt::compile_prompt(message);
    event_log.append(EventKind::PromptCompile, prompt.clone());
    let request = ModelRequest {
        prompt,
        user_message: message.to_string(),
    };
    match gateway.complete(request) {
        Ok(response) => {
            event_log.append(EventKind::ModelRoute, response.route.detail());
            for word in &response.tokens {
                event_log.append(
                    EventKind::ModelToken,
                    format!("run_id={} turn_id={} text=\"{}\"", run_id, turn_id, word),
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

    #[test]
    fn run_mock_turn_appends_correct_event_sequence() {
        let mut event_log = EventLog::new();
        let gateway = ModelGateway::default();
        let output = run_mock_turn(&mut event_log, "hello", &gateway);

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
            expected_kinds.push(EventKind::ModelToken);
        }
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
        let output = run_mock_turn(&mut event_log, "hello", &gateway);

        assert_eq!(output.user_message, "hello");
        assert_eq!(output.assistant_response, "Mock response for: hello");
    }

    #[test]
    fn run_mock_turn_prompt_compile_detail_matches() {
        let mut event_log = EventLog::new();
        let gateway = ModelGateway::default();
        run_mock_turn(&mut event_log, "hello", &gateway);

        let events = event_log.events();
        let pc = events
            .iter()
            .find(|e| e.kind == EventKind::PromptCompile)
            .expect("PromptCompile event should exist");

        assert_eq!(pc.detail, crate::prompt::compile_prompt("hello"));
    }

    #[test]
    fn run_mock_turn_ids_match_event_seq_details() {
        let mut event_log = EventLog::new();
        let gateway = ModelGateway::default();
        let output = run_mock_turn(&mut event_log, "hello", &gateway);

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
    fn run_mock_turn_token_count_matches_model_output() {
        let mut event_log = EventLog::new();
        let gateway = ModelGateway::default();
        run_mock_turn(&mut event_log, "hello", &gateway);

        let token_events = event_log
            .events()
            .iter()
            .filter(|e| e.kind == EventKind::ModelToken)
            .count();
        let expected = ModelGateway::default()
            .complete(ModelRequest {
                prompt: crate::prompt::compile_prompt("hello"),
                user_message: "hello".to_string(),
            })
            .unwrap()
            .tokens
            .len();
        assert_eq!(token_events, expected);
    }

    #[test]
    fn run_mock_turn_response_matches_model_output() {
        let mut event_log = EventLog::new();
        let gateway = ModelGateway::default();
        let output = run_mock_turn(&mut event_log, "hello", &gateway);

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
        run_mock_turn(&mut event_log, "hello", &gateway);

        let events = event_log.events();

        // Find the PromptCompile and first ModelToken indices.
        let prompt_compile_idx = events
            .iter()
            .position(|e| e.kind == EventKind::PromptCompile)
            .expect("PromptCompile event should exist");
        let first_model_token_idx = events
            .iter()
            .position(|e| e.kind == EventKind::ModelToken)
            .expect("ModelToken event should exist");

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

        // ModelRoute must be immediately after PromptCompile and before first ModelToken.
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
            "ModelRoute should be before the first ModelToken"
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
        let output = run_mock_turn(&mut event_log, "hello", &gateway);

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

        // Must NOT contain ModelToken or RunComplete.
        assert!(
            !events.iter().any(|e| e.kind == EventKind::ModelToken),
            "error path must not emit ModelToken events"
        );
        assert!(
            !events.iter().any(|e| e.kind == EventKind::RunComplete),
            "error path must not emit RunComplete event"
        );
    }
}
