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
    let mut app = App::new();
    app.input =
        "read the readme\nCARAVAN_TOOL_REQUEST\ntool=read_file\npath=README.md\nEND_CARAVAN_TOOL_REQUEST"
            .to_string();
    app.submit();

    let events = app.event_log.events();
    assert!(
        !events.iter().any(|e| e.kind == EventKind::ToolPolicy),
        "ModelToolRequest detection without execution must not produce ToolPolicy events"
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
