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

    // Set up candidate (auto-sets pending_manual_tool_context via T-1 behavior).
    app.input = "/tool read source.txt".to_string();
    app.submit();
    let candidate_before = app.last_tool_output_candidate.clone();
    assert!(candidate_before.is_some());
    // After T-1, /tool read auto-sets pending_manual_tool_context.
    let pending_before = app.pending_manual_tool_context.clone();
    assert!(pending_before.is_some());

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

    // pending_manual_tool_context must remain unchanged (preview-write must not clear it).
    let pending_after = app.pending_manual_tool_context.as_ref().expect(
        "pending_manual_tool_context must remain Some after preview-write (auto-set by read)",
    );
    assert_eq!(
        pending_after.content,
        pending_before.as_ref().unwrap().content,
        "pending_manual_tool_context must be unchanged by preview-write"
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

// --- /tool propose-write tests ---

/// Unique sentinel used in propose-write tests to assert no raw content leaks into events.
const PROPOSE_WRITE_SENTINEL: &str = "PROPOSE_WRITE_CONTENT_SENTINEL_TUI_1234567";

// (a) No candidate: submitting /tool propose-write emits only SlashCommand and the
//     no-candidate screen-log line. No ToolPolicy/ToolCall/ToolResult/ToolError/ApprovalRequest.
#[test]
fn tool_propose_write_no_candidate_emits_only_slash_command_and_guidance() {
    let mut app = App::new();
    assert!(app.last_tool_output_candidate.is_none());

    let event_len_before = app.event_log.len();
    let log_len_before = app.log.len();
    app.input = "/tool propose-write NOTES.md".to_string();
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
        "no ApprovalRequest must appear when candidate is absent"
    );

    // The no-candidate guidance line must appear in the screen log.
    assert!(
        app.log.len() > log_len_before,
        "log must have grown with the guidance line"
    );
    assert!(
        app.log
            .iter()
            .any(|l| l.contains("No latest tool output to propose")),
        "log must contain the no-candidate guidance"
    );
}

// (b) Candidate + new-file success: appends 6-event sequence
//     (SlashCommand, ToolPolicy, ToolCall, ToolResult, ToolPolicy, ApprovalRequest)
//     and screen log contains "Write proposal preview for" and the approval lines.
#[test]
fn tool_propose_write_new_file_success_emits_six_event_sequence() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    // Source file for /tool read (target does not exist yet → new-file case).
    std::fs::write(
        workspace_dir.path().join("source.txt"),
        PROPOSE_WRITE_SENTINEL,
    )
    .unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    // Seed candidate via /tool read.
    app.input = "/tool read source.txt".to_string();
    app.submit();
    assert!(app.last_tool_output_candidate.is_some());

    let event_len_before = app.event_log.len();
    // new.txt does not exist → new-file preview.
    app.input = "/tool propose-write new.txt".to_string();
    app.submit();

    // Six new events: SlashCommand, ToolPolicy, ToolCall, ToolResult, ToolPolicy, ApprovalRequest.
    assert_eq!(app.event_log.len(), event_len_before + 6);
    let events = app.event_log.events();
    let n = events.len();
    assert_eq!(events[n - 6].kind, EventKind::SlashCommand);
    assert_eq!(events[n - 5].kind, EventKind::ToolPolicy);
    assert_eq!(events[n - 4].kind, EventKind::ToolCall);
    assert_eq!(events[n - 3].kind, EventKind::ToolResult);
    assert_eq!(events[n - 2].kind, EventKind::ToolPolicy);
    assert_eq!(events[n - 1].kind, EventKind::ApprovalRequest);

    // Screen log must contain the proposal header.
    assert!(
        app.log
            .iter()
            .any(|l| l.contains("Write proposal preview for")),
        "screen log must contain 'Write proposal preview for'"
    );
    // Screen log must contain the approval-request guidance.
    assert!(
        app.log
            .iter()
            .any(|l| l == "Approval requested for proposed write."),
        "screen log must contain 'Approval requested for proposed write.'"
    );
    assert!(
        app.log
            .iter()
            .any(|l| l == "Use /approval status to inspect pending approvals."),
        "screen log must contain approval status guidance"
    );
    assert!(
        app.log
            .iter()
            .any(|l| l == "Use /approval approve <seq> or /approval reject <seq> to resolve."),
        "screen log must contain approval resolve guidance"
    );
}

