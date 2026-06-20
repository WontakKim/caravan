use super::super::App;
use kernel::{ApprovalDecision, ApprovalDecisionRecord, EventKind};

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

#[test]
fn approval_status_excludes_resolved_request() {
    let mut app = App::new();
    let request_detail = "tool=read_file path=\"README.md\" risk=read_only reason=test_resolved";
    let seq1 = app
        .event_log
        .append(EventKind::ApprovalRequest, request_detail);
    let decision_detail = ApprovalDecisionRecord {
        request_seq: seq1,
        decision: ApprovalDecision::Approved,
        reason: "test_approved".to_string(),
    }
    .detail();
    app.event_log
        .append(EventKind::ApprovalDecision, &decision_detail);

    app.input = "/approval status".to_string();
    app.submit();

    assert!(
        app.log.iter().any(|l| l == "- pending: none"),
        "log must contain '- pending: none' for a fully resolved queue"
    );
    assert!(
        !app.log.iter().any(|l| l.contains(&format!("seq={seq1}"))),
        "log must not contain a rendered line for the resolved request seq1"
    );
}

#[test]
fn approval_status_shows_only_unresolved_when_mixed() {
    let mut app = App::new();
    let detail1 = "tool=read_file path=\"README.md\" risk=read_only reason=test_resolved";
    let seq1 = app.event_log.append(EventKind::ApprovalRequest, detail1);
    let decision_detail = ApprovalDecisionRecord {
        request_seq: seq1,
        decision: ApprovalDecision::Approved,
        reason: "approved_first".to_string(),
    }
    .detail();
    app.event_log
        .append(EventKind::ApprovalDecision, &decision_detail);

    let detail2 = "tool=read_file path=\"src/main.rs\" risk=read_only reason=test_pending";
    let seq2 = app.event_log.append(EventKind::ApprovalRequest, detail2);

    app.input = "/approval status".to_string();
    app.submit();

    assert!(
        app.log.iter().any(|l| l == "- pending: 1"),
        "log must contain '- pending: 1' when one request is unresolved"
    );
    assert!(
        app.log
            .iter()
            .any(|l| *l == format!("- seq={seq2} {detail2}")),
        "log must contain the rendered line for the unresolved request seq2"
    );
    assert!(
        !app.log.iter().any(|l| l.contains(&format!("seq={seq1}"))),
        "log must not contain a rendered line for the resolved request seq1"
    );
}

#[test]
fn approval_status_does_not_append_approval_decision() {
    let mut app = App::new();
    let seq1 = app.event_log.append(
        EventKind::ApprovalRequest,
        "tool=read_file path=\"README.md\" risk=read_only reason=test_resolved",
    );
    let decision_detail = ApprovalDecisionRecord {
        request_seq: seq1,
        decision: ApprovalDecision::Approved,
        reason: "test_approved".to_string(),
    }
    .detail();
    app.event_log
        .append(EventKind::ApprovalDecision, &decision_detail);

    let event_len_before = app.event_log.len();
    app.input = "/approval status".to_string();
    app.submit();

    assert_eq!(
        app.event_log.len(),
        event_len_before + 1,
        "expected exactly one new event appended by /approval status"
    );
    let new_events = &app.event_log.events()[event_len_before..];
    assert_eq!(
        new_events[0].kind,
        EventKind::SlashCommand,
        "the single new event must be SlashCommand"
    );
    assert!(
        !new_events
            .iter()
            .any(|e| e.kind == EventKind::ApprovalDecision),
        "must not append an ApprovalDecision event"
    );
}

#[test]
fn approval_approve_pending_appends_decision_and_logs() {
    let mut app = App::new();
    let request_seq = app.event_log.append(
        EventKind::ApprovalRequest,
        "tool=read_file path=\"README.md\" risk=read_only reason=test",
    );
    let len_before = app.event_log.len();

    app.input = format!("/approval approve {request_seq}");
    app.submit();

    let new_events = &app.event_log.events()[len_before..];
    assert_eq!(
        new_events.len(),
        2,
        "expected exactly two new events (SlashCommand + ApprovalDecision)"
    );
    assert_eq!(
        new_events[0].kind,
        EventKind::SlashCommand,
        "first new event must be SlashCommand"
    );
    assert_eq!(
        new_events[1].kind,
        EventKind::ApprovalDecision,
        "second new event must be ApprovalDecision"
    );
    assert_eq!(
        new_events[1].detail,
        format!("request_seq={request_seq} decision=approved reason=operator_approved")
    );
    assert!(
        app.log
            .iter()
            .any(|l| *l == format!("Approved approval request seq={request_seq}")),
        "log must contain 'Approved approval request seq=...'"
    );
}

#[test]
fn approval_reject_pending_appends_decision_and_logs() {
    let mut app = App::new();
    let request_seq = app.event_log.append(
        EventKind::ApprovalRequest,
        "tool=write_file path=\"/etc/hosts\" risk=high reason=test",
    );
    let len_before = app.event_log.len();

    app.input = format!("/approval reject {request_seq}");
    app.submit();

    let new_events = &app.event_log.events()[len_before..];
    assert_eq!(
        new_events.len(),
        2,
        "expected exactly two new events (SlashCommand + ApprovalDecision)"
    );
    assert_eq!(
        new_events[0].kind,
        EventKind::SlashCommand,
        "first new event must be SlashCommand"
    );
    assert_eq!(
        new_events[1].kind,
        EventKind::ApprovalDecision,
        "second new event must be ApprovalDecision"
    );
    assert_eq!(
        new_events[1].detail,
        format!("request_seq={request_seq} decision=rejected reason=operator_rejected")
    );
    assert!(
        app.log
            .iter()
            .any(|l| *l == format!("Rejected approval request seq={request_seq}")),
        "log must contain 'Rejected approval request seq=...'"
    );
}

