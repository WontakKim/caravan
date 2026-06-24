use std::collections::BTreeSet;

use super::super::*;
use super::common::*;
use kernel::events::EventKind;
use kernel::manual_context::ManualToolContext;
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

// --- /tool preview-write tests ---

/// Unique sentinel used in preview-write tests to assert no content leaks into events.
const PREVIEW_PROPOSED_SENTINEL: &str = "PREVIEW_PROPOSED_CONTENT_SENTINEL_TUI_7654321";

// (a) No candidate: submitting /tool preview-write emits only SlashCommand and the
//     no-candidate screen-log line. No ToolPolicy/ToolCall/ToolResult/ToolError/ApprovalRequest.
#[test]
fn tool_preview_write_no_candidate_emits_only_slash_command_and_guidance() {
    let mut app = App::new();
    assert!(app.last_tool_output_candidate.is_none());

    let event_len_before = app.event_log.len();
    let log_len_before = app.log.len();
    app.input = "/tool preview-write NOTES.md".to_string();
    app.submit();

    // Exactly one new event: SlashCommand.
    assert_eq!(app.event_log.len(), event_len_before + 1);
    let events = app.event_log.events();
    let n = events.len();
    assert_eq!(events[n - 1].kind, EventKind::SlashCommand);

    // No tool machinery events.
    assert!(
        !events.iter().any(|e| e.kind == EventKind::ToolPolicy),
        "no ToolPolicy must appear when candidate is absent"
    );
    assert!(
        !events.iter().any(|e| e.kind == EventKind::ToolCall),
        "no ToolCall must appear when candidate is absent"
    );
    assert!(
        !events.iter().any(|e| e.kind == EventKind::ToolResult),
        "no ToolResult must appear when candidate is absent"
    );
    assert!(
        !events.iter().any(|e| e.kind == EventKind::ToolError),
        "no ToolError must appear when candidate is absent"
    );
    assert!(
        !events.iter().any(|e| e.kind == EventKind::ApprovalRequest),
        "no ApprovalRequest must appear for preview-write"
    );

    // The no-candidate guidance line must appear in the screen log.
    assert!(
        app.log.len() > log_len_before,
        "log must have grown with the guidance line"
    );
    assert!(
        app.log
            .iter()
            .any(|l| l.contains("No latest tool output to preview")),
        "log must contain the no-candidate guidance"
    );
}

// (b) With a candidate, /tool preview-write yields SlashCommand, ToolPolicy, ToolCall,
//     ToolResult; no ApprovalRequest; bounded diff preview in screen log; ToolResult
//     detail does NOT contain the proposed-content sentinel.
#[test]
fn tool_preview_write_with_candidate_emits_policy_call_result_events() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    // Source file for /tool read.
    std::fs::write(
        workspace_dir.path().join("source.txt"),
        PREVIEW_PROPOSED_SENTINEL,
    )
    .unwrap();
    // Separate target with different content.
    std::fs::write(
        workspace_dir.path().join("target.txt"),
        "old content line\n",
    )
    .unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    // Set up candidate via /tool read.
    app.input = "/tool read source.txt".to_string();
    app.submit();
    assert!(
        app.last_tool_output_candidate.is_some(),
        "candidate must be set after /tool read"
    );

    let event_len_before = app.event_log.len();
    app.input = "/tool preview-write target.txt".to_string();
    app.submit();

    // Four new events: SlashCommand, ToolPolicy, ToolCall, ToolResult.
    assert_eq!(app.event_log.len(), event_len_before + 4);
    let events = app.event_log.events();
    let n = events.len();
    assert_eq!(events[n - 4].kind, EventKind::SlashCommand);
    assert_eq!(events[n - 3].kind, EventKind::ToolPolicy);
    assert_eq!(events[n - 2].kind, EventKind::ToolCall);
    assert_eq!(events[n - 1].kind, EventKind::ToolResult);

    // No ApprovalRequest.
    assert!(
        !events.iter().any(|e| e.kind == EventKind::ApprovalRequest),
        "no ApprovalRequest must appear for preview-write"
    );

    // ToolResult detail must NOT contain the proposed-content sentinel.
    let result_detail = &events[n - 1].detail;
    assert!(
        !result_detail.contains(PREVIEW_PROPOSED_SENTINEL),
        "ToolResult detail must not contain proposed-content sentinel: {result_detail}"
    );

    // Screen log must contain the bounded diff preview header.
    assert!(
        app.log.iter().any(|l| l.contains("Write preview for")),
        "screen log must contain 'Write preview for'"
    );
}

// (c) No-change candidate: renders "No changes." exactly once in screen log.
#[test]
fn tool_preview_write_no_change_renders_no_changes_exactly_once() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    let identical_content = "identical content for no-change test\n";
    std::fs::write(workspace_dir.path().join("same.txt"), identical_content).unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    // Read the file so the candidate content equals the target.
    app.input = "/tool read same.txt".to_string();
    app.submit();

    app.input = "/tool preview-write same.txt".to_string();
    app.submit();

    let no_change_count = app.log.iter().filter(|l| *l == "No changes.").count();
    assert_eq!(
        no_change_count, 1,
        "\"No changes.\" must appear exactly once in the screen log, got {no_change_count}"
    );
}

