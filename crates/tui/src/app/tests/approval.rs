use super::super::App;
use super::common::TempDir;
use kernel::storage::EventStore;
use kernel::{ApprovalDecision, ApprovalDecisionRecord, ApprovalQueue, EventKind};

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
        "/approval resume",
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
    // The resolved request must not appear in the *pending* portion of the log.
    // Split at the "- approved resume plans:" marker so approved plans listed
    // after it do not falsely trigger this assertion.
    let pending_lines: Vec<&str> = app
        .log
        .iter()
        .take_while(|l| !l.starts_with("- approved resume plans:"))
        .map(|s| s.as_str())
        .collect();
    assert!(
        !pending_lines
            .iter()
            .any(|l| l.contains(&format!("seq={seq1}"))),
        "pending portion must not contain a rendered line for the resolved request seq1"
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
    // The resolved request must not appear in the *pending* portion of the log.
    // Split at the "- approved resume plans:" marker so approved plans listed
    // after it do not falsely trigger this assertion.
    let pending_lines: Vec<&str> = app
        .log
        .iter()
        .take_while(|l| !l.starts_with("- approved resume plans:"))
        .map(|s| s.as_str())
        .collect();
    assert!(
        !pending_lines
            .iter()
            .any(|l| l.contains(&format!("seq={seq1}"))),
        "pending portion must not contain a rendered line for the resolved request seq1"
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

#[test]
fn approval_status_approved_shows_resume_plan_lines() {
    let mut app = App::new();
    let request_detail = "tool=read_file path=\"README.md\" risk=read_only reason=test";
    let seq = app
        .event_log
        .append(EventKind::ApprovalRequest, request_detail);
    let decision_detail = ApprovalDecisionRecord {
        request_seq: seq,
        decision: ApprovalDecision::Approved,
        reason: "test_approved".to_string(),
    }
    .detail();
    app.event_log
        .append(EventKind::ApprovalDecision, &decision_detail);

    app.input = "/approval status".to_string();
    app.submit();

    assert!(
        app.log.iter().any(|l| l == "- approved resume plans: 1"),
        "log must contain '- approved resume plans: 1' after approving a supported request"
    );
    assert!(
        app.log
            .iter()
            .any(|l| l == "- suggested: /tool read README.md"),
        "log must contain the suggested /tool read command"
    );
}

#[test]
fn approval_status_rejected_shows_zero_resume_plans() {
    let mut app = App::new();
    let request_detail = "tool=read_file path=\"README.md\" risk=read_only reason=test";
    let seq = app
        .event_log
        .append(EventKind::ApprovalRequest, request_detail);
    let decision_detail = ApprovalDecisionRecord {
        request_seq: seq,
        decision: ApprovalDecision::Rejected,
        reason: "test_rejected".to_string(),
    }
    .detail();
    app.event_log
        .append(EventKind::ApprovalDecision, &decision_detail);

    app.input = "/approval status".to_string();
    app.submit();

    assert!(
        app.log.iter().any(|l| l == "- approved resume plans: 0"),
        "log must contain '- approved resume plans: 0' after rejecting a request"
    );
    assert!(
        !app.log.iter().any(|l| l.starts_with("- suggested:")),
        "log must not contain a suggested line when the request was rejected"
    );
}

// --- /approval resume handler tests ---

/// Seed an ApprovalRequest + approving ApprovalDecision into the event log.
/// Returns the request_seq (EventSeq) of the seeded request.
fn seed_approved_request(app: &mut App, detail: &str) -> kernel::events::EventSeq {
    let request_seq = app.event_log.append(EventKind::ApprovalRequest, detail);
    let decision_detail = ApprovalDecisionRecord {
        request_seq,
        decision: ApprovalDecision::Approved,
        reason: "test_approved".to_string(),
    }
    .detail();
    app.event_log
        .append(EventKind::ApprovalDecision, &decision_detail);
    request_seq
}

#[test]
fn approval_resume_read_file_success() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();
    std::fs::write(workspace_dir.path().join("notes.txt"), "hello world").unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    let request_seq = seed_approved_request(
        &mut app,
        "tool=read_file path=\"notes.txt\" risk=read_only reason=test_resume",
    );
    let event_len_before = app.event_log.len();

    app.input = format!("/approval resume {request_seq}");
    app.submit();

    let new_events = &app.event_log.events()[event_len_before..];
    assert_eq!(
        new_events.len(),
        5,
        "expected exactly 5 new events (SlashCommand, ApprovalResume, ToolPolicy, ToolCall, ToolResult)"
    );
    assert_eq!(new_events[0].kind, EventKind::SlashCommand);
    assert_eq!(new_events[1].kind, EventKind::ApprovalResume);
    assert_eq!(new_events[2].kind, EventKind::ToolPolicy);
    assert_eq!(new_events[3].kind, EventKind::ToolCall);
    assert_eq!(new_events[4].kind, EventKind::ToolResult);

    assert!(
        app.last_tool_output_candidate.is_some(),
        "last_tool_output_candidate must be Some after successful resume"
    );
    assert!(
        app.log.iter().any(|l| {
            l == "Run /context attach-last-tool to include this tool output in the next prompt."
        }),
        "log must contain the exact attach-hint line"
    );
    assert!(
        app.pending_manual_tool_context.is_none(),
        "pending_manual_tool_context must remain None"
    );
    assert!(
        app.pending_model_tool_request.is_none(),
        "pending_model_tool_request must remain None"
    );
}

#[test]
fn approval_resume_list_files_success() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    let request_seq = seed_approved_request(
        &mut app,
        "tool=list_files path=\".\" risk=read_only reason=test_resume",
    );
    let event_len_before = app.event_log.len();

    app.input = format!("/approval resume {request_seq}");
    app.submit();

    let new_events = &app.event_log.events()[event_len_before..];
    assert_eq!(
        new_events.len(),
        5,
        "expected exactly 5 new events (SlashCommand, ApprovalResume, ToolPolicy, ToolCall, ToolResult)"
    );
    assert_eq!(new_events[0].kind, EventKind::SlashCommand);
    assert_eq!(new_events[1].kind, EventKind::ApprovalResume);
    assert_eq!(new_events[2].kind, EventKind::ToolPolicy);
    assert_eq!(new_events[3].kind, EventKind::ToolCall);
    assert_eq!(new_events[4].kind, EventKind::ToolResult);
}

