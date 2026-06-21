use super::super::*;
use super::common::*;
use kernel::events::EventKind;
use kernel::storage::EventStore;

#[test]
fn tool_list_success_appends_slash_tool_call_result_events() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    std::fs::write(workspace_dir.path().join("alpha.txt"), "a").unwrap();
    std::fs::write(workspace_dir.path().join("beta.txt"), "b").unwrap();
    std::fs::write(workspace_dir.path().join("gamma.txt"), "g").unwrap();
    let entry_count = 3;

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    let event_len_before = app.event_log.len();
    app.input = "/tool list .".to_string();
    app.submit();

    assert_eq!(app.event_log.len(), event_len_before + 4);
    let events = app.event_log.events();
    let n = events.len();
    assert_eq!(events[n - 4].kind, EventKind::SlashCommand);
    assert_eq!(events[n - 3].kind, EventKind::ToolPolicy);
    assert_eq!(events[n - 2].kind, EventKind::ToolCall);
    assert_eq!(events[n - 1].kind, EventKind::ToolResult);

    assert!(app.log.iter().any(|l| l == "Tool list .:"));
    assert!(
        events[n - 1]
            .detail
            .contains(&format!("entries={}", entry_count))
    );
}

#[test]
fn tool_read_success_appends_slash_tool_call_result_events() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    let content = "hello, world!";
    std::fs::write(workspace_dir.path().join("greeting.txt"), content).unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    let event_len_before = app.event_log.len();
    app.input = "/tool read greeting.txt".to_string();
    app.submit();

    assert_eq!(app.event_log.len(), event_len_before + 4);
    let events = app.event_log.events();
    let n = events.len();
    assert_eq!(events[n - 4].kind, EventKind::SlashCommand);
    assert_eq!(events[n - 3].kind, EventKind::ToolPolicy);
    assert_eq!(events[n - 2].kind, EventKind::ToolCall);
    assert_eq!(events[n - 1].kind, EventKind::ToolResult);

    assert!(app.log.iter().any(|l| l == "Tool read greeting.txt:"));
    assert!(
        events[n - 1]
            .detail
            .contains(&format!("bytes={}", content.len()))
    );

    // No event in the event_log may contain the raw file content.
    assert!(!events.iter().any(|e| e.detail.contains(content)));
}

#[test]
fn tool_read_workspace_violation_appends_slash_tool_call_error_events() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    let log_len_before = app.log.len();
    let event_len_before = app.event_log.len();
    app.input = "/tool read ../secret.txt".to_string();
    app.submit();

    assert_eq!(app.event_log.len(), event_len_before + 4);
    let events = app.event_log.events();
    let n = events.len();
    assert_eq!(events[n - 4].kind, EventKind::SlashCommand);
    assert_eq!(events[n - 3].kind, EventKind::ToolPolicy);
    assert_eq!(events[n - 2].kind, EventKind::ToolCall);
    assert_eq!(events[n - 1].kind, EventKind::ToolError);

    // Screen log gained a readable error line.
    assert!(app.log.len() > log_len_before);
    assert!(app.log.iter().any(|l| l.contains("Tool error:")));
}

#[test]
fn tool_malformed_commands_produce_only_slash_and_unknown_events() {
    for input in &["/tool", "/tool read", "/tool foo some-file"] {
        let mut app = App::new();
        let event_len_before = app.event_log.len();
        app.input = input.to_string();
        app.submit();

        assert_eq!(
            app.event_log.len(),
            event_len_before + 2,
            "expected +2 events for input: {input}"
        );

        let events = app.event_log.events();
        let n = events.len();
        assert_eq!(
            events[n - 2].kind,
            EventKind::SlashCommand,
            "expected SlashCommand for: {input}"
        );
        assert_eq!(
            events[n - 1].kind,
            EventKind::UnknownSlashCommand,
            "expected UnknownSlashCommand for: {input}"
        );
        assert_ne!(events[n - 2].kind, EventKind::ToolCall);
        assert_ne!(events[n - 1].kind, EventKind::ToolCall);
    }
}

#[test]
fn tool_list_bounded_output_shows_preview_and_trailer() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    let total = 53usize;
    for i in 0..total {
        std::fs::write(workspace_dir.path().join(format!("file_{:02}.txt", i)), "").unwrap();
    }

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    app.input = "/tool list .".to_string();
    app.submit();

    let entry_lines: Vec<_> = app.log.iter().filter(|l| l.starts_with("- ")).collect();
    assert_eq!(
        entry_lines.len(),
        TOOL_LIST_PREVIEW_ENTRIES,
        "expected exactly {} entry lines",
        TOOL_LIST_PREVIEW_ENTRIES
    );

    let expected_trailer = format!("... and {} more", total - TOOL_LIST_PREVIEW_ENTRIES);
    assert!(
        app.log.iter().any(|l| l == &expected_trailer),
        "expected trailer '{}' in log",
        expected_trailer
    );
}

