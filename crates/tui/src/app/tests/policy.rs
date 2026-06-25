use super::super::*;
use super::common::*;
use kernel::events::EventKind;

// --- Negative-path ToolPolicy tests (Step 11) ---

#[test]
fn invalid_tool_subcommand_produces_no_tool_policy_event() {
    let mut app = App::new();
    app.input = "/tool bogus".to_string();
    app.submit();

    let events = app.event_log.events();
    assert!(
        !events.iter().any(|e| e.kind == EventKind::ToolPolicy),
        "invalid /tool command must not produce ToolPolicy events"
    );
}

#[test]
fn request_run_without_pending_produces_no_tool_policy_event() {
    let mut app = App::new();
    assert!(app.pending_model_tool_request.is_none());

    app.input = "/request run".to_string();
    app.submit();

    let events = app.event_log.events();
    assert!(
        !events.iter().any(|e| e.kind == EventKind::ToolPolicy),
        "/request run with no pending request must not produce ToolPolicy events"
    );
}

#[test]
fn model_tool_request_detection_only_produces_no_tool_policy_event() {
    // The default runtime no longer detects model tool requests; the
    // CARAVAN_TOOL_REQUEST block is treated as plain assistant text. Either
    // way, submitting the message must never produce a ToolPolicy event.
    let mut app = App::new();
    app.input =
        "read the readme\nCARAVAN_TOOL_REQUEST\ntool=read_file\npath=README.md\nEND_CARAVAN_TOOL_REQUEST"
            .to_string();
    app.submit();

    let events = app.event_log.events();
    assert!(
        !events.iter().any(|e| e.kind == EventKind::ToolPolicy),
        "ModelToolRequest block without execution must not produce ToolPolicy events"
    );
}

// --- PlanWrite ToolPolicy detail assertion ---

#[test]
fn tool_plan_write_emits_tool_policy_with_workspace_write_detail() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    let store = kernel::storage::EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    app.input = "/tool plan-write README.md".to_string();
    app.submit();

    let events = app.event_log.events();
    let policy_event = events
        .iter()
        .find(|e| e.kind == EventKind::ToolPolicy)
        .expect("ToolPolicy event must be present for plan-write");

    assert_eq!(
        policy_event.detail,
        r#"tool=write_file path="README.md" risk=workspace_write decision=allow reason=workspace_write_requires_approval"#
    );
}

// --- PolicyDenied arm test (T-5) ---

#[test]
fn push_tool_error_output_policy_denied_formats_message() {
    let mut app = App::new();
    app.push_tool_error_output(kernel::ToolError::PolicyDenied {
        reason: "test_reason".to_string(),
    });
    let last = app.log.last().unwrap();
    assert!(
        last.contains("policy denied"),
        "log must contain 'policy denied': {last}"
    );
    assert!(
        last.contains("test_reason"),
        "log must contain 'test_reason': {last}"
    );
}