// (c) Candidate + existing-file success: same 6-event sequence.
#[test]
fn tool_propose_write_existing_file_success_emits_six_event_sequence() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    std::fs::write(
        workspace_dir.path().join("source.txt"),
        PROPOSE_WRITE_SENTINEL,
    )
    .unwrap();
    std::fs::write(workspace_dir.path().join("target.txt"), "old content\n").unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    app.input = "/tool read source.txt".to_string();
    app.submit();
    assert!(app.last_tool_output_candidate.is_some());

    let event_len_before = app.event_log.len();
    app.input = "/tool propose-write target.txt".to_string();
    app.submit();

    assert_eq!(app.event_log.len(), event_len_before + 6);
    let events = app.event_log.events();
    let n = events.len();
    assert_eq!(events[n - 6].kind, EventKind::SlashCommand);
    assert_eq!(events[n - 5].kind, EventKind::ToolPolicy);
    assert_eq!(events[n - 4].kind, EventKind::ToolCall);
    assert_eq!(events[n - 3].kind, EventKind::ToolResult);
    assert_eq!(events[n - 2].kind, EventKind::ToolPolicy);
    assert_eq!(events[n - 1].kind, EventKind::ApprovalRequest);
}

// (d) Candidate + no-change success: screen log shows "No changes." exactly once.
#[test]
fn tool_propose_write_no_change_renders_no_changes_exactly_once() {
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

    app.input = "/tool read same.txt".to_string();
    app.submit();

    app.input = "/tool propose-write same.txt".to_string();
    app.submit();

    let no_change_count = app.log.iter().filter(|l| *l == "No changes.").count();
    assert_eq!(
        no_change_count, 1,
        "\"No changes.\" must appear exactly once in the screen log, got {no_change_count}"
    );
    // Six events still expected (NoChange is a valid preview kind, approval still requested).
    let events = app.event_log.events();
    let n = events.len();
    assert_eq!(events[n - 1].kind, EventKind::ApprovalRequest);
}

// (e) Candidate + workspace-violation: appends SlashCommand, ToolPolicy, ToolCall, ToolError;
//     NO ApprovalRequest; screen log contains "Write proposal preview failed:".
#[test]
fn tool_propose_write_workspace_violation_emits_policy_call_error_no_approval() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    std::fs::write(workspace_dir.path().join("source.txt"), "proposed content").unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    app.input = "/tool read source.txt".to_string();
    app.submit();

    let event_len_before = app.event_log.len();
    app.input = "/tool propose-write ../escape.txt".to_string();
    app.submit();

    // Four events: SlashCommand, ToolPolicy, ToolCall, ToolError.
    assert_eq!(app.event_log.len(), event_len_before + 4);
    let events = app.event_log.events();
    let n = events.len();
    assert_eq!(events[n - 4].kind, EventKind::SlashCommand);
    assert_eq!(events[n - 3].kind, EventKind::ToolPolicy);
    assert_eq!(events[n - 2].kind, EventKind::ToolCall);
    assert_eq!(events[n - 1].kind, EventKind::ToolError);

    assert!(
        !events.iter().any(|e| e.kind == EventKind::ApprovalRequest),
        "no ApprovalRequest must appear on workspace-violation"
    );

    assert!(
        app.log
            .iter()
            .any(|l| l.contains("Write proposal preview failed:")),
        "screen log must contain 'Write proposal preview failed:'"
    );
}

// (f) Success does not modify/create the target file and creates no temp file.
#[test]
fn tool_propose_write_dry_run_does_not_modify_existing_file() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    let original_bytes = b"original content that must never be changed by propose-write";
    let target_path = workspace_dir.path().join("target.txt");
    std::fs::write(&target_path, original_bytes).unwrap();

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

    app.last_tool_output_candidate = Some(ManualToolContext::from_read_file(
        "target.txt",
        PROPOSE_WRITE_SENTINEL,
    ));

    app.input = "/tool propose-write target.txt".to_string();
    app.submit();

    let bytes_after = std::fs::read(&target_path).unwrap();
    assert_eq!(
        bytes_after, original_bytes,
        "propose-write must not alter the target file's byte content"
    );

    let entries_after: BTreeSet<String> = std::fs::read_dir(workspace_dir.path())
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
        .collect();
    assert_eq!(
        entries_before, entries_after,
        "propose-write must not create any new files in the workspace directory"
    );
}

#[test]
fn tool_propose_write_dry_run_does_not_create_nonexistent_target() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    let nonexistent_path = workspace_dir.path().join("will_not_exist.txt");
    assert!(!nonexistent_path.exists());

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    app.last_tool_output_candidate = Some(ManualToolContext::from_read_file(
        "will_not_exist.txt",
        "proposed content for new file",
    ));

    app.input = "/tool propose-write will_not_exist.txt".to_string();
    app.submit();

    assert!(
        !nonexistent_path.exists(),
        "propose-write must not create the target file"
    );
}