#[test]
fn approval_resume_tool_error_missing_path() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    // Seed a request for a file that does not exist in the workspace.
    let request_seq = seed_approved_request(
        &mut app,
        "tool=read_file path=\"missing.txt\" risk=read_only reason=test_resume",
    );
    let event_len_before = app.event_log.len();

    app.input = format!("/approval resume {request_seq}");
    app.submit();

    let new_events = &app.event_log.events()[event_len_before..];
    assert_eq!(
        new_events.len(),
        5,
        "expected exactly 5 new events (SlashCommand, ApprovalResume, ToolPolicy, ToolCall, ToolError)"
    );
    assert_eq!(new_events[0].kind, EventKind::SlashCommand);
    assert_eq!(new_events[1].kind, EventKind::ApprovalResume);
    assert_eq!(new_events[2].kind, EventKind::ToolPolicy);
    assert_eq!(new_events[3].kind, EventKind::ToolCall);
    assert_eq!(new_events[4].kind, EventKind::ToolError);

    assert!(
        app.last_tool_output_candidate.is_none(),
        "last_tool_output_candidate must remain None after tool error"
    );
}

#[test]
fn approval_resume_pending_request_no_decision() {
    let mut app = App::new();
    let request_seq = app.event_log.append(
        EventKind::ApprovalRequest,
        "tool=read_file path=\"notes.txt\" risk=read_only reason=test_resume",
    );
    let event_len_before = app.event_log.len();

    app.input = format!("/approval resume {request_seq}");
    app.submit();

    let new_events = &app.event_log.events()[event_len_before..];
    assert_eq!(
        new_events.len(),
        1,
        "expected exactly one new event (SlashCommand only)"
    );
    assert_eq!(new_events[0].kind, EventKind::SlashCommand);
    assert!(
        !new_events
            .iter()
            .any(|e| e.kind == EventKind::ApprovalResume),
        "must not emit ApprovalResume for a pending (undecided) request"
    );
}

