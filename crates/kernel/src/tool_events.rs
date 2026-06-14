//! ToolEventRunner: traces read-only tool execution as EventLog entries.

use crate::events::{EventKind, EventLog};
use crate::tools::{ToolError, ToolExecutionContext, ToolOutput, ToolRegistry, ToolRequest};

/// Runs read-only tool calls and records them in an [`EventLog`].
pub struct ToolEventRunner {
    registry: ToolRegistry,
}

impl ToolEventRunner {
    /// Creates a new runner backed by a read-only [`ToolRegistry`].
    pub fn new_readonly() -> Self {
        ToolEventRunner {
            registry: ToolRegistry::new_readonly(),
        }
    }

    /// Executes a tool request, recording a [`ToolCall`] event before delegating
    /// and either a [`ToolResult`] or [`ToolError`] event on completion.
    ///
    /// The `path` is captured from `&request` BEFORE the move into
    /// [`ToolRegistry::execute`] so the event always reflects the caller-supplied
    /// path, never workspace-internal paths that may appear in error variant fields.
    pub fn run(
        &self,
        event_log: &mut EventLog,
        context: &ToolExecutionContext,
        request: ToolRequest,
    ) -> Result<ToolOutput, ToolError> {
        let (tool_name, tool_path) = match &request {
            ToolRequest::ListFiles { path } => ("list_files", path.clone()),
            ToolRequest::ReadFile { path } => ("read_file", path.clone()),
        };

        event_log.append(
            EventKind::ToolCall,
            format_tool_call_detail(tool_name, &tool_path),
        );

        match self.registry.execute(context, request) {
            Ok(output) => {
                event_log.append(
                    EventKind::ToolResult,
                    format_tool_result_detail(tool_name, &tool_path, &output),
                );
                Ok(output)
            }
            Err(error) => {
                event_log.append(
                    EventKind::ToolError,
                    format_tool_error_detail(tool_name, &tool_path, &error),
                );
                Err(error)
            }
        }
    }
}

fn format_tool_call_detail(tool_name: &str, path: &str) -> String {
    format!("tool={} path={:?} risk=read_only", tool_name, path)
}

fn format_tool_result_detail(tool_name: &str, path: &str, output: &ToolOutput) -> String {
    match output {
        ToolOutput::FileList { entries, .. } => {
            format!(
                "tool={} path={:?} entries={}",
                tool_name,
                path,
                entries.len()
            )
        }
        ToolOutput::FileContent { content, .. } => {
            format!("tool={} path={:?} bytes={}", tool_name, path, content.len())
        }
    }
}

fn format_tool_error_detail(tool_name: &str, path: &str, error: &ToolError) -> String {
    let token = match error {
        ToolError::WorkspaceViolation { .. } => "workspace_violation".to_string(),
        ToolError::NotFound { .. } => "not_found".to_string(),
        ToolError::NotAFile { .. } => "not_a_file".to_string(),
        ToolError::NotADirectory { .. } => "not_a_directory".to_string(),
        ToolError::NonUtf8 { .. } => "non_utf8".to_string(),
        ToolError::TooLarge { max_bytes, .. } => format!("too_large max_bytes={}", max_bytes),
        ToolError::Io { message } => {
            let token = "io";
            format!("{} message={:?}", token, message)
        }
    };
    format!("tool={} path={:?} error={}", tool_name, path, token)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::events::{EventKind, EventLog};
    use crate::storage::EventStore;
    use crate::tools::ToolExecutionContext;

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
        assert_eq!(log.len(), 2);
        assert_eq!(log.get(0).unwrap().kind, EventKind::ToolCall);
        assert_eq!(log.get(1).unwrap().kind, EventKind::ToolResult);
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
        assert_eq!(log.len(), 2);
        assert_eq!(log.get(0).unwrap().kind, EventKind::ToolCall);
        assert_eq!(log.get(1).unwrap().kind, EventKind::ToolError);
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
        assert_eq!(log.len(), 2);
        assert_eq!(log.get(0).unwrap().kind, EventKind::ToolCall);
        assert_eq!(log.get(1).unwrap().kind, EventKind::ToolResult);
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
        assert_eq!(log.len(), 2);
        assert_eq!(log.get(0).unwrap().kind, EventKind::ToolCall);
        assert_eq!(log.get(1).unwrap().kind, EventKind::ToolError);
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

        let detail = &log.get(1).unwrap().detail;
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

        let detail = &log.get(1).unwrap().detail;
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

        let detail = &log.get(1).unwrap().detail;
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
        std::fs::write(workspace_dir.path().join("sample.txt"), "some content")
            .expect("write file");
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

        // Reload from the same store dir and verify all four events restored.
        let store2 = EventStore::new(store_dir.path());
        let log2 = EventLog::load_from(store2);

        assert_eq!(log2.len(), 4);
        assert_eq!(log2.get(0).unwrap().kind, EventKind::ToolCall);
        assert_eq!(log2.get(1).unwrap().kind, EventKind::ToolResult);
        assert_eq!(log2.get(2).unwrap().kind, EventKind::ToolCall);
        assert_eq!(log2.get(3).unwrap().kind, EventKind::ToolError);
        assert!(!log2.get(0).unwrap().detail.is_empty());
        assert!(!log2.get(1).unwrap().detail.is_empty());
        assert!(!log2.get(2).unwrap().detail.is_empty());
        assert!(!log2.get(3).unwrap().detail.is_empty());
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

        let detail = &log.get(0).unwrap().detail;
        assert!(!detail.is_empty(), "ToolCall detail must be non-empty");
    }
}
