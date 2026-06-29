use crate::model::{ModelError, ModelOutput, ModelRequest, ModelResult, ModelUsage};
use crate::tool::registry::{ToolError, ToolOutput, ToolRequest};

/// Maximum bytes the model-facing tool result string may occupy.
pub const MODEL_TOOL_RESULT_MAX_BYTES: usize = 16 * 1024;

/// Validates a native model tool call into a [`ToolRequest`] without accessing
/// the filesystem.
///
/// The model schema is guidance, not a trust boundary — arguments are treated
/// as untrusted input and validated independently. Malformed or unsupported
/// inputs return [`ModelError::AdapterFailure`].
pub fn model_tool_call_to_request(call: &ModelToolCall) -> ModelResult<ToolRequest> {
    let obj = call
        .arguments
        .as_object()
        .ok_or_else(|| ModelError::AdapterFailure {
            message: "malformed_tool_arguments: arguments must be a JSON object".to_string(),
        })?;

    match call.name.as_str() {
        "list_files" => {
            // Reject unknown fields — mirrors the schema's additionalProperties: false.
            if obj.keys().any(|k| k != "path") {
                return Err(ModelError::AdapterFailure {
                    message: "malformed_tool_arguments: list_files contains an unsupported field"
                        .to_string(),
                });
            }
            let path = match obj.get("path") {
                None => ".".to_string(),
                Some(serde_json::Value::String(s)) => {
                    if s.is_empty() {
                        ".".to_string()
                    } else {
                        s.clone()
                    }
                }
                Some(_) => {
                    return Err(ModelError::AdapterFailure {
                        message: "malformed_tool_arguments: list_files path must be a string"
                            .to_string(),
                    });
                }
            };
            Ok(ToolRequest::ListFiles { path })
        }
        "read_file" => {
            // Reject unknown fields — mirrors the schema's additionalProperties: false.
            if obj.keys().any(|k| k != "path") {
                return Err(ModelError::AdapterFailure {
                    message: "malformed_tool_arguments: read_file contains an unsupported field"
                        .to_string(),
                });
            }
            let path = match obj.get("path") {
                Some(serde_json::Value::String(s)) if !s.is_empty() => s.clone(),
                _ => {
                    return Err(ModelError::AdapterFailure {
                        message: "malformed_tool_arguments: read_file requires a non-empty path"
                            .to_string(),
                    });
                }
            };
            Ok(ToolRequest::ReadFile { path })
        }
        "search_text" => {
            // Reject unknown fields — mirrors the schema's additionalProperties: false.
            if obj.keys().any(|k| k != "query") {
                return Err(ModelError::AdapterFailure {
                    message: "malformed_tool_arguments: search_text contains an unsupported field"
                        .to_string(),
                });
            }
            let query = match obj.get("query") {
                Some(serde_json::Value::String(s)) if !s.is_empty() => s.clone(),
                _ => {
                    return Err(ModelError::AdapterFailure {
                        message: "malformed_tool_arguments: search_text requires a non-empty query"
                            .to_string(),
                    });
                }
            };
            Ok(ToolRequest::SearchText { query })
        }
        name => Err(ModelError::AdapterFailure {
            message: format!("unsupported_model_tool: {name}"),
        }),
    }
}

/// Truncates `rendered` so its total byte length stays `<= MODEL_TOOL_RESULT_MAX_BYTES`,
/// appending `"\n... [truncated]"` when a cut is needed. The cut is always on a
/// UTF-8 character boundary.
fn limit_model_tool_text(rendered: String) -> String {
    if rendered.len() <= MODEL_TOOL_RESULT_MAX_BYTES {
        return rendered;
    }
    const SUFFIX: &str = "\n... [truncated]";
    let keep = MODEL_TOOL_RESULT_MAX_BYTES - SUFFIX.len();
    // Truncate on a UTF-8 char boundary.
    let mut cut = keep;
    while !rendered.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}{}", &rendered[..cut], SUFFIX)
}