#[test]
fn approval_approve_non_pending_appends_only_slash_command() {
    let mut app = App::new();
    let len_before = app.event_log.len();

    // seq=9999 was never requested
    app.input = "/approval approve 9999".to_string();
    app.submit();

    let new_events = &app.event_log.events()[len_before..];
    assert_eq!(
        new_events.len(),
        1,
        "expected exactly one new event (SlashCommand only)"
    );
    assert_eq!(new_events[0].kind, EventKind::SlashCommand);
    assert!(
        !new_events
            .iter()
            .any(|e| e.kind == EventKind::ApprovalDecision),
        "must not append ApprovalDecision for non-pending seq"
    );
    assert!(
        app.log
            .iter()
            .any(|l| l == "No pending approval for seq=9999"),
        "log must contain 'No pending approval for seq=9999'"
    );
}

#[test]
fn approval_approve_already_resolved_appends_only_slash_command() {
    let mut app = App::new();
    let request_detail = "tool=read_file path=\"README.md\" risk=read_only reason=test_resolved";
    let seq1 = app
        .event_log
        .append(EventKind::ApprovalRequest, request_detail);
    let decision_detail = ApprovalDecisionRecord {
        request_seq: seq1,
        decision: ApprovalDecision::Approved,
        reason: "already_resolved".to_string(),
    }
    .detail();
    app.event_log
        .append(EventKind::ApprovalDecision, &decision_detail);
    let len_before = app.event_log.len();

    app.input = format!("/approval approve {seq1}");
    app.submit();

    let new_events = &app.event_log.events()[len_before..];
    assert_eq!(
        new_events.len(),
        1,
        "expected exactly one new event (SlashCommand only) for already-resolved seq"
    );
    assert_eq!(new_events[0].kind, EventKind::SlashCommand);
    assert!(
        !new_events
            .iter()
            .any(|e| e.kind == EventKind::ApprovalDecision),
        "must not append ApprovalDecision for already-resolved seq"
    );
    assert!(
        app.log
            .iter()
            .any(|l| *l == format!("No pending approval for seq={seq1}")),
        "log must contain 'No pending approval for seq=...'"
    );
}

#[test]
fn approval_status_logs_pending_none_after_approve() {
    let mut app = App::new();
    let request_seq = app.event_log.append(
        EventKind::ApprovalRequest,
        "tool=read_file path=\"README.md\" risk=read_only reason=test",
    );

    app.input = format!("/approval approve {request_seq}");
    app.submit();

    app.input = "/approval status".to_string();
    app.submit();

    assert!(
        app.log.iter().any(|l| l == "- pending: none"),
        "log must contain '- pending: none' after approve"
    );
}

#[test]
fn approval_status_logs_pending_none_after_reject() {
    let mut app = App::new();
    let request_seq = app.event_log.append(
        EventKind::ApprovalRequest,
        "tool=write_file path=\"/etc/hosts\" risk=high reason=test",
    );

    app.input = format!("/approval reject {request_seq}");
    app.submit();

    app.input = "/approval status".to_string();
    app.submit();

    assert!(
        app.log.iter().any(|l| l == "- pending: none"),
        "log must contain '- pending: none' after reject"
    );
}

#[test]
fn approval_approve_does_not_mutate_pending_state() {
    let mut app = App::new();
    let request_seq = app.event_log.append(
        EventKind::ApprovalRequest,
        "tool=read_file path=\"README.md\" risk=read_only reason=test",
    );

    app.input = format!("/approval approve {request_seq}");
    app.submit();

    assert!(
        app.pending_model_tool_request.is_none(),
        "pending_model_tool_request must remain None after approve"
    );
    assert!(
        app.pending_manual_tool_context.is_none(),
        "pending_manual_tool_context must remain None after approve"
    );
    assert!(
        app.last_tool_output_candidate.is_none(),
        "last_tool_output_candidate must remain None after approve"
    );
}

#[test]
fn approval_reject_does_not_mutate_pending_state() {
    let mut app = App::new();
    let request_seq = app.event_log.append(
        EventKind::ApprovalRequest,
        "tool=write_file path=\"/etc/hosts\" risk=high reason=test",
    );

    app.input = format!("/approval reject {request_seq}");
    app.submit();

    assert!(
        app.pending_model_tool_request.is_none(),
        "pending_model_tool_request must remain None after reject"
    );
    assert!(
        app.pending_manual_tool_context.is_none(),
        "pending_manual_tool_context must remain None after reject"
    );
    assert!(
        app.last_tool_output_candidate.is_none(),
        "last_tool_output_candidate must remain None after reject"
    );
}

#[test]
fn production_tool_list_emits_no_approval_events() {
    let mut app = App::new();
    app.input = "/tool list .".to_string();
    app.submit();

    let events = app.event_log.events();
    assert!(
        !events.iter().any(|e| e.kind == EventKind::ApprovalRequest),
        "production /tool list . must not emit ApprovalRequest"
    );
    assert!(
        !events.iter().any(|e| e.kind == EventKind::ApprovalDecision),
        "production /tool list . must not emit ApprovalDecision"
    );
}