// (g) Success leaves last_tool_output_candidate, pending_manual_tool_context,
//     and pending_model_tool_request unchanged.
#[test]
fn tool_propose_write_does_not_mutate_state_fields() {
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

    app.input = "/tool read source.txt".to_string();
    app.submit();
    let candidate_before = app.last_tool_output_candidate.clone();
    assert!(candidate_before.is_some());
    // After T-1, /tool read auto-sets pending_manual_tool_context.
    let pending_before = app.pending_manual_tool_context.clone();
    assert!(pending_before.is_some());

    let pending_req = ModelToolRequest {
        kind: ModelToolRequestKind::ReadFile,
        path: "notes.txt".to_string(),
    };
    app.pending_model_tool_request = Some(pending_req.clone());

    app.input = "/tool propose-write target.txt".to_string();
    app.submit();

    let candidate_after = app.last_tool_output_candidate.as_ref().unwrap();
    let candidate_before_ref = candidate_before.as_ref().unwrap();
    assert_eq!(
        candidate_after.content, candidate_before_ref.content,
        "last_tool_output_candidate.content must be unchanged after propose-write"
    );

    // pending_manual_tool_context must remain unchanged (propose-write must not clear it).
    let pending_after = app.pending_manual_tool_context.as_ref().expect(
        "pending_manual_tool_context must remain Some after propose-write (auto-set by read)",
    );
    assert_eq!(
        pending_after.content,
        pending_before.as_ref().unwrap().content,
        "pending_manual_tool_context must be unchanged by propose-write"
    );

    assert_eq!(
        app.pending_model_tool_request,
        Some(pending_req),
        "pending_model_tool_request must not be cleared by propose-write"
    );
}

// (h) ToolResult detail contains no +/- diff line and no file-content sentinel.
#[test]
fn tool_propose_write_tool_result_detail_contains_no_diff_lines_and_no_sentinel() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    std::fs::write(
        workspace_dir.path().join("source.txt"),
        PROPOSE_WRITE_SENTINEL,
    )
    .unwrap();
    std::fs::write(workspace_dir.path().join("target.txt"), "old content\n").unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    app.input = "/tool read source.txt".to_string();
    app.submit();

    app.input = "/tool propose-write target.txt".to_string();
    app.submit();

    let events = app.event_log.events();
    let n = events.len();
    assert_eq!(events[n - 3].kind, EventKind::ToolResult);
    let result_detail = &events[n - 3].detail;

    // Must not contain the raw proposed-content sentinel.
    assert!(
        !result_detail.contains(PROPOSE_WRITE_SENTINEL),
        "ToolResult detail must not contain the proposed-content sentinel: {result_detail}"
    );
    // Must not contain unified-diff addition or removal lines.
    assert!(
        !result_detail
            .lines()
            .any(|l| l.starts_with("+ ") || l.starts_with("- ")),
        "ToolResult detail must not contain diff lines: {result_detail}"
    );
}

// (i) ApprovalRequest detail contains preview_kind=/truncated= but no +/- diff line
//     and no file-content sentinel, and its parsed to_tool_request() is None (non-resumable).
#[test]
fn tool_propose_write_approval_request_detail_is_summary_only_and_non_resumable() {
    use kernel::approval::ParsedApprovalRequest;

    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    std::fs::write(
        workspace_dir.path().join("source.txt"),
        PROPOSE_WRITE_SENTINEL,
    )
    .unwrap();
    std::fs::write(workspace_dir.path().join("target.txt"), "old content\n").unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    app.input = "/tool read source.txt".to_string();
    app.submit();

    app.input = "/tool propose-write target.txt".to_string();
    app.submit();

    let events = app.event_log.events();
    let n = events.len();
    assert_eq!(events[n - 1].kind, EventKind::ApprovalRequest);
    let approval_detail = &events[n - 1].detail;

    // Must contain summary fields.
    assert!(
        approval_detail.contains("preview_kind="),
        "ApprovalRequest detail must contain 'preview_kind=': {approval_detail}"
    );
    assert!(
        approval_detail.contains("truncated="),
        "ApprovalRequest detail must contain 'truncated=': {approval_detail}"
    );

    // Must not contain raw proposed-content sentinel.
    assert!(
        !approval_detail.contains(PROPOSE_WRITE_SENTINEL),
        "ApprovalRequest detail must not contain the proposed-content sentinel: {approval_detail}"
    );

    // Must not contain unified-diff lines.
    assert!(
        !approval_detail
            .lines()
            .any(|l| l.starts_with("+ ") || l.starts_with("- ")),
        "ApprovalRequest detail must not contain diff lines: {approval_detail}"
    );

    // Must be non-resumable: to_tool_request() returns None for write_file.
    let parsed = ParsedApprovalRequest::parse_detail(approval_detail)
        .expect("ApprovalRequest detail must be parseable");
    assert_eq!(
        parsed.to_tool_request(),
        None,
        "propose-write ApprovalRequest must be non-resumable (write_file not in resumable set)"
    );
}