#[test]
fn approval_resume_rejected_request() {
    let mut app = App::new();
    let request_seq = app.event_log.append(
        EventKind::ApprovalRequest,
        "tool=read_file path=\"notes.txt\" risk=read_only reason=test_resume",
    );
    let decision_detail = ApprovalDecisionRecord {
        request_seq,
        decision: ApprovalDecision::Rejected,
        reason: "test_rejected".to_string(),
    }
    .detail();
    app.event_log
        .append(EventKind::ApprovalDecision, &decision_detail);
    let event_len_before = app.event_log.len();

    app.input = format!("/approval resume {request_seq}");
    app.submit();

    let new_events = &app.event_log.events()[event_len_before..];
    assert_eq!(
        new_events.len(),
        1,
        "expected exactly one new event (SlashCommand only)"
    );
    assert_eq!(new_events[0].kind, EventKind::SlashCommand);
    assert!(
        !new_events
            .iter()
            .any(|e| e.kind == EventKind::ApprovalResume),
        "must not emit ApprovalResume for a rejected request"
    );
}

#[test]
fn approval_resume_unknown_seq() {
    let mut app = App::new();
    let event_len_before = app.event_log.len();

    app.input = "/approval resume 9999".to_string();
    app.submit();

    let new_events = &app.event_log.events()[event_len_before..];
    assert_eq!(
        new_events.len(),
        1,
        "expected exactly one new event (SlashCommand only)"
    );
    assert_eq!(new_events[0].kind, EventKind::SlashCommand);
    assert!(
        !new_events
            .iter()
            .any(|e| e.kind == EventKind::ApprovalResume),
        "must not emit ApprovalResume for an unknown seq"
    );
}

#[test]
fn approval_resume_unsupported_tool_write_file() {
    let mut app = App::new();
    let request_seq = seed_approved_request(
        &mut app,
        "tool=write_file path=\"output.txt\" risk=high reason=test",
    );
    let event_len_before = app.event_log.len();

    app.input = format!("/approval resume {request_seq}");
    app.submit();

    let new_events = &app.event_log.events()[event_len_before..];
    assert_eq!(
        new_events.len(),
        1,
        "expected exactly one new event (SlashCommand only) for unsupported tool"
    );
    assert_eq!(new_events[0].kind, EventKind::SlashCommand);
    assert!(
        !new_events
            .iter()
            .any(|e| e.kind == EventKind::ApprovalResume),
        "must not emit ApprovalResume for an unsupported tool"
    );
    assert!(
        !new_events.iter().any(|e| e.kind == EventKind::ToolPolicy),
        "must not emit ToolPolicy for an unsupported tool"
    );
    assert!(
        !new_events.iter().any(|e| e.kind == EventKind::ToolCall),
        "must not emit ToolCall for an unsupported tool"
    );
    assert!(
        !new_events.iter().any(|e| e.kind == EventKind::ToolResult),
        "must not emit ToolResult for an unsupported tool"
    );
    assert!(
        !new_events.iter().any(|e| e.kind == EventKind::ToolError),
        "must not emit ToolError for an unsupported tool"
    );
    assert!(
        app.log
            .iter()
            .any(|l| *l == format!("No approved resume plan for seq={request_seq}")),
        "log must contain 'No approved resume plan for seq=...' for unsupported tool"
    );
    assert!(
        app.last_tool_output_candidate.is_none(),
        "last_tool_output_candidate must remain None"
    );
}

#[test]
fn approval_resume_status_not_listed_after_success() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();
    std::fs::write(workspace_dir.path().join("notes.txt"), "hello world").unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    let request_seq = seed_approved_request(
        &mut app,
        "tool=read_file path=\"notes.txt\" risk=read_only reason=test_resume",
    );

    app.input = format!("/approval resume {request_seq}");
    app.submit();

    // Clear log to make assertions simpler.
    app.log.clear();
    app.input = "/approval status".to_string();
    app.submit();

    assert!(
        app.log.iter().any(|l| l == "- approved resume plans: 0"),
        "after successful resume, /approval status must show 0 approved resume plans"
    );
}

