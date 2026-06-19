use super::super::App;
use kernel::EventKind;

#[test]
fn approval_status_no_pending_logs_none() {
    let mut app = App::new();
    app.input = "/approval status".to_string();
    app.submit();

    assert!(
        app.log.iter().any(|l| l == "Approval status:"),
        "log must contain 'Approval status:'"
    );
    assert!(
        app.log.iter().any(|l| l == "- pending: none"),
        "log must contain '- pending: none'"
    );

    let events = app.event_log.events();
    let slash_count = events
        .iter()
        .filter(|e| e.kind == EventKind::SlashCommand)
        .count();
    assert_eq!(slash_count, 1, "expected exactly one SlashCommand event");

    assert!(
        !events.iter().any(|e| e.kind == EventKind::ApprovalRequest),
        "must not emit ApprovalRequest"
    );
    assert!(
        !events.iter().any(|e| e.kind == EventKind::ToolPolicy),
        "must not emit ToolPolicy"
    );
    assert!(
        !events.iter().any(|e| e.kind == EventKind::ToolCall),
        "must not emit ToolCall"
    );
    assert!(
        !events.iter().any(|e| e.kind == EventKind::ToolResult),
        "must not emit ToolResult"
    );
    assert!(
        !events.iter().any(|e| e.kind == EventKind::ToolError),
        "must not emit ToolError"
    );

    assert!(
        app.pending_model_tool_request.is_none(),
        "pending_model_tool_request must be None"
    );
    assert!(
        app.pending_manual_tool_context.is_none(),
        "pending_manual_tool_context must be None"
    );
    assert!(
        app.last_tool_output_candidate.is_none(),
        "last_tool_output_candidate must be None"
    );
}

#[test]
fn approval_status_with_seeded_pending_shows_seq_and_detail() {
    let mut app = App::new();
    let seq1 = app.event_log.append(
        EventKind::ApprovalRequest,
        "tool=read_file path=\"README.md\" risk=read_only reason=test_manual_approval",
    );
    let seq2 = app.event_log.append(
        EventKind::ApprovalRequest,
        "tool=read_file path=\"src/main.rs\" risk=read_only reason=test_manual_approval",
    );
    let event_len_before = app.event_log.len();

    app.input = "/approval status".to_string();
    app.submit();

    assert!(
        app.log.iter().any(|l| l == "- pending: 2"),
        "log must contain '- pending: 2'"
    );
    // Exact rendered lines for both seeded items, in event order.
    assert!(
        app.log.iter().any(|l| *l
            == format!(
                "- seq={seq1} tool=read_file path=\"README.md\" risk=read_only reason=test_manual_approval"
            )),
        "log must contain the exact rendered line for the first pending approval"
    );
    assert!(
        app.log.iter().any(|l| *l
            == format!(
                "- seq={seq2} tool=read_file path=\"src/main.rs\" risk=read_only reason=test_manual_approval"
            )),
        "log must contain the exact rendered line for the second pending approval"
    );

    // Observe-only guarantees must also hold on the non-empty (pending) path:
    // only a SlashCommand event is appended, no tool/approval events are emitted,
    // and no pending/candidate state is mutated.
    assert_eq!(
        app.event_log.len(),
        event_len_before + 1,
        "expected exactly one new event on the pending path"
    );
    let new_events = &app.event_log.events()[event_len_before..];
    assert_eq!(new_events[0].kind, EventKind::SlashCommand);
    assert!(
        !new_events
            .iter()
            .any(|e| e.kind == EventKind::ApprovalRequest),
        "must not append ApprovalRequest on the pending path"
    );
    assert!(
        !new_events.iter().any(|e| e.kind == EventKind::ToolPolicy),
        "must not append ToolPolicy on the pending path"
    );
    assert!(
        !new_events.iter().any(|e| e.kind == EventKind::ToolCall),
        "must not append ToolCall on the pending path"
    );
    assert!(
        !new_events.iter().any(|e| e.kind == EventKind::ToolResult),
        "must not append ToolResult on the pending path"
    );
    assert!(
        !new_events.iter().any(|e| e.kind == EventKind::ToolError),
        "must not append ToolError on the pending path"
    );
    assert!(
        app.pending_model_tool_request.is_none(),
        "pending_model_tool_request must remain None on the pending path"
    );
    assert!(
        app.pending_manual_tool_context.is_none(),
        "pending_manual_tool_context must remain None on the pending path"
    );
    assert!(
        app.last_tool_output_candidate.is_none(),
        "last_tool_output_candidate must remain None on the pending path"
    );
}

#[test]
fn approval_status_only_appends_slash_command() {
    let mut app = App::new();
    let event_len_before = app.event_log.len();

    app.input = "/approval status".to_string();
    app.submit();

    assert_eq!(
        app.event_log.len(),
        event_len_before + 1,
        "expected exactly one new event"
    );
    let new_event = app.event_log.get(event_len_before).unwrap();
    assert_eq!(new_event.kind, EventKind::SlashCommand);

    let new_events = &app.event_log.events()[event_len_before..];
    assert!(
        !new_events
            .iter()
            .any(|e| e.kind == EventKind::ApprovalRequest),
        "must not append ApprovalRequest"
    );
    assert!(
        !new_events.iter().any(|e| e.kind == EventKind::ToolPolicy),
        "must not append ToolPolicy"
    );
    assert!(
        !new_events.iter().any(|e| e.kind == EventKind::ToolCall),
        "must not append ToolCall"
    );
    assert!(
        !new_events.iter().any(|e| e.kind == EventKind::ToolResult),
        "must not append ToolResult"
    );
    assert!(
        !new_events.iter().any(|e| e.kind == EventKind::ToolError),
        "must not append ToolError"
    );
}

#[test]
fn approval_subcommands_are_unknown() {
    let inputs = [
        "/approval",
        "/approval approve",
        "/approval reject",
        "/approval clear",
        "/approval run",
        "/approval unknown",
    ];

    for input in inputs {
        let mut app = App::new();
        app.input = input.to_string();
        app.submit();

        let events = app.event_log.events();
        assert!(
            events
                .iter()
                .any(|e| e.kind == EventKind::UnknownSlashCommand),
            "expected UnknownSlashCommand for input: {input}"
        );
        assert!(
            app.log.iter().any(|l| l.contains("Unknown command:")),
            "expected 'Unknown command:' in log for input: {input}"
        );
    }
}