// (d) Workspace-violation target: yields SlashCommand, ToolPolicy, ToolCall, ToolError;
//     no ApprovalRequest.
#[test]
fn tool_preview_write_workspace_violation_emits_policy_call_error_events() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    std::fs::write(workspace_dir.path().join("source.txt"), "proposed content").unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    // Set up candidate.
    app.input = "/tool read source.txt".to_string();
    app.submit();

    let event_len_before = app.event_log.len();
    app.input = "/tool preview-write ../escape.txt".to_string();
    app.submit();

    assert_eq!(app.event_log.len(), event_len_before + 4);
    let events = app.event_log.events();
    let n = events.len();
    assert_eq!(events[n - 4].kind, EventKind::SlashCommand);
    assert_eq!(events[n - 3].kind, EventKind::ToolPolicy);
    assert_eq!(events[n - 2].kind, EventKind::ToolCall);
    assert_eq!(events[n - 1].kind, EventKind::ToolError);

    assert!(
        !events.iter().any(|e| e.kind == EventKind::ApprovalRequest),
        "no ApprovalRequest must appear for workspace-violation preview"
    );
}

// (e) After a successful /tool preview-write, last_tool_output_candidate,
//     pending_manual_tool_context, and pending_model_tool_request are unchanged.
#[test]
fn tool_preview_write_does_not_mutate_state_fields() {
    use kernel::model_tool_request::{ModelToolRequest, ModelToolRequestKind};

    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    std::fs::write(workspace_dir.path().join("source.txt"), "original content").unwrap();
    std::fs::write(workspace_dir.path().join("target.txt"), "existing content").unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    // Set up candidate.
    app.input = "/tool read source.txt".to_string();
    app.submit();
    let candidate_before = app.last_tool_output_candidate.clone();
    assert!(candidate_before.is_some());

    // Set up a pending model tool request.
    let pending_req = ModelToolRequest {
        kind: ModelToolRequestKind::ReadFile,
        path: "notes.txt".to_string(),
    };
    app.pending_model_tool_request = Some(pending_req.clone());

    // Run preview-write.
    app.input = "/tool preview-write target.txt".to_string();
    app.submit();

    // Candidate must be unchanged.
    let candidate_after = app.last_tool_output_candidate.as_ref().unwrap();
    let candidate_before_ref = candidate_before.as_ref().unwrap();
    assert_eq!(
        candidate_after.content, candidate_before_ref.content,
        "last_tool_output_candidate.content must be unchanged after preview-write"
    );

    // pending_manual_tool_context must remain None.
    assert!(
        app.pending_manual_tool_context.is_none(),
        "pending_manual_tool_context must not be set by preview-write"
    );

    // pending_model_tool_request must remain set to the same value.
    assert_eq!(
        app.pending_model_tool_request,
        Some(pending_req),
        "pending_model_tool_request must not be cleared by preview-write"
    );
}

// (f) DRY-RUN regression: existing target file bytes are byte-for-byte unchanged
//     after /tool preview-write; no temp/sidecar file is created; nonexistent
//     target is NOT created on disk.
#[test]
fn tool_preview_write_dry_run_does_not_modify_existing_file_byte_for_byte() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    let original_bytes = b"original content that must never be changed by a dry-run preview";
    let target_path = workspace_dir.path().join("target.txt");
    std::fs::write(&target_path, original_bytes).unwrap();

    // Record directory entries before the preview.
    let entries_before: BTreeSet<String> = std::fs::read_dir(workspace_dir.path())
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
        .collect();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    // Seed candidate directly with DIFFERENT proposed content.
    app.last_tool_output_candidate = Some(ManualToolContext::from_read_file(
        "target.txt",
        PREVIEW_PROPOSED_SENTINEL,
    ));

    // Run preview-write against the existing target.
    app.input = "/tool preview-write target.txt".to_string();
    app.submit();

    // Target file bytes must be unchanged.
    let bytes_after = std::fs::read(&target_path).unwrap();
    assert_eq!(
        bytes_after, original_bytes,
        "preview-write must not alter the target file's byte content"
    );

    // Directory entry set must be unchanged (no temp/sidecar files created).
    let entries_after: BTreeSet<String> = std::fs::read_dir(workspace_dir.path())
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
        .collect();
    assert_eq!(
        entries_before, entries_after,
        "preview-write must not create any new files in the workspace directory"
    );
}

#[test]
fn tool_preview_write_dry_run_does_not_create_nonexistent_target() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    let nonexistent_path = workspace_dir.path().join("will_not_exist.txt");
    assert!(
        !nonexistent_path.exists(),
        "target must not exist before preview"
    );

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    // Seed candidate with proposed content.
    app.last_tool_output_candidate = Some(ManualToolContext::from_read_file(
        "will_not_exist.txt",
        "proposed content for new file",
    ));

    app.input = "/tool preview-write will_not_exist.txt".to_string();
    app.submit();

    assert!(
        !nonexistent_path.exists(),
        "preview-write must not create the target file"
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