#[test]
fn approval_resume_status_not_listed_after_error() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    // Use a path that doesn't exist → tool error, but ApprovalResume is still appended.
    let request_seq = seed_approved_request(
        &mut app,
        "tool=read_file path=\"missing.txt\" risk=read_only reason=test_resume",
    );

    app.input = format!("/approval resume {request_seq}");
    app.submit();

    // Clear log to make assertions simpler.
    app.log.clear();
    app.input = "/approval status".to_string();
    app.submit();

    assert!(
        app.log.iter().any(|l| l == "- approved resume plans: 0"),
        "after failed resume, /approval status must show 0 approved resume plans"
    );
}

#[test]
fn approval_resume_attach_last_tool_after_success() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();
    std::fs::write(workspace_dir.path().join("notes.txt"), "hello world").unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    let request_seq = seed_approved_request(
        &mut app,
        "tool=read_file path=\"notes.txt\" risk=read_only reason=test_resume",
    );

    app.input = format!("/approval resume {request_seq}");
    app.submit();

    assert!(
        app.last_tool_output_candidate.is_some(),
        "last_tool_output_candidate must be Some before attach"
    );
    assert!(
        app.pending_manual_tool_context.is_none(),
        "pending_manual_tool_context must be None before attach"
    );

    app.input = "/context attach-last-tool".to_string();
    app.submit();

    assert!(
        app.pending_manual_tool_context.is_some(),
        "pending_manual_tool_context must be Some after attach-last-tool"
    );

    let events = app.event_log.events();
    assert!(
        events
            .iter()
            .any(|e| e.kind == EventKind::ToolContextAttach),
        "must emit ToolContextAttach after attach-last-tool"
    );
}

// --- /tool plan-write approval-flow regression tests ---

/// Real plan-write `ApprovalRequest` detail: `risk=workspace_write`, not the old `risk=high`.
const PLAN_WRITE_APPROVAL_DETAIL: &str = "tool=write_file path=\"README.md\" risk=workspace_write reason=workspace_write_requires_approval";

#[test]
fn plan_write_approve_shows_pending_then_resolves() {
    let mut app = App::new();

    // Submit the plan-write command: emits SlashCommand, ToolPolicy, ApprovalRequest.
    app.input = "/tool plan-write README.md".to_string();
    app.submit();

    // Extract the ApprovalRequest event seq (last event appended).
    let events = app.event_log.events();
    let approval_event = events
        .iter()
        .rev()
        .find(|e| e.kind == EventKind::ApprovalRequest)
        .expect("ApprovalRequest event must be present after /tool plan-write");
    let approval_seq = approval_event.seq;

    // /approval status must show 1 pending entry with the workspace_write detail.
    app.log.clear();
    app.input = "/approval status".to_string();
    app.submit();

    assert!(
        app.log.iter().any(|l| l == "- pending: 1"),
        "log must contain '- pending: 1' after plan-write"
    );
    assert!(
        app.log.iter().any(|l| l.contains("workspace_write")),
        "log must contain 'workspace_write' in the pending entry detail"
    );

    // /approval approve <seq> must append SlashCommand + ApprovalDecision and remove from pending.
    let len_before = app.event_log.len();
    app.input = format!("/approval approve {approval_seq}");
    app.submit();

    let new_events = &app.event_log.events()[len_before..];
    assert_eq!(
        new_events.len(),
        2,
        "expected exactly 2 new events (SlashCommand + ApprovalDecision)"
    );
    assert_eq!(new_events[0].kind, EventKind::SlashCommand);
    assert_eq!(new_events[1].kind, EventKind::ApprovalDecision);
    assert!(
        app.log
            .iter()
            .any(|l| *l == format!("Approved approval request seq={approval_seq}")),
        "log must contain 'Approved approval request seq=...'"
    );

    // After approve, /approval status must show pending: none.
    app.log.clear();
    app.input = "/approval status".to_string();
    app.submit();

    assert!(
        app.log.iter().any(|l| l == "- pending: none"),
        "log must contain '- pending: none' after plan-write approve"
    );
}