/// Formats a [`ToolOutput`] into bounded text for the follow-up model call.
///
/// The result is truncated to [`MODEL_TOOL_RESULT_MAX_BYTES`] if needed,
/// appending a `"\n... [truncated]"` suffix so the total stays within the
/// limit.
pub fn format_tool_output_for_model(output: &ToolOutput) -> String {
    let rendered = match output {
        ToolOutput::FileList { path, entries } => {
            format!("Directory: {}\n{}", path, entries.join("\n"))
        }
        ToolOutput::FileContent { path, content } => {
            format!("File: {}\n{}", path, content)
        }
        ToolOutput::WritePreview { .. } => {
            "[write preview not available on the read-only path]".to_string()
        }
        ToolOutput::SearchResults {
            query,
            matches,
            truncated,
        } => {
            let mut lines = vec![format!("Search results for \"{}\":", query)];
            for m in matches {
                lines.push(format!("{}:{}: {}", m.path, m.line, m.text));
            }
            if *truncated {
                lines.push("... [truncated]".to_string());
            }
            lines.join("\n")
        }
        ToolOutput::FileMatches {
            pattern,
            paths,
            truncated,
        } => {
            let mut lines = vec![format!("Glob pattern: {pattern}")];
            for p in paths {
                lines.push(p.clone());
            }
            if *truncated {
                lines.push("... [truncated]".to_string());
            }
            lines.join("\n")
        }
    };
    limit_model_tool_text(rendered)
}

/// Formats a [`ToolError`] into a human-readable error string for the model.
///
/// The string always begins with `"Error: "` because the OpenAI tool message
/// carries no machine-readable `is_error` flag — the model only sees this text.
/// Raw OS error details and secrets are never embedded. The result is bounded
/// to [`MODEL_TOOL_RESULT_MAX_BYTES`] via [`limit_model_tool_text`] to guard
/// against arbitrarily long paths embedded in error variants.
pub fn format_tool_error_for_model(error: &ToolError) -> String {
    let rendered = match error {
        ToolError::NotFound { path } => {
            format!("Error: file or directory not found: {path}")
        }
        ToolError::NotAFile { path } => {
            format!("Error: not a file: {path}")
        }
        ToolError::NotADirectory { path } => {
            format!("Error: not a directory: {path}")
        }
        ToolError::NonUtf8 { path } => {
            format!("Error: file is not valid UTF-8 text: {path}")
        }
        ToolError::TooLarge { path, max_bytes } => {
            format!("Error: file too large (max {max_bytes} bytes): {path}")
        }
        ToolError::WorkspaceViolation { path } => {
            format!("Error: path is outside the workspace: {path}")
        }
        ToolError::Io { .. } => "Error: I/O error while accessing the workspace.".to_string(),
        ToolError::PolicyDenied { .. } | ToolError::ApprovalRequired { .. } => {
            "Error: this operation is not permitted by the active safety policy.".to_string()
        }
        ToolError::InvalidPattern { pattern } => {
            format!("Error: invalid glob pattern: {:?}", pattern)
        }
    };
    limit_model_tool_text(rendered)
}

/// A tool definition passed to the model so it knows what tools are available.
#[derive(Debug, Clone)]
pub struct ModelToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// A tool call the model decided to make.
#[derive(Debug, Clone)]
pub struct ModelToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// The result of executing a tool call, ready to feed back to the model.
#[derive(Debug, Clone)]
pub struct ModelToolResult {
    pub tool_call_id: String,
    pub name: String,
    pub content: String,
    pub is_error: bool,
}

/// A paired tool call and its result from a prior turn.
#[derive(Debug, Clone)]
pub struct ModelToolExchange {
    pub call: ModelToolCall,
    pub result: ModelToolResult,
}

/// A single step request: a base model request plus available tools and any
/// prior tool exchanges to include in the conversation history (in order).
pub struct ModelStepRequest {
    pub request: ModelRequest,
    pub tools: Vec<ModelToolDefinition>,
    pub prior_tool_exchanges: Vec<ModelToolExchange>,
}