// --- Auto-attached workspace context tests (T-1 behavior) ---

// Step 1: /tool read auto-sets pending_manual_tool_context and last_tool_output_candidate,
// and pushes the guidance line into app.log.
#[test]
fn tool_read_auto_sets_pending_manual_tool_context_and_pushes_guidance() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    std::fs::write(workspace_dir.path().join("data.txt"), "auto-set content").unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    assert!(app.pending_manual_tool_context.is_none());
    assert!(app.last_tool_output_candidate.is_none());

    app.input = "/tool read data.txt".to_string();
    app.submit();

    let pending = app
        .pending_manual_tool_context
        .as_ref()
        .expect("pending_manual_tool_context must be auto-set after successful /tool read");
    let candidate = app
        .last_tool_output_candidate
        .as_ref()
        .expect("last_tool_output_candidate must be set after successful /tool read");
    // Both fields must carry the actual read output, and must be the same value
    // (built once and cloned, per D1).
    assert!(
        pending.content.contains("auto-set content"),
        "pending content must carry the read output"
    );
    assert_eq!(
        candidate.content, pending.content,
        "candidate and pending must be the same auto-set context"
    );
    assert!(
        app.log
            .iter()
            .any(|l| l
                == "This tool output will be used as workspace context for your next message."),
        "guidance line must be pushed into app.log after /tool read"
    );
}

// Step 2: /tool list auto-sets pending_manual_tool_context and last_tool_output_candidate,
// and pushes the guidance line into app.log.
#[test]
fn tool_list_auto_sets_pending_manual_tool_context_and_pushes_guidance() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    std::fs::write(workspace_dir.path().join("entry.txt"), "e").unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    assert!(app.pending_manual_tool_context.is_none());
    assert!(app.last_tool_output_candidate.is_none());

    app.input = "/tool list .".to_string();
    app.submit();

    let pending = app
        .pending_manual_tool_context
        .as_ref()
        .expect("pending_manual_tool_context must be auto-set after successful /tool list");
    let candidate = app
        .last_tool_output_candidate
        .as_ref()
        .expect("last_tool_output_candidate must be set after successful /tool list");
    // Both fields must carry the actual list output (the known entry), and must
    // be the same value (built once and cloned, per D1).
    assert!(
        pending.content.contains("entry.txt"),
        "pending content must carry the list output entry"
    );
    assert_eq!(
        candidate.content, pending.content,
        "candidate and pending must be the same auto-set context"
    );
    assert!(
        app.log
            .iter()
            .any(|l| l
                == "This tool output will be used as workspace context for your next message."),
        "guidance line must be pushed into app.log after /tool list"
    );
}

// Step 3: After /tool read, a plain-text user message produces a PromptCompile event
// whose detail contains "Workspace Context:", "Attached Workspace Context:", and the bounded
// file content appearing AFTER "Workspace Context:" (position-based check).
#[test]
fn tool_read_auto_set_prompt_compile_contains_workspace_and_manual_context() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();
    let file_content = "unique_read_content_for_prompt_test_xyz";
    std::fs::write(workspace_dir.path().join("prompt_test.txt"), file_content).unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    app.input = "/tool read prompt_test.txt".to_string();
    app.submit();
    assert!(app.pending_manual_tool_context.is_some());

    app.input = "use the file content".to_string();
    app.submit();

    // pending is cleared after one-shot consumption.
    assert!(app.pending_manual_tool_context.is_none());

    let events = app.event_log.events();
    let pc = events
        .iter()
        .filter(|e| e.kind == EventKind::PromptCompile)
        .next()
        .expect("PromptCompile event should exist");

    // Ordered chain proves the content is rendered INSIDE the Workspace Context
    // section's Attached Workspace Context block, not merely somewhere after the header.
    let ws_pos = pc
        .detail
        .find("Workspace Context:")
        .expect("PromptCompile detail must contain 'Workspace Context:'");
    let manual_pos = pc
        .detail
        .find("Attached Workspace Context:")
        .expect("PromptCompile detail must contain 'Attached Workspace Context:'");
    let content_pos = pc
        .detail
        .find(file_content)
        .expect("PromptCompile detail must contain the bounded file content");
    assert!(
        ws_pos < manual_pos && manual_pos < content_pos,
        "expected order Workspace Context: < Attached Workspace Context: < file content"
    );
}

