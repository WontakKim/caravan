use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::*;
use crate::approval::ParsedApprovalRequest;
use crate::events::{EventKind, EventLog};
use crate::model::tool_use::format_tool_output_for_model;
use crate::storage::EventStore;
use crate::tool::registry::{ToolExecutionContext, ToolOutput};
use crate::write_intent::{WriteIntentMode, WriteIntentSource, new_text};
use crate::write_preview::preview_write_intent;

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> Self {
        let count = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let name = format!("caravan_tool_events_test_{}_{}", std::process::id(), count);
        let path = std::env::temp_dir().join(name);
        std::fs::create_dir_all(&path).expect("failed to create temp dir");
        TempDir { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn make_context(root: &Path) -> ToolExecutionContext {
    ToolExecutionContext {
        workspace_root: root.to_path_buf(),
    }
}

#[test]
fn list_files_success_appends_tool_call_and_result() {
    let dir = TempDir::new();
    let ctx = make_context(dir.path());
    let runner = ToolEventRunner::new_readonly();
    let mut log = EventLog::new();

    let result = runner.run(
        &mut log,
        &ctx,
        ToolRequest::ListFiles {
            path: ".".to_string(),
        },
    );

    assert!(result.is_ok(), "expected Ok, got {:?}", result);
    assert_eq!(log.len(), 3);
    assert_eq!(log.get(0).unwrap().kind, EventKind::ToolPolicy);
    assert_eq!(log.get(1).unwrap().kind, EventKind::ToolCall);
    assert_eq!(log.get(2).unwrap().kind, EventKind::ToolResult);
}

#[test]
fn read_file_escape_appends_tool_call_and_error() {
    let dir = TempDir::new();
    let ctx = make_context(dir.path());
    let runner = ToolEventRunner::new_readonly();
    let mut log = EventLog::new();

    let result = runner.run(
        &mut log,
        &ctx,
        ToolRequest::ReadFile {
            path: "../escape".to_string(),
            offset: None,
            limit: None,
        },
    );

    assert!(
        matches!(result, Err(ToolError::WorkspaceViolation { .. })),
        "expected WorkspaceViolation, got {:?}",
        result
    );
    assert_eq!(log.len(), 3);
    assert_eq!(log.get(0).unwrap().kind, EventKind::ToolPolicy);
    assert_eq!(log.get(1).unwrap().kind, EventKind::ToolCall);
    assert_eq!(log.get(2).unwrap().kind, EventKind::ToolError);
}

#[test]
fn format_tool_call_detail_uses_debug_path_and_risk() {
    let detail = format_tool_call_detail("list_files", ".", None, None);
    assert_eq!(detail, r#"tool=list_files path="." risk=read_only"#);
}

#[test]
fn format_tool_result_detail_file_list_uses_entry_count() {
    let output = ToolOutput::FileList {
        path: ".".to_string(),
        entries: vec!["a".to_string(), "b".to_string()],
    };
    let detail = format_tool_result_detail("list_files", ".", &output);
    assert_eq!(detail, r#"tool=list_files path="." entries=2"#);
}

#[test]
fn format_tool_result_detail_file_content_uses_byte_length() {
    let output = ToolOutput::FileContent {
        path: "readme.md".to_string(),
        content: "hello".to_string(),
        start_line: None,
        line_count: None,
        truncated: false,
    };
    let detail = format_tool_result_detail("read_file", "readme.md", &output);
    assert_eq!(detail, r#"tool=read_file path="readme.md" bytes=5"#);
}

#[test]
fn format_tool_error_detail_workspace_violation() {
    let error = ToolError::WorkspaceViolation {
        path: "../escape".to_string(),
    };
    let detail = format_tool_error_detail("read_file", "../escape", &error);
    assert_eq!(
        detail,
        r#"tool=read_file path="../escape" error=workspace_violation"#
    );
}

#[test]
fn format_tool_error_detail_too_large() {
    let error = ToolError::TooLarge {
        path: "big.bin".to_string(),
        max_bytes: 65536,
    };
    let detail = format_tool_error_detail("read_file", "big.bin", &error);
    assert_eq!(
        detail,
        r#"tool=read_file path="big.bin" error=too_large max_bytes=65536"#
    );
}

#[test]
fn format_tool_error_detail_io_uses_debug_message() {
    let error = ToolError::Io {
        message: "permission denied".to_string(),
    };
    let detail = format_tool_error_detail("read_file", "file.txt", &error);
    assert_eq!(
        detail,
        r#"tool=read_file path="file.txt" error=io message="permission denied""#
    );
}

// --- Success-ordering tests (Step 1) ---

#[test]
fn read_file_success_appends_tool_call_and_result() {
    let dir = TempDir::new();
    std::fs::write(dir.path().join("hello.txt"), "hello, world!").expect("write file");
    let ctx = make_context(dir.path());
    let runner = ToolEventRunner::new_readonly();
    let mut log = EventLog::new();

    let result = runner.run(
        &mut log,
        &ctx,
        ToolRequest::ReadFile {
            path: "hello.txt".to_string(),
            offset: None,
            limit: None,
        },
    );

    assert!(result.is_ok(), "expected Ok, got {:?}", result);
    assert_eq!(log.len(), 3);
    assert_eq!(log.get(0).unwrap().kind, EventKind::ToolPolicy);
    assert_eq!(log.get(1).unwrap().kind, EventKind::ToolCall);
    assert_eq!(log.get(2).unwrap().kind, EventKind::ToolResult);
}

// --- Failure-ordering tests (Step 2) ---

#[test]
fn list_files_on_file_appends_tool_call_and_error() {
    let dir = TempDir::new();
    std::fs::write(dir.path().join("a_file.txt"), "content").expect("write file");
    let ctx = make_context(dir.path());
    let runner = ToolEventRunner::new_readonly();
    let mut log = EventLog::new();

    let result = runner.run(
        &mut log,
        &ctx,
        ToolRequest::ListFiles {
            path: "a_file.txt".to_string(),
        },
    );

    assert!(
        matches!(result, Err(ToolError::NotADirectory { .. })),
        "expected NotADirectory, got {:?}",
        result
    );
    assert_eq!(log.len(), 3);
    assert_eq!(log.get(0).unwrap().kind, EventKind::ToolPolicy);
    assert_eq!(log.get(1).unwrap().kind, EventKind::ToolCall);
    assert_eq!(log.get(2).unwrap().kind, EventKind::ToolError);
}

// --- Detail-content tests (Step 3) ---

#[test]
fn list_files_result_detail_contains_entries() {
    let dir = TempDir::new();
    let ctx = make_context(dir.path());
    let runner = ToolEventRunner::new_readonly();
    let mut log = EventLog::new();

    runner
        .run(
            &mut log,
            &ctx,
            ToolRequest::ListFiles {
                path: ".".to_string(),
            },
        )
        .ok();

    let detail = &log.get(2).unwrap().detail;
    assert!(
        detail.contains("entries="),
        "expected entries= in detail: {detail}"
    );
}

#[test]
fn read_file_result_detail_contains_bytes_not_content() {
    let dir = TempDir::new();
    let secret = "this is the secret content";
    std::fs::write(dir.path().join("secret.txt"), secret).expect("write file");
    let ctx = make_context(dir.path());
    let runner = ToolEventRunner::new_readonly();
    let mut log = EventLog::new();

    runner
        .run(
            &mut log,
            &ctx,
            ToolRequest::ReadFile {
                path: "secret.txt".to_string(),
                offset: None,
                limit: None,
            },
        )
        .ok();

    let detail = &log.get(2).unwrap().detail;
    assert!(
        detail.contains("bytes="),
        "expected bytes= in detail: {detail}"
    );
    assert!(
        !detail.contains(secret),
        "detail must not contain file content: {detail}"
    );
}

#[test]
fn read_file_escape_error_detail_contains_workspace_violation() {
    let dir = TempDir::new();
    let ctx = make_context(dir.path());
    let runner = ToolEventRunner::new_readonly();
    let mut log = EventLog::new();

    runner
        .run(
            &mut log,
            &ctx,
            ToolRequest::ReadFile {
                path: "../escape".to_string(),
                offset: None,
                limit: None,
            },
        )
        .ok();

    let detail = &log.get(2).unwrap().detail;
    assert!(
        detail.contains("error=workspace_violation"),
        "expected error=workspace_violation in detail: {detail}"
    );
}

#[test]
fn io_error_detail_contains_error_io_token_and_message() {
    let error = ToolError::Io {
        message: "permission denied".to_string(),
    };
    let detail = format_tool_error_detail("read_file", "file.txt", &error);
    assert!(detail.contains("error=io"));
    assert!(detail.contains("message="));
}

// --- Return-parity tests (Step 4) ---

#[test]
fn run_success_return_value_matches_registry() {
    let dir = TempDir::new();
    std::fs::write(dir.path().join("parity.txt"), "parity content").expect("write file");
    let ctx = make_context(dir.path());
    let runner = ToolEventRunner::new_readonly();
    let registry = ToolRegistry::new_readonly();
    let mut log = EventLog::new();

    let runner_result = runner.run(
        &mut log,
        &ctx,
        ToolRequest::ReadFile {
            path: "parity.txt".to_string(),
            offset: None,
            limit: None,
        },
    );
    let registry_result = registry.execute(
        &ctx,
        ToolRequest::ReadFile {
            path: "parity.txt".to_string(),
            offset: None,
            limit: None,
        },
    );

    assert_eq!(runner_result, registry_result);
}

#[test]
fn run_failure_return_value_matches_registry() {
    let dir = TempDir::new();
    let ctx = make_context(dir.path());
    let runner = ToolEventRunner::new_readonly();
    let registry = ToolRegistry::new_readonly();
    let mut log = EventLog::new();

    let runner_result = runner.run(
        &mut log,
        &ctx,
        ToolRequest::ReadFile {
            path: "../escape".to_string(),
            offset: None,
            limit: None,
        },
    );
    let registry_result = registry.execute(
        &ctx,
        ToolRequest::ReadFile {
            path: "../escape".to_string(),
            offset: None,
            limit: None,
        },
    );

    assert_eq!(runner_result, registry_result);
}

// --- Persistence round-trip test (Step 5) ---

#[test]
fn persistence_round_trip_restores_events() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();
    std::fs::write(workspace_dir.path().join("sample.txt"), "some content").expect("write file");
    let ctx = make_context(workspace_dir.path());
    let runner = ToolEventRunner::new_readonly();

    // Run one success and one failure, then drop the log to flush.
    {
        let store = EventStore::new(store_dir.path());
        let mut log = EventLog::load_from(store);
        runner
            .run(
                &mut log,
                &ctx,
                ToolRequest::ListFiles {
                    path: ".".to_string(),
                },
            )
            .ok();
        runner
            .run(
                &mut log,
                &ctx,
                ToolRequest::ReadFile {
                    path: "../escape".to_string(),
                    offset: None,
                    limit: None,
                },
            )
            .ok();
    }

    // Reload from the same store dir and verify all six events restored.
    let store2 = EventStore::new(store_dir.path());
    let log2 = EventLog::load_from(store2);

    assert_eq!(log2.len(), 6);
    assert_eq!(log2.get(0).unwrap().kind, EventKind::ToolPolicy);
    assert_eq!(log2.get(1).unwrap().kind, EventKind::ToolCall);
    assert_eq!(log2.get(2).unwrap().kind, EventKind::ToolResult);
    assert_eq!(log2.get(3).unwrap().kind, EventKind::ToolPolicy);
    assert_eq!(log2.get(4).unwrap().kind, EventKind::ToolCall);
    assert_eq!(log2.get(5).unwrap().kind, EventKind::ToolError);
    assert!(!log2.get(0).unwrap().detail.is_empty());
    assert!(!log2.get(1).unwrap().detail.is_empty());
    assert!(!log2.get(2).unwrap().detail.is_empty());
    assert!(!log2.get(3).unwrap().detail.is_empty());
    assert!(!log2.get(4).unwrap().detail.is_empty());
    assert!(!log2.get(5).unwrap().detail.is_empty());
}

// --- Never-panic test (Step 6) ---

#[test]
fn run_does_not_panic_for_path_with_quote_character() {
    let dir = TempDir::new();
    let ctx = make_context(dir.path());
    let runner = ToolEventRunner::new_readonly();
    let mut log = EventLog::new();

    // Path contains a `"` character — must not panic regardless of outcome.
    let _ = runner.run(
        &mut log,
        &ctx,
        ToolRequest::ReadFile {
            path: r#"file"name.txt"#.to_string(),
            offset: None,
            limit: None,
        },
    );

    let detail = &log.get(1).unwrap().detail;
    assert!(
        !detail.is_empty(),
        "ToolCall event at index 1 detail must be non-empty"
    );
}

// --- PlanWrite mutation-intent tests (T-1) ---

#[test]
fn plan_write_emits_only_tool_policy_and_approval_request() {
    let dir = TempDir::new();
    let ctx = make_context(dir.path());
    let runner = ToolEventRunner::new_readonly();
    let mut log = EventLog::new();

    let result = runner.run(
        &mut log,
        &ctx,
        ToolRequest::PlanWrite {
            path: "README.md".to_string(),
        },
    );

    // Exactly two events: ToolPolicy then ApprovalRequest.
    assert_eq!(log.len(), 2, "expected exactly 2 events, got {}", log.len());
    assert_eq!(log.get(0).unwrap().kind, EventKind::ToolPolicy);
    assert_eq!(log.get(1).unwrap().kind, EventKind::ApprovalRequest);

    // No ToolCall, ToolResult, or ToolError events appended.
    assert!(
        !log.events().iter().any(|e| e.kind == EventKind::ToolCall),
        "ToolCall must not be appended for PlanWrite"
    );
    assert!(
        !log.events().iter().any(|e| e.kind == EventKind::ToolResult),
        "ToolResult must not be appended for PlanWrite"
    );
    assert!(
        !log.events().iter().any(|e| e.kind == EventKind::ToolError),
        "ToolError must not be appended for PlanWrite"
    );

    // Returns Err(ApprovalRequired).
    assert!(
        matches!(result, Err(ToolError::ApprovalRequired { .. })),
        "expected ApprovalRequired error, got {:?}",
        result
    );

    // ToolPolicy detail.
    let policy_detail = &log.get(0).unwrap().detail;
    assert_eq!(
        policy_detail,
        r#"tool=write_file path="README.md" risk=workspace_write decision=allow reason=workspace_write_requires_approval"#,
        "ToolPolicy detail mismatch: {policy_detail}"
    );

    // ApprovalRequest detail.
    let approval_detail = &log.get(1).unwrap().detail;
    assert_eq!(
        approval_detail,
        r#"tool=write_file path="README.md" risk=workspace_write reason=workspace_write_requires_approval"#,
        "ApprovalRequest detail mismatch: {approval_detail}"
    );
}

// --- ToolPolicy ordering and detail tests (Steps 6-7) ---

#[test]
fn list_files_success_policy_event_precedes_call_and_result_with_allow_detail() {
    let dir = TempDir::new();
    let ctx = make_context(dir.path());
    let runner = ToolEventRunner::new_readonly();
    let mut log = EventLog::new();

    let result = runner.run(
        &mut log,
        &ctx,
        ToolRequest::ListFiles {
            path: ".".to_string(),
        },
    );

    assert!(result.is_ok(), "expected Ok, got {:?}", result);
    assert_eq!(log.len(), 3);
    assert_eq!(log.get(0).unwrap().kind, EventKind::ToolPolicy);
    assert_eq!(log.get(1).unwrap().kind, EventKind::ToolCall);
    assert_eq!(log.get(2).unwrap().kind, EventKind::ToolResult);

    let policy_detail = &log.get(0).unwrap().detail;
    assert!(
        policy_detail.contains("risk=read_only"),
        "ToolPolicy detail must contain risk=read_only: {policy_detail}"
    );
    assert!(
        policy_detail.contains("decision=allow"),
        "ToolPolicy detail must contain decision=allow: {policy_detail}"
    );
    assert!(
        policy_detail.contains("reason=read_only_auto_allow"),
        "ToolPolicy detail must contain reason=read_only_auto_allow: {policy_detail}"
    );
}

#[test]
fn read_file_escape_policy_allows_then_registry_produces_violation() {
    let dir = TempDir::new();
    let ctx = make_context(dir.path());
    let runner = ToolEventRunner::new_readonly();
    let mut log = EventLog::new();

    let result = runner.run(
        &mut log,
        &ctx,
        ToolRequest::ReadFile {
            path: "../secret.txt".to_string(),
            offset: None,
            limit: None,
        },
    );

    assert!(
        matches!(result, Err(ToolError::WorkspaceViolation { .. })),
        "expected WorkspaceViolation, got {:?}",
        result
    );
    assert_eq!(log.len(), 3);
    assert_eq!(log.get(0).unwrap().kind, EventKind::ToolPolicy);
    assert_eq!(log.get(1).unwrap().kind, EventKind::ToolCall);
    assert_eq!(log.get(2).unwrap().kind, EventKind::ToolError);

    // Policy allows even for escaping paths — registry produces the violation post-ToolCall.
    let policy_detail = &log.get(0).unwrap().detail;
    assert!(
        policy_detail.contains("decision=allow"),
        "ToolPolicy must allow even for escaping paths: {policy_detail}"
    );
}

// --- Deny-path tests (T-5) ---

#[test]
fn deny_all_engine_returns_policy_denied_error_without_tool_call() {
    let dir = TempDir::new();
    std::fs::write(dir.path().join("hello.txt"), "hello, world!").expect("write file");
    let ctx = make_context(dir.path());
    let runner = ToolEventRunner::with_policy(ToolPolicyEngine::deny_all());
    let mut log = EventLog::new();

    let result = runner.run(
        &mut log,
        &ctx,
        ToolRequest::ReadFile {
            path: "hello.txt".to_string(),
            offset: None,
            limit: None,
        },
    );

    // Exactly one event: ToolPolicy with decision=deny.
    assert_eq!(log.len(), 1);
    assert_eq!(log.get(0).unwrap().kind, EventKind::ToolPolicy);
    let policy_detail = &log.get(0).unwrap().detail;
    assert!(
        policy_detail.contains("decision=deny"),
        "ToolPolicy detail must contain decision=deny: {policy_detail}"
    );

    // No ToolCall event appended.
    assert!(
        !log.events().iter().any(|e| e.kind == EventKind::ToolCall),
        "no ToolCall event should be appended on policy deny"
    );

    // Result is Err(ToolError::PolicyDenied { .. }).
    assert!(
        matches!(result, Err(ToolError::PolicyDenied { .. })),
        "expected PolicyDenied error, got {:?}",
        result
    );
}

// --- ApprovalGate integration tests (T-4) ---

#[test]
fn manual_approval_engine_stops_before_tool_call_and_records_approval_request() {
    let dir = TempDir::new();
    // Target file is intentionally NOT created — gate stops before registry.
    let ctx = make_context(dir.path());
    let runner =
        ToolEventRunner::with_policy(ToolPolicyEngine::manual_for_test("test_manual_approval"));
    let mut log = EventLog::new();

    let result = runner.run(
        &mut log,
        &ctx,
        ToolRequest::ReadFile {
            path: "missing.txt".to_string(),
            offset: None,
            limit: None,
        },
    );

    // Exactly two events: ToolPolicy then ApprovalRequest.
    assert_eq!(log.len(), 2, "expected exactly 2 events, got {}", log.len());
    assert_eq!(log.get(0).unwrap().kind, EventKind::ToolPolicy);
    assert_eq!(log.get(1).unwrap().kind, EventKind::ApprovalRequest);

    // No ToolCall, ToolResult, or ToolError events appended.
    assert!(
        !log.events().iter().any(|e| e.kind == EventKind::ToolCall),
        "ToolCall must not be appended when gate stops execution"
    );
    assert!(
        !log.events().iter().any(|e| e.kind == EventKind::ToolResult),
        "ToolResult must not be appended when gate stops execution"
    );
    assert!(
        !log.events().iter().any(|e| e.kind == EventKind::ToolError),
        "ToolError must not be appended when gate stops execution"
    );

    // Result is Err(ToolError::ApprovalRequired { .. }).
    assert!(
        matches!(result, Err(ToolError::ApprovalRequired { .. })),
        "expected ApprovalRequired error, got {:?}",
        result
    );

    // ApprovalRequest detail contains risk=read_only and reason=test_manual_approval.
    let approval_detail = &log.get(1).unwrap().detail;
    assert!(
        approval_detail.contains("risk=read_only"),
        "ApprovalRequest detail must contain risk=read_only: {approval_detail}"
    );
    assert!(
        approval_detail.contains("reason=test_manual_approval"),
        "ApprovalRequest detail must contain reason=test_manual_approval: {approval_detail}"
    );
}

#[test]
fn read_only_engine_gate_pass_produces_no_approval_request_event() {
    let dir = TempDir::new();
    let ctx = make_context(dir.path());
    let runner = ToolEventRunner::new_readonly();
    let mut log = EventLog::new();

    let result = runner.run(
        &mut log,
        &ctx,
        ToolRequest::ListFiles {
            path: ".".to_string(),
        },
    );

    assert!(result.is_ok(), "expected Ok, got {:?}", result);

    // Exactly three events in order: ToolPolicy, ToolCall, ToolResult.
    assert_eq!(log.len(), 3, "expected exactly 3 events, got {}", log.len());
    assert_eq!(log.get(0).unwrap().kind, EventKind::ToolPolicy);
    assert_eq!(log.get(1).unwrap().kind, EventKind::ToolCall);
    assert_eq!(log.get(2).unwrap().kind, EventKind::ToolResult);

    // No ApprovalRequest event inserted when requirement is None.
    assert!(
        !log.events()
            .iter()
            .any(|e| e.kind == EventKind::ApprovalRequest),
        "ApprovalRequest must not appear when ApprovalRequirement::None"
    );
}

#[test]
fn approval_request_event_survives_persistence_round_trip() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();
    let ctx = make_context(workspace_dir.path());
    let runner =
        ToolEventRunner::with_policy(ToolPolicyEngine::manual_for_test("test_manual_approval"));

    // Run a request that triggers the gate; flush by dropping the log.
    {
        let store = EventStore::new(store_dir.path());
        let mut log = EventLog::load_from(store);
        runner
            .run(
                &mut log,
                &ctx,
                ToolRequest::ReadFile {
                    path: "missing.txt".to_string(),
                    offset: None,
                    limit: None,
                },
            )
            .ok();
    }

    // Reload and verify the ApprovalRequest event kind is preserved.
    let store2 = EventStore::new(store_dir.path());
    let log2 = EventLog::load_from(store2);

    assert_eq!(log2.len(), 2);
    assert_eq!(log2.get(0).unwrap().kind, EventKind::ToolPolicy);
    assert_eq!(log2.get(1).unwrap().kind, EventKind::ApprovalRequest);
    assert!(
        !log2.get(1).unwrap().detail.is_empty(),
        "ApprovalRequest detail must be non-empty after reload"
    );
}

// --- SearchText integration tests ---

#[test]
fn search_text_run_emits_tool_policy_call_result() {
    let dir = TempDir::new();
    std::fs::write(dir.path().join("hello.txt"), "hello world\n").expect("write file");
    let ctx = make_context(dir.path());
    let runner = ToolEventRunner::new_readonly();
    let mut log = EventLog::new();

    let result = runner.run(
        &mut log,
        &ctx,
        ToolRequest::SearchText {
            query: "hello".to_string(),
        },
    );

    assert!(result.is_ok(), "expected Ok, got {:?}", result);
    assert_eq!(log.len(), 3);
    assert_eq!(log.get(0).unwrap().kind, EventKind::ToolPolicy);
    assert_eq!(log.get(1).unwrap().kind, EventKind::ToolCall);
    assert_eq!(log.get(2).unwrap().kind, EventKind::ToolResult);
}

#[test]
fn search_text_result_detail_is_summary_only_no_matched_line_text() {
    let dir = TempDir::new();
    let secret_line = "UNIQUE_SECRET_MATCH_TEXT_KERNEL_99887";
    std::fs::write(dir.path().join("secret.txt"), format!("{secret_line}\n")).expect("write file");
    let ctx = make_context(dir.path());
    let runner = ToolEventRunner::new_readonly();
    let mut log = EventLog::new();

    runner
        .run(
            &mut log,
            &ctx,
            ToolRequest::SearchText {
                query: "UNIQUE_SECRET_MATCH_TEXT".to_string(),
            },
        )
        .ok();

    let detail = &log.get(2).unwrap().detail;
    assert!(
        detail.contains("matches="),
        "expected matches= in detail: {detail}"
    );
    assert!(
        detail.contains("truncated="),
        "expected truncated= in detail: {detail}"
    );
    assert!(
        !detail.contains(secret_line),
        "detail must not contain matched line text: {detail}"
    );
}

// --- PreviewWrite tests ---

const PROPOSED_CONTENT_SENTINEL: &str = "PROPOSED_CONTENT_SENTINEL_XYZ_9182736";
const OLD_TARGET_CONTENT_SENTINEL: &str = "OLD_TARGET_CONTENT_SENTINEL_ABC_1234567";

// (a) PreviewWrite success — event sequence ToolPolicy, ToolCall, ToolResult;
//     ToolResult detail starts with "tool=preview_write path=" and contains exactly
//     one "path=" token (path-duplication guard) and a "kind=" token.
#[test]
fn preview_write_success_emits_policy_call_result_events() {
    let dir = TempDir::new();
    std::fs::write(dir.path().join("target.txt"), OLD_TARGET_CONTENT_SENTINEL).expect("write");
    let ctx = make_context(dir.path());
    let runner = ToolEventRunner::new_readonly();
    let mut log = EventLog::new();

    let result = runner.run(
        &mut log,
        &ctx,
        ToolRequest::PreviewWrite {
            path: "target.txt".to_string(),
            content: PROPOSED_CONTENT_SENTINEL.to_string(),
        },
    );

    assert!(result.is_ok(), "expected Ok, got {:?}", result);
    assert_eq!(log.len(), 3);
    assert_eq!(log.get(0).unwrap().kind, EventKind::ToolPolicy);
    assert_eq!(log.get(1).unwrap().kind, EventKind::ToolCall);
    assert_eq!(log.get(2).unwrap().kind, EventKind::ToolResult);

    let result_detail = &log.get(2).unwrap().detail;
    assert!(
        result_detail.starts_with("tool=preview_write path="),
        "ToolResult detail must start with 'tool=preview_write path=': {result_detail}"
    );

    // Exactly one "path=" token (path-duplication guard).
    let path_count = result_detail.matches("path=").count();
    assert_eq!(
        path_count, 1,
        "ToolResult detail must contain exactly one 'path=' token, got {path_count}: {result_detail}"
    );

    assert!(
        result_detail.contains("kind="),
        "ToolResult detail must contain 'kind=': {result_detail}"
    );
}

// (b) Content-leak guard: no event detail must contain the proposed or existing
//     content sentinels; no ApprovalRequest event must appear.
#[test]
fn preview_write_success_no_content_leak_in_events() {
    let dir = TempDir::new();
    std::fs::write(dir.path().join("secret.txt"), OLD_TARGET_CONTENT_SENTINEL).expect("write");
    let ctx = make_context(dir.path());
    let runner = ToolEventRunner::new_readonly();
    let mut log = EventLog::new();

    runner
        .run(
            &mut log,
            &ctx,
            ToolRequest::PreviewWrite {
                path: "secret.txt".to_string(),
                content: PROPOSED_CONTENT_SENTINEL.to_string(),
            },
        )
        .ok();

    for event in log.events() {
        assert!(
            !event.detail.contains(PROPOSED_CONTENT_SENTINEL),
            "event detail must not contain proposed content sentinel: {:?}",
            event.detail
        );
        assert!(
            !event.detail.contains(OLD_TARGET_CONTENT_SENTINEL),
            "event detail must not contain existing content sentinel: {:?}",
            event.detail
        );
        assert!(
            !event
                .detail
                .contains(&format!("+{PROPOSED_CONTENT_SENTINEL}")),
            "event detail must not contain +sentinel: {:?}",
            event.detail
        );
        assert!(
            !event
                .detail
                .contains(&format!("-{OLD_TARGET_CONTENT_SENTINEL}")),
            "event detail must not contain -sentinel: {:?}",
            event.detail
        );
    }

    assert!(
        !log.events()
            .iter()
            .any(|e| e.kind == EventKind::ApprovalRequest),
        "no ApprovalRequest must be emitted for PreviewWrite"
    );
}

// (c) Workspace-violation path produces ToolPolicy, ToolCall, ToolError with
//     error=workspace_violation; no ApprovalRequest; no content sentinel leak.
#[test]
fn preview_write_workspace_violation_emits_policy_call_error_events() {
    let dir = TempDir::new();
    let ctx = make_context(dir.path());
    let runner = ToolEventRunner::new_readonly();
    let mut log = EventLog::new();

    let result = runner.run(
        &mut log,
        &ctx,
        ToolRequest::PreviewWrite {
            path: "../escape.txt".to_string(),
            content: PROPOSED_CONTENT_SENTINEL.to_string(),
        },
    );

    assert!(
        matches!(result, Err(ToolError::WorkspaceViolation { .. })),
        "expected WorkspaceViolation, got {:?}",
        result
    );
    assert_eq!(log.len(), 3);
    assert_eq!(log.get(0).unwrap().kind, EventKind::ToolPolicy);
    assert_eq!(log.get(1).unwrap().kind, EventKind::ToolCall);
    assert_eq!(log.get(2).unwrap().kind, EventKind::ToolError);

    let error_detail = &log.get(2).unwrap().detail;
    assert!(
        error_detail.contains("error=workspace_violation"),
        "ToolError detail must contain error=workspace_violation: {error_detail}"
    );

    assert!(
        !log.events()
            .iter()
            .any(|e| e.kind == EventKind::ApprovalRequest),
        "no ApprovalRequest must be emitted for a workspace-violation preview"
    );

    for event in log.events() {
        assert!(
            !event.detail.contains(PROPOSED_CONTENT_SENTINEL),
            "event detail must not contain proposed content sentinel: {:?}",
            event.detail
        );
    }
}

// (d) Error-mapping coverage — ParentNotFound maps to NotFound.
#[test]
fn preview_write_missing_parent_maps_to_not_found() {
    let dir = TempDir::new();
    let ctx = make_context(dir.path());
    let runner = ToolEventRunner::new_readonly();
    let mut log = EventLog::new();

    let result = runner.run(
        &mut log,
        &ctx,
        ToolRequest::PreviewWrite {
            path: "nonexistent_dir/child.txt".to_string(),
            content: "proposed content".to_string(),
        },
    );

    assert!(
        matches!(result, Err(ToolError::NotFound { .. })),
        "expected NotFound, got {:?}",
        result
    );
}

// (d) Error-mapping coverage — NotAFile maps to NotAFile.
#[test]
fn preview_write_directory_target_maps_to_not_a_file() {
    let dir = TempDir::new();
    std::fs::create_dir_all(dir.path().join("subdir")).expect("create subdir");
    let ctx = make_context(dir.path());
    let runner = ToolEventRunner::new_readonly();
    let mut log = EventLog::new();

    let result = runner.run(
        &mut log,
        &ctx,
        ToolRequest::PreviewWrite {
            path: "subdir".to_string(),
            content: "proposed content".to_string(),
        },
    );

    assert!(
        matches!(result, Err(ToolError::NotAFile { .. })),
        "expected NotAFile, got {:?}",
        result
    );
}

// --- append_write_approval tests ---

const APPEND_WRITE_APPROVAL_SENTINEL: &str = "APPEND_WRITE_APPROVAL_SENTINEL_XYZ_12345678";

fn make_write_preview_intent(path: &str, content: &str) -> crate::write_intent::WriteIntent {
    new_text(
        path,
        content,
        WriteIntentMode::CreateOrReplace,
        WriteIntentSource::Operator,
    )
    .expect("valid intent")
}

// (a) append_write_approval emits exactly ToolPolicy then ApprovalRequest.
#[test]
fn append_write_approval_emits_tool_policy_then_approval_request() {
    let dir = TempDir::new();
    std::fs::write(
        dir.path().join("target.txt"),
        APPEND_WRITE_APPROVAL_SENTINEL,
    )
    .expect("write file");
    let ctx = make_context(dir.path());
    let intent = make_write_preview_intent("target.txt", "replacement content\n");
    let preview = preview_write_intent(&ctx, &intent).expect("should succeed");

    let runner = ToolEventRunner::new_readonly();
    let mut log = EventLog::new();

    runner.append_write_approval(&mut log, "target.txt", &preview);

    // Exactly two events in order.
    assert_eq!(log.len(), 2, "expected 2 events, got {}", log.len());
    assert_eq!(log.get(0).unwrap().kind, EventKind::ToolPolicy);
    assert_eq!(log.get(1).unwrap().kind, EventKind::ApprovalRequest);

    // No ToolCall, ToolResult, or ToolError events.
    assert!(
        !log.events().iter().any(|e| e.kind == EventKind::ToolCall),
        "ToolCall must not be appended"
    );
    assert!(
        !log.events().iter().any(|e| e.kind == EventKind::ToolResult),
        "ToolResult must not be appended"
    );
    assert!(
        !log.events().iter().any(|e| e.kind == EventKind::ToolError),
        "ToolError must not be appended"
    );
}

// (b) ToolPolicy detail has the expected exact format.
#[test]
fn append_write_approval_tool_policy_detail_exact() {
    let dir = TempDir::new();
    let ctx = make_context(dir.path());
    let intent = make_write_preview_intent("README.md", "new content\n");
    let preview = preview_write_intent(&ctx, &intent).expect("should succeed");

    let runner = ToolEventRunner::new_readonly();
    let mut log = EventLog::new();

    runner.append_write_approval(&mut log, "README.md", &preview);

    let policy_detail = &log.get(0).unwrap().detail;
    assert_eq!(
        policy_detail,
        r#"tool=write_file path="README.md" risk=workspace_write decision=allow reason=workspace_write_requires_approval"#,
        "ToolPolicy detail mismatch: {policy_detail}"
    );
}

// (c) ApprovalRequest detail starts with expected prefix and contains summary fields.
#[test]
fn append_write_approval_approval_request_detail_structure() {
    let dir = TempDir::new();
    std::fs::write(
        dir.path().join("target.txt"),
        APPEND_WRITE_APPROVAL_SENTINEL,
    )
    .expect("write file");
    let ctx = make_context(dir.path());
    let intent = make_write_preview_intent("target.txt", "replacement content\n");
    let preview = preview_write_intent(&ctx, &intent).expect("should succeed");

    let runner = ToolEventRunner::new_readonly();
    let mut log = EventLog::new();

    runner.append_write_approval(&mut log, "target.txt", &preview);

    let approval_detail = &log.get(1).unwrap().detail;

    // Starts with expected prefix (note trailing space before summary).
    assert!(
        approval_detail.starts_with(
            r#"tool=write_file path="target.txt" risk=workspace_write reason=workspace_write_requires_approval "#
        ),
        "ApprovalRequest detail must start with expected prefix: {approval_detail}"
    );

    // Contains preview summary fields.
    assert!(
        approval_detail.contains("preview_kind="),
        "expected preview_kind= in approval detail: {approval_detail}"
    );
    assert!(
        approval_detail.contains("truncated="),
        "expected truncated= in approval detail: {approval_detail}"
    );

    // No diff lines.
    for line in approval_detail.lines() {
        assert!(
            !line.starts_with("+ ") && !line.starts_with("- "),
            "approval detail must not contain diff lines: {line}"
        );
    }

    // No content sentinel leak.
    assert!(
        !approval_detail.contains(APPEND_WRITE_APPROVAL_SENTINEL),
        "approval detail must not contain file content sentinel: {approval_detail}"
    );
}

// (d) ParsedApprovalRequest parses correctly and to_tool_request() returns None.
#[test]
fn append_write_approval_parsed_approval_request_non_resumable() {
    let dir = TempDir::new();
    let ctx = make_context(dir.path());
    let intent = make_write_preview_intent("output.txt", "new file content\n");
    let preview = preview_write_intent(&ctx, &intent).expect("should succeed");

    let runner = ToolEventRunner::new_readonly();
    let mut log = EventLog::new();

    runner.append_write_approval(&mut log, "output.txt", &preview);

    let approval_detail = &log.get(1).unwrap().detail;

    let parsed = ParsedApprovalRequest::parse_detail(approval_detail)
        .expect("ApprovalRequest detail must parse");

    assert_eq!(parsed.tool, "write_file");
    assert_eq!(parsed.path, "output.txt");
    assert_eq!(parsed.risk, "workspace_write");

    // Non-resumable: write_file is not a recognised resumable tool.
    assert!(
        parsed.to_tool_request().is_none(),
        "write_file approval must be non-resumable (to_tool_request must return None)"
    );
}

// --- GlobFiles event-runner tests (T-2) ---

#[test]
fn glob_files_run_emits_tool_policy_call_result_no_approval() {
    let dir = TempDir::new();
    std::fs::write(dir.path().join("main.rs"), "fn main() {}").expect("write file");
    let ctx = make_context(dir.path());
    let runner = ToolEventRunner::new_readonly();
    let mut log = EventLog::new();

    let result = runner.run(
        &mut log,
        &ctx,
        ToolRequest::GlobFiles {
            pattern: "*.rs".to_string(),
        },
    );

    assert!(result.is_ok(), "expected Ok, got {:?}", result);
    assert_eq!(log.len(), 3);
    assert_eq!(log.get(0).unwrap().kind, EventKind::ToolPolicy);
    assert_eq!(log.get(1).unwrap().kind, EventKind::ToolCall);
    assert_eq!(log.get(2).unwrap().kind, EventKind::ToolResult);

    // No ApprovalRequest event.
    assert!(
        !log.events()
            .iter()
            .any(|e| e.kind == EventKind::ApprovalRequest),
        "ApprovalRequest must not appear for GlobFiles"
    );
}

#[test]
fn glob_files_policy_evaluate_returns_read_only_auto_allow_no_approval() {
    use crate::approval::ApprovalRequirement;
    use crate::tool::policy::{ToolPolicyDecision, ToolPolicyEngine};
    use crate::tool::registry::ToolRisk;

    let engine = ToolPolicyEngine::read_only();
    let request = ToolRequest::GlobFiles {
        pattern: "*.rs".to_string(),
    };
    let outcome = engine.evaluate(&request);

    assert_eq!(outcome.decision, ToolPolicyDecision::Allow);
    assert_eq!(outcome.risk, ToolRisk::ReadOnly);
    assert_eq!(outcome.reason, "read_only_auto_allow");
    assert_eq!(outcome.approval_requirement, ApprovalRequirement::None);
}

#[test]
fn glob_files_tool_result_detail_is_summary_only_no_matched_path() {
    let dir = TempDir::new();
    let unique_path_name = "UNIQUE_GLOB_SENTINEL_PATH_99887.rs";
    std::fs::write(dir.path().join(unique_path_name), "fn main() {}").expect("write file");
    let ctx = make_context(dir.path());
    let runner = ToolEventRunner::new_readonly();
    let mut log = EventLog::new();

    runner
        .run(
            &mut log,
            &ctx,
            ToolRequest::GlobFiles {
                pattern: "*.rs".to_string(),
            },
        )
        .ok();

    let detail = &log.get(2).unwrap().detail;
    assert!(
        detail.contains("truncated="),
        "expected truncated= in detail: {detail}"
    );
    assert!(
        detail.contains("pattern="),
        "expected pattern= in detail: {detail}"
    );
    assert!(
        !detail.contains(unique_path_name),
        "detail must not contain matched path: {detail}"
    );
}

#[test]
fn glob_files_model_output_starts_with_glob_pattern_prefix() {
    let output = ToolOutput::FileMatches {
        pattern: "**/*.rs".to_string(),
        paths: vec!["src/main.rs".to_string(), "src/lib.rs".to_string()],
        truncated: false,
    };
    let formatted = format_tool_output_for_model(&output);
    assert!(
        formatted.starts_with("Glob pattern: **/*.rs"),
        "model output must start with 'Glob pattern:': {formatted}"
    );
    assert!(
        formatted.contains("src/main.rs"),
        "model output must contain matched paths: {formatted}"
    );
}

#[test]
fn glob_files_model_output_truncated_ends_with_truncated_suffix() {
    let output = ToolOutput::FileMatches {
        pattern: "*.rs".to_string(),
        paths: vec!["a.rs".to_string()],
        truncated: true,
    };
    let formatted = format_tool_output_for_model(&output);
    assert!(
        formatted.ends_with("\n... [truncated]"),
        "model output must end with truncation suffix when truncated=true: {formatted}"
    );
}

#[test]
fn glob_files_model_output_not_truncated_has_no_truncated_suffix() {
    let output = ToolOutput::FileMatches {
        pattern: "*.rs".to_string(),
        paths: vec!["a.rs".to_string()],
        truncated: false,
    };
    let formatted = format_tool_output_for_model(&output);
    assert!(
        !formatted.contains("... [truncated]"),
        "model output must not contain truncation suffix when truncated=false: {formatted}"
    );
}

// ─── Range detail-string tests ────────────────────────────────────────────────

#[test]
fn format_tool_result_detail_range_file_content_includes_start_line_fields() {
    let output = ToolOutput::FileContent {
        path: "src/lib.rs".to_string(),
        content: "line5\nline6\nline7".to_string(),
        start_line: Some(5),
        line_count: Some(3),
        truncated: false,
    };
    let detail = format_tool_result_detail("read_file", "src/lib.rs", &output);
    assert!(
        detail.contains("start_line=5"),
        "expected start_line=5 in detail: {detail}"
    );
    assert!(
        detail.contains("line_count=3"),
        "expected line_count=3 in detail: {detail}"
    );
    assert!(
        detail.contains("bytes="),
        "expected bytes= in detail: {detail}"
    );
    assert!(
        detail.contains("truncated=false"),
        "expected truncated=false in detail: {detail}"
    );
}

#[test]
fn format_tool_call_detail_range_read_includes_offset_and_limit() {
    let detail = format_tool_call_detail("read_file", "src/lib.rs", Some(10), Some(20));
    assert!(
        detail.contains("offset=10"),
        "expected offset=10 in detail: {detail}"
    );
    assert!(
        detail.contains("limit=20"),
        "expected limit=20 in detail: {detail}"
    );
    assert!(
        detail.contains("risk=read_only"),
        "expected risk=read_only in detail: {detail}"
    );
}

#[test]
fn range_read_event_log_result_detail_has_start_line_format() {
    let dir = TempDir::new();
    std::fs::write(
        dir.path().join("target.txt"),
        "alpha\nbeta\ngamma\ndelta\nepsilon",
    )
    .expect("write file");
    let ctx = make_context(dir.path());
    let runner = ToolEventRunner::new_readonly();
    let mut log = EventLog::new();

    runner
        .run(
            &mut log,
            &ctx,
            ToolRequest::ReadFile {
                path: "target.txt".to_string(),
                offset: Some(2),
                limit: Some(3),
            },
        )
        .ok();

    let result_detail = &log.get(2).unwrap().detail;
    assert!(
        result_detail.contains("start_line=2"),
        "ToolResult detail must contain start_line=2: {result_detail}"
    );
    assert!(
        result_detail.contains("line_count=3"),
        "ToolResult detail must contain line_count=3: {result_detail}"
    );
    assert!(
        result_detail.contains("truncated="),
        "ToolResult detail must contain truncated=: {result_detail}"
    );
    assert!(
        result_detail.contains("bytes="),
        "ToolResult detail must contain bytes=: {result_detail}"
    );

    // ToolCall detail must include offset= and limit=.
    let call_detail = &log.get(1).unwrap().detail;
    assert!(
        call_detail.contains("offset=2"),
        "ToolCall detail must contain offset=2: {call_detail}"
    );
    assert!(
        call_detail.contains("limit=3"),
        "ToolCall detail must contain limit=3: {call_detail}"
    );
}