/// The output of a single model step: either a plain assistant reply or a
/// tool call the model wants to make.
pub enum ModelStepOutput {
    Assistant(ModelOutput),
    ToolCall {
        call: ModelToolCall,
        usage: Option<ModelUsage>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::types::{ModelAdapterKind, ModelProvider};
    use crate::model::{MockModelAdapter, ModelAdapter, ModelAdapterContext, ModelRequest};

    fn mock_context() -> ModelAdapterContext {
        ModelAdapterContext {
            provider: ModelProvider::Mock,
            model: "mock-model".into(),
            adapter: ModelAdapterKind::MockModelAdapter,
        }
    }

    #[test]
    fn tool_definition_construction_and_clone() {
        let def = ModelToolDefinition {
            name: "search".into(),
            description: "Search the web".into(),
            input_schema: serde_json::json!({"type": "object"}),
        };
        let cloned = def.clone();
        assert_eq!(cloned.name, "search");
        assert_eq!(cloned.description, "Search the web");
    }

    #[test]
    fn tool_call_construction_and_clone() {
        let call = ModelToolCall {
            id: "call-1".into(),
            name: "search".into(),
            arguments: serde_json::json!({"query": "rust"}),
        };
        let cloned = call.clone();
        assert_eq!(cloned.id, "call-1");
        assert_eq!(cloned.name, "search");
    }

    #[test]
    fn tool_result_construction_and_clone() {
        let result = ModelToolResult {
            tool_call_id: "call-1".into(),
            name: "search".into(),
            content: "found results".into(),
            is_error: false,
        };
        let cloned = result.clone();
        assert_eq!(cloned.tool_call_id, "call-1");
        assert!(!cloned.is_error);
    }

    #[test]
    fn tool_exchange_construction_and_clone() {
        let call = ModelToolCall {
            id: "call-2".into(),
            name: "run".into(),
            arguments: serde_json::json!({}),
        };
        let result = ModelToolResult {
            tool_call_id: "call-2".into(),
            name: "run".into(),
            content: "ok".into(),
            is_error: false,
        };
        let exchange = ModelToolExchange {
            call: call.clone(),
            result: result.clone(),
        };
        let cloned = exchange.clone();
        assert_eq!(cloned.call.id, "call-2");
        assert_eq!(cloned.result.content, "ok");
    }

    #[test]
    fn model_request_clone_equality() {
        let req = ModelRequest {
            prompt: "system prompt".into(),
            user_message: "hello".into(),
        };
        let cloned = req.clone();
        assert_eq!(cloned.prompt, req.prompt);
        assert_eq!(cloned.user_message, req.user_message);
    }

    #[test]
    fn step_output_assistant_variant() {
        let output = ModelOutput {
            response: "hi".into(),
            chunks: vec!["hi".into()],
            usage: None,
        };
        let step_output = ModelStepOutput::Assistant(output);
        match step_output {
            ModelStepOutput::Assistant(o) => assert_eq!(o.response, "hi"),
            ModelStepOutput::ToolCall { .. } => panic!("expected Assistant variant"),
        }
    }

    #[test]
    fn step_output_tool_call_variant() {
        let call = ModelToolCall {
            id: "c1".into(),
            name: "tool".into(),
            arguments: serde_json::json!({}),
        };
        let step_output = ModelStepOutput::ToolCall { call, usage: None };
        match step_output {
            ModelStepOutput::ToolCall { call, usage } => {
                assert_eq!(call.id, "c1");
                assert!(usage.is_none());
            }
            ModelStepOutput::Assistant(_) => panic!("expected ToolCall variant"),
        }
    }

    #[test]
    fn mock_adapter_complete_step_returns_assistant() {
        let request = ModelRequest {
            prompt: "sys".into(),
            user_message: "greet".into(),
        };
        let step_request = ModelStepRequest {
            request,
            tools: vec![],
            prior_tool_exchanges: vec![],
        };
        let result = MockModelAdapter.complete_step(&mock_context(), &step_request);
        let output = result.unwrap();
        match output {
            ModelStepOutput::Assistant(o) => {
                assert_eq!(o.response, "Mock response for: greet");
            }
            ModelStepOutput::ToolCall { .. } => panic!("expected Assistant variant"),
        }
    }

    // --- model_tool_call_to_request tests ---

    fn make_call(name: &str, arguments: serde_json::Value) -> ModelToolCall {
        ModelToolCall {
            id: "test-id".into(),
            name: name.into(),
            arguments,
        }
    }

    #[test]
    fn list_files_with_string_path_returns_list_files_request() {
        let call = make_call("list_files", serde_json::json!({"path": "src/"}));
        let result = model_tool_call_to_request(&call).unwrap();
        assert_eq!(
            result,
            crate::tool::registry::ToolRequest::ListFiles {
                path: "src/".to_string(),
            }
        );
    }

    #[test]
    fn list_files_missing_path_defaults_to_dot() {
        let call = make_call("list_files", serde_json::json!({}));
        let result = model_tool_call_to_request(&call).unwrap();
        assert_eq!(
            result,
            crate::tool::registry::ToolRequest::ListFiles {
                path: ".".to_string(),
            }
        );
    }

    #[test]
    fn list_files_empty_string_path_defaults_to_dot() {
        let call = make_call("list_files", serde_json::json!({"path": ""}));
        let result = model_tool_call_to_request(&call).unwrap();
        assert_eq!(
            result,
            crate::tool::registry::ToolRequest::ListFiles {
                path: ".".to_string(),
            }
        );
    }

    #[test]
    fn list_files_present_non_string_path_returns_adapter_failure() {
        let call = make_call("list_files", serde_json::json!({"path": 123}));
        let err = model_tool_call_to_request(&call).unwrap_err();
        assert!(
            matches!(err, crate::model::ModelError::AdapterFailure { .. }),
            "expected AdapterFailure, got: {err:?}"
        );
    }

    #[test]
    fn list_files_non_object_arguments_returns_adapter_failure() {
        let call = make_call("list_files", serde_json::json!(["src/"]));
        let err = model_tool_call_to_request(&call).unwrap_err();
        assert!(
            matches!(err, crate::model::ModelError::AdapterFailure { .. }),
            "expected AdapterFailure for array arguments"
        );
    }

    #[test]
    fn read_file_non_object_arguments_returns_adapter_failure() {
        let call = make_call("read_file", serde_json::json!("src/main.rs"));
        let err = model_tool_call_to_request(&call).unwrap_err();
        assert!(
            matches!(err, crate::model::ModelError::AdapterFailure { .. }),
            "expected AdapterFailure for string arguments"
        );
    }

    #[test]
    fn read_file_with_valid_path_returns_read_file_request() {
        let call = make_call("read_file", serde_json::json!({"path": "Cargo.toml"}));
        let result = model_tool_call_to_request(&call).unwrap();
        assert_eq!(
            result,
            crate::tool::registry::ToolRequest::ReadFile {
                path: "Cargo.toml".to_string(),
            }
        );
    }

    #[test]
    fn read_file_missing_path_returns_adapter_failure() {
        let call = make_call("read_file", serde_json::json!({}));
        let err = model_tool_call_to_request(&call).unwrap_err();
        assert!(matches!(
            err,
            crate::model::ModelError::AdapterFailure { .. }
        ));
    }

    #[test]
    fn read_file_empty_path_returns_adapter_failure() {
        let call = make_call("read_file", serde_json::json!({"path": ""}));
        let err = model_tool_call_to_request(&call).unwrap_err();
        assert!(matches!(
            err,
            crate::model::ModelError::AdapterFailure { .. }
        ));
    }

    #[test]
    fn read_file_non_string_path_returns_adapter_failure() {
        let call = make_call("read_file", serde_json::json!({"path": false}));
        let err = model_tool_call_to_request(&call).unwrap_err();
        assert!(matches!(
            err,
            crate::model::ModelError::AdapterFailure { .. }
        ));
    }

    #[test]
    fn unsupported_tool_returns_adapter_failure() {
        let call = make_call("shell_exec", serde_json::json!({"cmd": "ls"}));
        let err = model_tool_call_to_request(&call).unwrap_err();
        match err {
            crate::model::ModelError::AdapterFailure { message } => {
                assert!(
                    message.contains("unsupported_model_tool"),
                    "expected unsupported_model_tool in message, got: {message}"
                );
            }
            other => panic!("expected AdapterFailure, got: {other:?}"),
        }
    }

    // --- format_tool_output_for_model tests ---

    #[test]
    fn file_list_formatting_includes_directory_header_and_newline_joined_entries() {
        let output = crate::tool::registry::ToolOutput::FileList {
            path: "src/".to_string(),
            entries: vec!["main.rs".to_string(), "lib.rs".to_string()],
        };
        let formatted = format_tool_output_for_model(&output);
        assert!(formatted.starts_with("Directory: src/\n"));
        assert!(formatted.contains("main.rs\nlib.rs"));
    }

    #[test]
    fn file_content_formatting_includes_file_header() {
        let output = crate::tool::registry::ToolOutput::FileContent {
            path: "README.md".to_string(),
            content: "hello world".to_string(),
        };
        let formatted = format_tool_output_for_model(&output);
        assert_eq!(formatted, "File: README.md\nhello world");
    }

    #[test]
    fn oversized_content_is_truncated_within_max_bytes() {
        // Create content that will exceed MODEL_TOOL_RESULT_MAX_BYTES when combined with the header.
        let header = "File: big.txt\n";
        let body_size = MODEL_TOOL_RESULT_MAX_BYTES; // definitely over the limit
        let content = "x".repeat(body_size);
        let output = crate::tool::registry::ToolOutput::FileContent {
            path: "big.txt".to_string(),
            content,
        };
        let formatted = format_tool_output_for_model(&output);
        assert!(
            formatted.len() <= MODEL_TOOL_RESULT_MAX_BYTES,
            "output length {} exceeds MODEL_TOOL_RESULT_MAX_BYTES {}",
            formatted.len(),
            MODEL_TOOL_RESULT_MAX_BYTES
        );
        assert!(
            formatted.ends_with("\n... [truncated]"),
            "expected truncation suffix"
        );
        // Suppress unused variable warning in release mode.
        let _ = header;
    }

    // --- format_tool_error_for_model tests ---

    #[test]
    fn all_tool_error_variants_produce_error_prefix() {
        use crate::tool::registry::ToolError;
        let errors: Vec<ToolError> = vec![
            ToolError::NotFound {
                path: "a.txt".to_string(),
            },
            ToolError::NotAFile {
                path: "dir/".to_string(),
            },
            ToolError::NotADirectory {
                path: "file.rs".to_string(),
            },
            ToolError::NonUtf8 {
                path: "bin.dat".to_string(),
            },
            ToolError::TooLarge {
                path: "big.bin".to_string(),
                max_bytes: 65536,
            },
            ToolError::WorkspaceViolation {
                path: "../escape".to_string(),
            },
            ToolError::Io {
                message: "permission denied (os error 13)".to_string(),
            },
            ToolError::PolicyDenied {
                reason: "blocked".to_string(),
            },
            ToolError::ApprovalRequired {
                reason: "write_op".to_string(),
            },
        ];
        for err in &errors {
            let summary = format_tool_error_for_model(err);
            assert!(!summary.is_empty(), "summary must not be empty for {err:?}");
            assert!(
                summary.starts_with("Error: "),
                "summary must begin with 'Error: ' for {err:?}, got: {summary:?}"
            );
        }
    }

    #[test]
    fn io_error_summary_contains_no_raw_os_detail() {
        use crate::tool::registry::ToolError;
        let err = ToolError::Io {
            message: "permission denied (os error 13)".to_string(),
        };
        let summary = format_tool_error_for_model(&err);
        // The raw OS message must not appear in the output.
        assert!(
            !summary.contains("permission denied (os error 13)"),
            "IO summary must not embed raw OS detail: {summary:?}"
        );
        assert!(summary.starts_with("Error: "));
    }

    #[test]
    fn not_found_summary_contains_path() {
        use crate::tool::registry::ToolError;
        let err = ToolError::NotFound {
            path: "missing.txt".to_string(),
        };
        let summary = format_tool_error_for_model(&err);
        assert!(summary.contains("missing.txt"));
        assert!(summary.starts_with("Error: "));
    }

    #[test]
    fn too_large_summary_contains_max_bytes_and_path() {
        use crate::tool::registry::ToolError;
        let err = ToolError::TooLarge {
            path: "huge.bin".to_string(),
            max_bytes: 65536,
        };
        let summary = format_tool_error_for_model(&err);
        assert!(summary.contains("65536"));
        assert!(summary.contains("huge.bin"));
        assert!(summary.starts_with("Error: "));
    }

    // --- G-2: unknown-field rejection tests ---

    #[test]
    fn list_files_with_unknown_field_returns_adapter_failure() {
        let call = make_call(
            "list_files",
            serde_json::json!({"path": ".", "unexpected": true}),
        );
        let err = model_tool_call_to_request(&call).unwrap_err();
        match err {
            crate::model::ModelError::AdapterFailure { message } => {
                assert!(
                    message.contains("unsupported field"),
                    "expected 'unsupported field' in message, got: {message}"
                );
            }
            other => panic!("expected AdapterFailure, got: {other:?}"),
        }
    }

    #[test]
    fn read_file_with_unknown_field_returns_adapter_failure() {
        let call = make_call("read_file", serde_json::json!({"path": "a", "extra": 1}));
        let err = model_tool_call_to_request(&call).unwrap_err();
        match err {
            crate::model::ModelError::AdapterFailure { message } => {
                assert!(
                    message.contains("unsupported field"),
                    "expected 'unsupported field' in message, got: {message}"
                );
            }
            other => panic!("expected AdapterFailure, got: {other:?}"),
        }
    }

    // --- search_text validation tests ---

    #[test]
    fn search_text_with_valid_query_returns_search_text_request() {
        let call = make_call("search_text", serde_json::json!({"query": "TODO"}));
        let result = model_tool_call_to_request(&call).unwrap();
        assert_eq!(
            result,
            crate::tool::registry::ToolRequest::SearchText {
                query: "TODO".to_string(),
            }
        );
    }

    #[test]
    fn search_text_missing_query_returns_adapter_failure() {
        let call = make_call("search_text", serde_json::json!({}));
        let err = model_tool_call_to_request(&call).unwrap_err();
        match err {
            crate::model::ModelError::AdapterFailure { message } => {
                assert!(
                    message.contains("malformed_tool_arguments"),
                    "expected malformed_tool_arguments in message, got: {message}"
                );
            }
            other => panic!("expected AdapterFailure, got: {other:?}"),
        }
    }

    #[test]
    fn search_text_empty_query_returns_adapter_failure() {
        let call = make_call("search_text", serde_json::json!({"query": ""}));
        let err = model_tool_call_to_request(&call).unwrap_err();
        assert!(
            matches!(err, crate::model::ModelError::AdapterFailure { .. }),
            "expected AdapterFailure for empty query"
        );
    }

    #[test]
    fn search_text_non_string_query_returns_adapter_failure() {
        let call = make_call("search_text", serde_json::json!({"query": 42}));
        let err = model_tool_call_to_request(&call).unwrap_err();
        assert!(
            matches!(err, crate::model::ModelError::AdapterFailure { .. }),
            "expected AdapterFailure for non-string query"
        );
    }

    #[test]
    fn search_text_unknown_extra_field_returns_adapter_failure() {
        let call = make_call(
            "search_text",
            serde_json::json!({"query": "TODO", "extra": true}),
        );
        let err = model_tool_call_to_request(&call).unwrap_err();
        match err {
            crate::model::ModelError::AdapterFailure { message } => {
                assert!(
                    message.contains("unsupported field"),
                    "expected 'unsupported field' in message, got: {message}"
                );
            }
            other => panic!("expected AdapterFailure, got: {other:?}"),
        }
    }

    #[test]
    fn search_text_non_object_arguments_returns_adapter_failure() {
        let call = make_call("search_text", serde_json::json!("not an object"));
        let err = model_tool_call_to_request(&call).unwrap_err();
        assert!(
            matches!(err, crate::model::ModelError::AdapterFailure { .. }),
            "expected AdapterFailure for non-object arguments"
        );
    }

    #[test]
    fn grep_tool_name_returns_unsupported_model_tool() {
        let call = make_call("grep", serde_json::json!({"pattern": "TODO"}));
        let err = model_tool_call_to_request(&call).unwrap_err();
        match err {
            crate::model::ModelError::AdapterFailure { message } => {
                assert!(
                    message.contains("unsupported_model_tool"),
                    "expected unsupported_model_tool in message, got: {message}"
                );
            }
            other => panic!("expected AdapterFailure, got: {other:?}"),
        }
    }

    // --- G-3: bounded error output test ---

    #[test]
    fn not_found_error_with_very_long_path_is_bounded() {
        use crate::tool::registry::ToolError;
        // Path long enough to push the error message well over MODEL_TOOL_RESULT_MAX_BYTES.
        let long_path = "x".repeat(MODEL_TOOL_RESULT_MAX_BYTES + 1024);
        let err = ToolError::NotFound { path: long_path };
        let summary = format_tool_error_for_model(&err);
        assert!(
            summary.len() <= MODEL_TOOL_RESULT_MAX_BYTES,
            "error output length {} exceeds MODEL_TOOL_RESULT_MAX_BYTES {}",
            summary.len(),
            MODEL_TOOL_RESULT_MAX_BYTES
        );
        assert!(
            summary.ends_with("\n... [truncated]"),
            "expected truncation suffix, got: {summary:?}"
        );
    }
}