// Step 4: After /tool list, a plain-text user message produces a PromptCompile event
// whose detail contains a known entry name under the Workspace Context section,
// verified with a position-based check.
#[test]
fn tool_list_auto_set_prompt_compile_contains_workspace_context_section() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();
    let known_entry = "unique_list_entry_abc.txt";
    std::fs::write(workspace_dir.path().join(known_entry), "x").unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    app.input = "/tool list .".to_string();
    app.submit();
    assert!(app.pending_manual_tool_context.is_some());

    app.input = "what files are there".to_string();
    app.submit();

    assert!(app.pending_manual_tool_context.is_none());

    let events = app.event_log.events();
    let pc = events
        .iter()
        .filter(|e| e.kind == EventKind::PromptCompile)
        .next()
        .expect("PromptCompile event should exist");

    // Ordered chain proves the list output is rendered INSIDE the Workspace
    // Context section's Attached Workspace Context block (the same wrapper as read).
    let ws_pos = pc
        .detail
        .find("Workspace Context:")
        .expect("PromptCompile detail must contain 'Workspace Context:'");
    let manual_pos = pc
        .detail
        .find("Attached Workspace Context:")
        .expect("PromptCompile detail must contain 'Attached Workspace Context:' for list output");
    let entry_pos = pc
        .detail
        .find(known_entry)
        .expect("PromptCompile detail must contain the known entry name from the list");
    assert!(
        ws_pos < manual_pos && manual_pos < entry_pos,
        "expected order Workspace Context: < Attached Workspace Context: < list entry"
    );
}

// Step 5: One-shot clearing — after /tool read then one user message,
// pending_manual_tool_context is None; a SECOND user message's PromptCompile
// detail does NOT contain "Attached Workspace Context:" or the bounded content.
#[test]
fn tool_read_auto_set_context_is_one_shot() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();
    let file_content = "one_shot_content_sentinel_abc123";
    std::fs::write(workspace_dir.path().join("oneshot.txt"), file_content).unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    app.input = "/tool read oneshot.txt".to_string();
    app.submit();
    assert!(app.pending_manual_tool_context.is_some());

    // First user message consumes the context.
    app.input = "first message".to_string();
    app.submit();

    assert!(
        app.pending_manual_tool_context.is_none(),
        "pending_manual_tool_context must be None after the first user message (one-shot)"
    );

    // Second user message must NOT include the context.
    app.input = "second message".to_string();
    app.submit();

    let events = app.event_log.events();
    let second_pc = events
        .iter()
        .filter(|e| e.kind == EventKind::PromptCompile)
        .nth(1)
        .expect("second PromptCompile event should exist");

    assert!(
        !second_pc.detail.contains("Attached Workspace Context:"),
        "second PromptCompile must NOT contain 'Attached Workspace Context:' (context was one-shot)"
    );
    assert!(
        !second_pc.detail.contains(file_content),
        "second PromptCompile must NOT contain the bounded content (context was one-shot)"
    );
}

// Step 6: A FAILED /tool read does not disturb pending_manual_tool_context.
// Sub-case (a): clean state → failed read → pending stays None.
// Sub-case (b): after a successful read (pending = Some), a failed read leaves
// the original pending intact (still Some, same content).
#[test]
fn tool_read_failure_leaves_pending_context_unchanged() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    std::fs::write(workspace_dir.path().join("real.txt"), "real content").unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    // Sub-case (a): clean state — a failed read (workspace violation) leaves pending None.
    assert!(app.pending_manual_tool_context.is_none());
    app.input = "/tool read ../outside.txt".to_string();
    app.submit();
    assert!(
        app.pending_manual_tool_context.is_none(),
        "sub-case (a): pending_manual_tool_context must remain None after a failed /tool read"
    );

    // Sub-case (b): after a successful read, a subsequent failed read leaves pending intact.
    app.input = "/tool read real.txt".to_string();
    app.submit();
    assert!(app.pending_manual_tool_context.is_some());
    let original_content = app
        .pending_manual_tool_context
        .as_ref()
        .unwrap()
        .content
        .clone();
    let guidance = "This tool output will be used as workspace context for your next message.";
    let guidance_before = app.log.iter().filter(|l| *l == guidance).count();

    // Failed read: missing file.
    app.input = "/tool read nonexistent_missing.txt".to_string();
    app.submit();

    assert!(
        app.pending_manual_tool_context.is_some(),
        "sub-case (b): pending_manual_tool_context must remain Some after a failed /tool read"
    );
    assert_eq!(
        app.pending_manual_tool_context.as_ref().unwrap().content,
        original_content,
        "sub-case (b): pending_manual_tool_context content must be unchanged after a failed read"
    );
    // A failed read must NOT push the success guidance line (guidance is success-only).
    assert_eq!(
        app.log.iter().filter(|l| *l == guidance).count(),
        guidance_before,
        "sub-case (b): a failed read must not push the success guidance line"
    );
}