#[test]
fn plan_write_reject_shows_pending_then_resolves() {
    let mut app = App::new();

    // Submit the plan-write command: emits SlashCommand, ToolPolicy, ApprovalRequest.
    app.input = "/tool plan-write README.md".to_string();
    app.submit();

    // Extract the ApprovalRequest event seq.
    let events = app.event_log.events();
    let approval_event = events
        .iter()
        .rev()
        .find(|e| e.kind == EventKind::ApprovalRequest)
        .expect("ApprovalRequest event must be present after /tool plan-write");
    let approval_seq = approval_event.seq;

    // /approval reject <seq> must append SlashCommand + ApprovalDecision (rejected).
    let len_before = app.event_log.len();
    app.input = format!("/approval reject {approval_seq}");
    app.submit();

    let new_events = &app.event_log.events()[len_before..];
    assert_eq!(
        new_events.len(),
        2,
        "expected exactly 2 new events (SlashCommand + ApprovalDecision)"
    );
    assert_eq!(new_events[0].kind, EventKind::SlashCommand);
    assert_eq!(new_events[1].kind, EventKind::ApprovalDecision);
    assert_eq!(
        new_events[1].detail,
        format!("request_seq={approval_seq} decision=rejected reason=operator_rejected")
    );
    assert!(
        app.log
            .iter()
            .any(|l| *l == format!("Rejected approval request seq={approval_seq}")),
        "log must contain 'Rejected approval request seq=...'"
    );

    // After reject, /approval status must show pending: none.
    app.log.clear();
    app.input = "/approval status".to_string();
    app.submit();

    assert!(
        app.log.iter().any(|l| l == "- pending: none"),
        "log must contain '- pending: none' after plan-write reject"
    );
}

#[test]
fn plan_write_resume_is_noop() {
    let mut app = App::new();

    // Seed an approved plan-write request using the real workspace_write detail shape.
    let request_seq = app
        .event_log
        .append(EventKind::ApprovalRequest, PLAN_WRITE_APPROVAL_DETAIL);
    let decision_detail = ApprovalDecisionRecord {
        request_seq,
        decision: ApprovalDecision::Approved,
        reason: "test_approved".to_string(),
    }
    .detail();
    app.event_log
        .append(EventKind::ApprovalDecision, &decision_detail);

    // ApprovalQueue::resume_plans() must yield 0 plans: write_file to_tool_request returns None.
    let queue = ApprovalQueue::from_event_log(&app.event_log);
    let plans = queue.resume_plans();
    assert_eq!(
        plans.len(),
        0,
        "resume_plans must yield 0 plans for an approved write_file (workspace_write) request"
    );

    // /approval resume <seq> must append only a SlashCommand — no resume or tool events.
    let event_len_before = app.event_log.len();
    app.input = format!("/approval resume {request_seq}");
    app.submit();

    let new_events = &app.event_log.events()[event_len_before..];
    assert_eq!(
        new_events.len(),
        1,
        "expected exactly one new event (SlashCommand only) for plan-write resume"
    );
    assert_eq!(
        new_events[0].kind,
        EventKind::SlashCommand,
        "the single new event must be SlashCommand"
    );
    assert!(
        !new_events
            .iter()
            .any(|e| e.kind == EventKind::ApprovalResume),
        "must not emit ApprovalResume for a plan-write resume"
    );
    assert!(
        !new_events.iter().any(|e| e.kind == EventKind::ToolPolicy),
        "must not emit ToolPolicy for a plan-write resume"
    );
    assert!(
        !new_events.iter().any(|e| e.kind == EventKind::ToolCall),
        "must not emit ToolCall for a plan-write resume"
    );
    assert!(
        !new_events.iter().any(|e| e.kind == EventKind::ToolResult),
        "must not emit ToolResult for a plan-write resume"
    );
    assert!(
        !new_events.iter().any(|e| e.kind == EventKind::ToolError),
        "must not emit ToolError for a plan-write resume"
    );
    assert!(
        app.last_tool_output_candidate.is_none(),
        "last_tool_output_candidate must remain None after plan-write resume"
    );
}

#[test]
fn approval_status_with_approved_plans_mutates_log_by_exactly_one_slash_command() {
    let mut app = App::new();
    let request_detail = "tool=read_file path=\"README.md\" risk=read_only reason=test";
    let seq = app
        .event_log
        .append(EventKind::ApprovalRequest, request_detail);
    let decision_detail = ApprovalDecisionRecord {
        request_seq: seq,
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
    assert!(
        !new_events
            .iter()
            .any(|e| e.kind == EventKind::ApprovalDecision),
        "must not append ApprovalDecision"
    );
}
