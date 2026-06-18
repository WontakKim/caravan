use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::*;
use crate::events::{EventKind, EventLog};
use crate::storage::EventStore;
use crate::tool::registry::ToolExecutionContext;

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
    let detail = format_tool_call_detail("list_files", ".");
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
        },
    );
    let registry_result = registry.execute(
        &ctx,
        ToolRequest::ReadFile {
            path: "parity.txt".to_string(),
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
        },
    );
    let registry_result = registry.execute(
        &ctx,
        ToolRequest::ReadFile {
            path: "../escape".to_string(),
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
        },
    );

    let detail = &log.get(1).unwrap().detail;
    assert!(
        !detail.is_empty(),
        "ToolCall event at index 1 detail must be non-empty"
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