// Step 7: A successful /tool read auto-sets pending but emits NO ToolContextAttach event.
#[test]
fn tool_read_auto_set_emits_no_tool_context_attach_event() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    std::fs::write(workspace_dir.path().join("noattach.txt"), "content").unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    std::fs::write(workspace_dir.path().join("listme.txt"), "x").unwrap();

    // /tool read auto-set: no ToolContextAttach.
    app.input = "/tool read noattach.txt".to_string();
    app.submit();
    assert!(
        app.pending_manual_tool_context.is_some(),
        "pending_manual_tool_context must be set after /tool read"
    );
    // /tool list auto-set: also no ToolContextAttach.
    app.input = "/tool list .".to_string();
    app.submit();
    assert!(app.pending_manual_tool_context.is_some());
    // Consume the context with a user message — must not emit ToolContextAttach
    // during submit either.
    app.input = "use it".to_string();
    app.submit();

    let events = app.event_log.events();
    assert!(
        events
            .iter()
            .all(|e| e.kind != EventKind::ToolContextAttach),
        "auto-set on /tool read or /tool list (and its later consumption) must NOT emit a ToolContextAttach event"
    );
}

// Step 8: A second successful /tool read of a different file overwrites
// pending_manual_tool_context with the newer file's content (last-write-wins, no panic).
#[test]
fn tool_read_second_success_overwrites_pending_last_write_wins() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    std::fs::write(
        workspace_dir.path().join("first.txt"),
        "content of first file",
    )
    .unwrap();
    std::fs::write(
        workspace_dir.path().join("second.txt"),
        "content of second file",
    )
    .unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    // First read.
    app.input = "/tool read first.txt".to_string();
    app.submit();
    assert!(app.pending_manual_tool_context.is_some());
    let first_content = app
        .pending_manual_tool_context
        .as_ref()
        .unwrap()
        .content
        .clone();

    // Second read of a different file — must overwrite pending (no panic).
    app.input = "/tool read second.txt".to_string();
    app.submit();

    assert!(
        app.pending_manual_tool_context.is_some(),
        "pending_manual_tool_context must remain Some after second /tool read"
    );
    let second_content = app
        .pending_manual_tool_context
        .as_ref()
        .unwrap()
        .content
        .clone();

    assert_ne!(
        first_content, second_content,
        "pending_manual_tool_context must be overwritten with the second file's content"
    );
    assert!(
        second_content.contains("content of second file"),
        "pending must reflect the second file's content (last-write-wins)"
    );
    // Last-write-wins means OVERWRITE, not append/merge: the first file's
    // content must be gone from pending.
    assert!(
        !second_content.contains("content of first file"),
        "pending must NOT retain the first file's content (overwrite, not append)"
    );
}

// --- /tool search tests ---

// (a) Hit: emits SlashCommand, ToolPolicy, ToolCall, ToolResult; sets candidate; no
//     ToolContextAttach; no ApprovalRequest; pending context clears one-shot after next message.
#[test]
fn tool_search_hit_emits_policy_call_result_and_stages_context() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    std::fs::write(workspace_dir.path().join("greet.txt"), "hello world\n").unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    let event_len_before = app.event_log.len();
    app.input = "/tool search hello".to_string();
    app.submit();

    // Exactly 4 new events: SlashCommand, ToolPolicy, ToolCall, ToolResult.
    assert_eq!(app.event_log.len(), event_len_before + 4);
    let events = app.event_log.events();
    let n = events.len();
    assert_eq!(events[n - 4].kind, EventKind::SlashCommand);
    assert_eq!(events[n - 3].kind, EventKind::ToolPolicy);
    assert_eq!(events[n - 2].kind, EventKind::ToolCall);
    assert_eq!(events[n - 1].kind, EventKind::ToolResult);

    // No ApprovalRequest emitted.
    assert!(
        !events.iter().any(|e| e.kind == EventKind::ApprovalRequest),
        "no ApprovalRequest must appear for search_text"
    );
    // No ToolContextAttach emitted on automatic staging.
    assert!(
        !events
            .iter()
            .any(|e| e.kind == EventKind::ToolContextAttach),
        "no ToolContextAttach must appear on automatic search staging"
    );

    // last_tool_output_candidate is set.
    assert!(
        app.last_tool_output_candidate.is_some(),
        "last_tool_output_candidate must be set after /tool search hit"
    );
    // pending_manual_tool_context is set.
    assert!(
        app.pending_manual_tool_context.is_some(),
        "pending_manual_tool_context must be set after /tool search hit"
    );

    // One-shot clear: a user message consumes the pending context.
    app.input = "use the search results".to_string();
    app.submit();
    assert!(
        app.pending_manual_tool_context.is_none(),
        "pending_manual_tool_context must be cleared after the next user message (one-shot)"
    );
}

