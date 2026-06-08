use crate::events::{EventKind, EventLog, RunId, TurnId};

pub struct MockRunOutput {
    pub user_message: String,
    pub assistant_response: String,
    pub run_id: String,
    pub turn_id: String,
}

pub fn run_mock_turn(event_log: &mut EventLog, message: &str) -> MockRunOutput {
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
    event_log.append(
        EventKind::PromptCompile,
        crate::prompt::compile_prompt(message),
    );
    let mock_response = format!("Mock response for: {}", message);
    for word in mock_response.split_whitespace() {
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
        assistant_response: mock_response,
        run_id: run_id.to_string(),
        turn_id: turn_id.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{EventKind, EventLog};

    #[test]
    fn run_mock_turn_appends_correct_event_sequence() {
        let mut event_log = EventLog::new();
        let output = run_mock_turn(&mut event_log, "hello");

        let events = event_log.events();
        let n_tokens = "Mock response for: hello".split_whitespace().count();

        let mut expected_kinds = vec![
            EventKind::RunCreate,
            EventKind::RunStart,
            EventKind::TurnStart,
            EventKind::PromptCompile,
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
    }

    #[test]
    fn run_mock_turn_returns_correct_output_fields() {
        let mut event_log = EventLog::new();
        let output = run_mock_turn(&mut event_log, "hello");

        assert_eq!(output.user_message, "hello");
        assert_eq!(output.assistant_response, "Mock response for: hello");
    }

    #[test]
    fn run_mock_turn_prompt_compile_detail_matches() {
        let mut event_log = EventLog::new();
        run_mock_turn(&mut event_log, "hello");

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
        let output = run_mock_turn(&mut event_log, "hello");

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
}