#[test]
fn tool_read_bounded_utf8_boundary_does_not_split_multibyte_char() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    // 4095 'a's + 'é' (2 bytes: 0xC3 0xA9) + 'Z' = 4098 bytes total.
    // At byte offset 4096 we are inside 'é', so the backward scan must
    // retreat to 4095 (the start of 'é'), which is a valid char boundary.
    let content = format!("{}\u{00e9}Z", "a".repeat(4095));
    assert_eq!(content.len(), 4098);

    std::fs::write(workspace_dir.path().join("boundary.txt"), &content).unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    // Must not panic.
    app.input = "/tool read boundary.txt".to_string();
    app.submit();

    // Find the preview line immediately after the header.
    let header_pos = app
        .log
        .iter()
        .position(|l| l == "Tool read boundary.txt:")
        .expect("header line must be present");
    let preview_line = &app.log[header_pos + 1];

    // Preview stops at byte 4095 — the 'é' must not be split.
    assert_eq!(preview_line.len(), 4095, "preview must stop before 'é'");
    assert!(
        !preview_line.contains('\u{00e9}'),
        "preview must not contain 'é'"
    );

    // Truncation marker must be present.
    assert!(
        app.log.iter().any(|l| l == "... [truncated]"),
        "log must contain '... [truncated]'"
    );
}

#[test]
fn tool_read_success_stores_last_tool_output_candidate() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    std::fs::write(workspace_dir.path().join("file.txt"), "hello world").unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    assert!(app.last_tool_output_candidate.is_none());
    app.input = "/tool read file.txt".to_string();
    app.submit();

    assert!(
        app.last_tool_output_candidate.is_some(),
        "last_tool_output_candidate should be set after /tool read success"
    );
}

#[test]
fn tool_list_success_stores_last_tool_output_candidate() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    std::fs::write(workspace_dir.path().join("alpha.txt"), "a").unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    assert!(app.last_tool_output_candidate.is_none());
    app.input = "/tool list .".to_string();
    app.submit();

    assert!(
        app.last_tool_output_candidate.is_some(),
        "last_tool_output_candidate should be set after /tool list success"
    );
}

#[test]
fn tool_read_failure_does_not_update_candidate() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    // Try to read a file that does not exist — this is an error path.
    app.input = "/tool read nonexistent.txt".to_string();
    app.submit();

    assert!(
        app.last_tool_output_candidate.is_none(),
        "last_tool_output_candidate must remain None after a failed /tool"
    );
}

#[test]
fn tool_plan_write_appends_slash_tool_policy_approval_request_events() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    // Pre-create README.md in the workspace so the content-unchanged assertion is meaningful.
    let readme_path = workspace_dir.path().join("README.md");
    let readme_content = b"original readme content";
    std::fs::write(&readme_path, readme_content).unwrap();
    let readme_bytes_before = std::fs::read(&readme_path).unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    let event_len_before = app.event_log.len();
    app.input = "/tool plan-write README.md".to_string();
    app.submit();

    // Event kind sequence must be: SlashCommand, ToolPolicy, ApprovalRequest (exactly 3 new events).
    assert_eq!(app.event_log.len(), event_len_before + 3);
    let events = app.event_log.events();
    let n = events.len();
    assert_eq!(events[n - 3].kind, EventKind::SlashCommand);
    assert_eq!(events[n - 2].kind, EventKind::ToolPolicy);
    assert_eq!(events[n - 1].kind, EventKind::ApprovalRequest);

    // No ToolCall, ToolResult, or ToolError events must appear.
    assert!(!events.iter().any(|e| e.kind == EventKind::ToolCall));
    assert!(!events.iter().any(|e| e.kind == EventKind::ToolResult));
    assert!(!events.iter().any(|e| e.kind == EventKind::ToolError));

    // README.md must not be modified.
    let readme_bytes_after = std::fs::read(&readme_path).unwrap();
    assert_eq!(
        readme_bytes_before, readme_bytes_after,
        "README.md must not be modified by plan-write"
    );

    // State fields must remain None.
    assert!(
        app.last_tool_output_candidate.is_none(),
        "last_tool_output_candidate must not be set by PlanWrite"
    );
    assert!(
        app.pending_manual_tool_context.is_none(),
        "pending_manual_tool_context must not be set by PlanWrite"
    );
    assert!(
        app.pending_model_tool_request.is_none(),
        "pending_model_tool_request must not be set by PlanWrite"
    );

    // Screen log must contain the canonical 3-line approval guidance.
    assert!(
        app.log.iter().any(|l| l == "Write plan requires approval."),
        "log must contain 'Write plan requires approval.'"
    );
    assert!(
        app.log
            .iter()
            .any(|l| l == "Use /approval status to inspect pending approvals."),
        "log must contain approval status guidance"
    );
    assert!(
        app.log
            .iter()
            .any(|l| l == "Use /approval approve <seq> or /approval reject <seq> to resolve."),
        "log must contain approval approve/reject guidance"
    );
}

#[test]
fn tool_success_does_not_auto_clear_pending_model_tool_request() {
    use kernel::model_tool_request::{ModelToolRequest, ModelToolRequestKind};

    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();
    std::fs::write(workspace_dir.path().join("notes.txt"), "hello").unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    // Pre-seed a pending model tool request.
    let req = ModelToolRequest {
        kind: ModelToolRequestKind::ReadFile,
        path: "notes.txt".to_string(),
    };
    app.pending_model_tool_request = Some(req.clone());

    // Run a successful /tool read — must NOT auto-clear pending_model_tool_request.
    app.input = "/tool read notes.txt".to_string();
    app.submit();

    assert_eq!(
        app.pending_model_tool_request,
        Some(req),
        "successful /tool read must not clear pending_model_tool_request (only /request clear does)"
    );

    // Also verify /tool list does not clear it.
    app.input = "/tool list .".to_string();
    app.submit();

    assert!(
        app.pending_model_tool_request.is_some(),
        "successful /tool list must not clear pending_model_tool_request (only /request clear does)"
    );
}