// (b) Miss: logs "No matches." and still stages context.
#[test]
fn tool_search_miss_logs_no_matches_and_stages_context() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    std::fs::write(workspace_dir.path().join("file.txt"), "nothing here\n").unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    app.input = "/tool search ZZZNOMATCH".to_string();
    app.submit();

    // "No matches." must appear in screen log.
    assert!(
        app.log.iter().any(|l| l == "No matches."),
        "log must contain 'No matches.' for a search with no results"
    );

    // pending context still staged (no-matches is a successful result).
    assert!(
        app.pending_manual_tool_context.is_some(),
        "pending_manual_tool_context must be staged even on no-matches"
    );
    assert!(
        app.last_tool_output_candidate.is_some(),
        "last_tool_output_candidate must be set even on no-matches"
    );
}

// (c) Failure: does NOT stage pending context.
#[test]
fn tool_search_failure_does_not_stage_context() {
    // Use a workspace root that causes search to fail (empty query goes to Unknown,
    // so we use a workspace that has the root unreadable — but that's tricky.
    // Instead, use a query that's empty which the parser maps to Unknown, so we
    // induce a registry-level error via a workaround: submit a direct SearchText
    // via the registry with an empty query is caught at parser level as Unknown.
    // Better: seed a candidate first, then submit a failing search (workspace violation).
    // We can't easily make search_workspace fail for a non-empty query without a
    // broken workspace root. Instead we test via an unreachable workspace root path
    // indirectly: create app with a non-existent workspace root so the search fails.
    let store_dir = TempDir::new();
    // Point workspace root at a non-existent path so canonicalization fails.
    let nonexistent_root = store_dir.path().join("nonexistent_workspace");
    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        nonexistent_root,
    );

    // Seed a candidate first.
    app.last_tool_output_candidate = Some(ManualToolContext::from_read_file("prior.txt", "prior"));

    app.input = "/tool search hello".to_string();
    app.submit();

    // pending_manual_tool_context must NOT be set (failure path).
    assert!(
        app.pending_manual_tool_context.is_none(),
        "pending_manual_tool_context must not be set after a failed /tool search"
    );
    // last_tool_output_candidate must NOT be updated (failure path preserves old value).
    assert!(
        app.last_tool_output_candidate.is_some(),
        "last_tool_output_candidate must remain at its prior value after a failed /tool search"
    );
}

// (d) /context attach-last-tool works with a staged search candidate.
#[test]
fn context_attach_last_tool_works_with_search_candidate() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    std::fs::write(workspace_dir.path().join("src.rs"), "fn main() {}\n").unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    // Run a search to set the candidate.
    app.input = "/tool search fn main".to_string();
    app.submit();
    assert!(
        app.last_tool_output_candidate.is_some(),
        "search must set last_tool_output_candidate"
    );

    // /context attach-last-tool should work.
    let log_len_before = app.log.len();
    app.input = "/context attach-last-tool".to_string();
    app.submit();

    // The screen log must have grown (success path).
    assert!(
        app.log.len() > log_len_before,
        "screen log must grow after /context attach-last-tool with a search candidate"
    );
    // pending_manual_tool_context must be set.
    assert!(
        app.pending_manual_tool_context.is_some(),
        "pending_manual_tool_context must be set after /context attach-last-tool with search candidate"
    );

    // A ToolContextAttach event must be emitted for explicit attach.
    let events = app.event_log.events();
    assert!(
        events
            .iter()
            .any(|e| e.kind == EventKind::ToolContextAttach),
        "ToolContextAttach must be emitted for explicit /context attach-last-tool"
    );
}

// --- /tool glob tests ---

// (a) Hit: emits SlashCommand, ToolPolicy, ToolCall, ToolResult; sets both candidate
//     fields; no ToolContextAttach; pending context clears one-shot after next message.
#[test]
fn tool_glob_hit_emits_policy_call_result_and_stages_context() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    std::fs::write(workspace_dir.path().join("hello.rs"), "fn main() {}").unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    let event_len_before = app.event_log.len();
    app.input = "/tool glob **/*.rs".to_string();
    app.submit();

    // Exactly 4 new events: SlashCommand, ToolPolicy, ToolCall, ToolResult.
    assert_eq!(app.event_log.len(), event_len_before + 4);
    let events = app.event_log.events();
    let n = events.len();
    assert_eq!(events[n - 4].kind, EventKind::SlashCommand);
    assert_eq!(events[n - 3].kind, EventKind::ToolPolicy);
    assert_eq!(events[n - 2].kind, EventKind::ToolCall);
    assert_eq!(events[n - 1].kind, EventKind::ToolResult);

    // No ApprovalRequest emitted.
    assert!(
        !events.iter().any(|e| e.kind == EventKind::ApprovalRequest),
        "no ApprovalRequest must appear for glob_files"
    );
    // No ToolContextAttach emitted on automatic staging.
    assert!(
        !events
            .iter()
            .any(|e| e.kind == EventKind::ToolContextAttach),
        "no ToolContextAttach must appear on automatic glob staging"
    );

    // Both context fields are set.
    assert!(
        app.last_tool_output_candidate.is_some(),
        "last_tool_output_candidate must be set after /tool glob hit"
    );
    assert!(
        app.pending_manual_tool_context.is_some(),
        "pending_manual_tool_context must be set after /tool glob hit"
    );

    // Screen log contains the header.
    assert!(
        app.log.iter().any(|l| l.contains("Glob results for")),
        "log must contain 'Glob results for'"
    );

    // One-shot clear: a user message consumes the pending context.
    app.input = "use the glob results".to_string();
    app.submit();
    assert!(
        app.pending_manual_tool_context.is_none(),
        "pending_manual_tool_context must be cleared after the next user message (one-shot)"
    );
}

// (b) Miss: logs "No matches." and still stages context.
#[test]
fn tool_glob_miss_logs_no_matches_and_stages_context() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    std::fs::write(workspace_dir.path().join("file.txt"), "content").unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    app.input = "/tool glob **/*.rs".to_string();
    app.submit();

    // "No matches." must appear in screen log.
    assert!(
        app.log.iter().any(|l| l == "No matches."),
        "log must contain 'No matches.' for a glob with no results"
    );

    // pending context still staged (no-matches is a successful result).
    assert!(
        app.pending_manual_tool_context.is_some(),
        "pending_manual_tool_context must be staged even on no-matches"
    );
    assert!(
        app.last_tool_output_candidate.is_some(),
        "last_tool_output_candidate must be set even on no-matches"
    );
}

// (c) Failure (workspace violation via ..): stages nothing.
#[test]
fn tool_glob_failure_does_not_stage_context() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    // Seed an existing candidate to confirm it is not overwritten on failure.
    app.last_tool_output_candidate = Some(ManualToolContext::from_read_file(
        "prior.txt",
        "prior content",
    ));

    app.input = "/tool glob ../**/*.rs".to_string();
    app.submit();

    // pending_manual_tool_context must NOT be set (failure path).
    assert!(
        app.pending_manual_tool_context.is_none(),
        "pending_manual_tool_context must not be set after a failed /tool glob"
    );
    // last_tool_output_candidate must NOT be updated (failure preserves old value).
    let candidate = app.last_tool_output_candidate.as_ref().unwrap();
    assert_eq!(
        candidate.content, "prior content",
        "last_tool_output_candidate must remain at its prior value after a failed /tool glob"
    );
}

// (j) Candidate-source preservation: a second propose-write still reflects the
//     original last_tool_output_candidate content, not the prior ToolResult summary.
//     No event detail anywhere contains the raw sentinel.
#[test]
fn tool_propose_write_candidate_source_preserved_across_sequential_proposals() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    // Source file whose content is the sentinel.
    std::fs::write(
        workspace_dir.path().join("source.txt"),
        PROPOSE_WRITE_SENTINEL,
    )
    .unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    // Seed candidate via /tool read.
    app.input = "/tool read source.txt".to_string();
    app.submit();
    assert!(app.last_tool_output_candidate.is_some());

    // First propose-write: proposal1.txt (new file).
    app.input = "/tool propose-write proposal1.txt".to_string();
    app.submit();

    // Second propose-write: proposal2.txt (also new file).
    app.input = "/tool propose-write proposal2.txt".to_string();
    app.submit();

    // The second proposal must have emitted a 6-event sequence.
    let events = app.event_log.events();
    let n = events.len();
    assert_eq!(events[n - 1].kind, EventKind::ApprovalRequest);
    assert_eq!(events[n - 2].kind, EventKind::ToolPolicy);
    assert_eq!(events[n - 3].kind, EventKind::ToolResult);

    // The second ToolResult detail must reflect writing SENTINEL content to proposal2.txt.
    // new_bytes must equal the byte length of the sentinel (proving candidate wasn't replaced
    // by the first proposal's ToolResult summary).
    let result_detail = &events[n - 3].detail;
    let sentinel_bytes = PROPOSE_WRITE_SENTINEL.len();
    assert!(
        result_detail.contains(&format!("new_bytes={}", sentinel_bytes)),
        "second ToolResult detail must show new_bytes={} (sentinel length): {result_detail}",
        sentinel_bytes
    );

    // No event detail anywhere must contain the raw sentinel content.
    for event in events.iter() {
        assert!(
            !event.detail.contains(PROPOSE_WRITE_SENTINEL),
            "event detail must not contain the raw sentinel (kind={:?}): {}",
            event.kind,
            event.detail
        );
    }
}
