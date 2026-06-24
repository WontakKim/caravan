//! Pure parser for Caravan tool-request blocks in assistant text.
//!
//! This module is intentionally free of filesystem access and path-safety
//! validation; those concerns belong to the `ToolRegistry` execution stage.

use crate::tool::registry::ToolRequest;

const OPEN_DELIMITER: &str = "CARAVAN_TOOL_REQUEST";
const CLOSE_DELIMITER: &str = "END_CARAVAN_TOOL_REQUEST";

/// The kind of tool being requested.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelToolRequestKind {
    ReadFile,
    ListFiles,
}

/// A typed tool request parsed from a `CARAVAN_TOOL_REQUEST` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelToolRequest {
    pub kind: ModelToolRequestKind,
    pub path: String,
}

impl ModelToolRequest {
    /// Returns a structured detail string for logging/display purposes.
    ///
    /// Format: `source=model tool=<tool> path="<path>" risk=read_only status=detected`
    pub fn detail(&self) -> String {
        let tool = match self.kind {
            ModelToolRequestKind::ReadFile => "read_file",
            ModelToolRequestKind::ListFiles => "list_files",
        };
        format!(
            "source=model tool={tool} path=\"{path}\" risk=read_only status=detected",
            tool = tool,
            path = self.path,
        )
    }

    /// Returns the exact `/tool` command the user should run next.
    pub fn suggested_command(&self) -> String {
        match self.kind {
            ModelToolRequestKind::ReadFile => format!("/tool read {}", self.path),
            ModelToolRequestKind::ListFiles => format!("/tool list {}", self.path),
        }
    }

    /// Returns a multi-line screen-log message conveying what happened and the next steps.
    pub fn user_guidance(&self) -> Vec<String> {
        vec![
            format!("Model requested read-only tool: {}", self.detail()),
            "Caravan did not execute it automatically.".to_string(),
            format!("Run: {}", self.suggested_command()),
            "Then run: /context attach-last-tool".to_string(),
        ]
    }

    /// Converts this `ModelToolRequest` into a `ToolRequest` suitable for
    /// passing to `ToolRegistry::execute`.
    pub fn to_tool_request(&self) -> ToolRequest {
        match self.kind {
            ModelToolRequestKind::ReadFile => ToolRequest::ReadFile {
                path: self.path.clone(),
            },
            ModelToolRequestKind::ListFiles => ToolRequest::ListFiles {
                path: self.path.clone(),
            },
        }
    }
}

/// Parses the first valid Caravan tool-request block from `text`.
///
/// Delimiter lines must match EXACTLY (no surrounding whitespace). Key lines
/// inside a block (`tool=`, `path=`) are trimmed. Returns `None` if no valid
/// block is found. Never panics and never accesses the filesystem.
pub fn parse_first_model_tool_request(text: &str) -> Option<ModelToolRequest> {
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        // Opening delimiter must match exactly — no trimming.
        if lines[i] != OPEN_DELIMITER {
            i += 1;
            continue;
        }
        let block_start = i + 1;
        i += 1;

        // Scan forward for the exact closing delimiter.
        let mut close_pos = None;
        while i < lines.len() {
            if lines[i] == CLOSE_DELIMITER {
                close_pos = Some(i);
                break;
            }
            i += 1;
        }

        let close_pos = match close_pos {
            Some(pos) => pos,
            // No closing delimiter — rest of the document is consumed; give up.
            None => return None,
        };

        // Parse key lines within block_start..close_pos (whitespace is trimmed).
        let block_lines = &lines[block_start..close_pos];
        let mut tool_value: Option<&str> = None;
        let mut path_value: Option<&str> = None;

        for line in block_lines {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("tool=") {
                tool_value = Some(rest.trim());
            } else if let Some(rest) = trimmed.strip_prefix("path=") {
                path_value = Some(rest.trim());
            }
        }

        // Advance past the closing delimiter before evaluating this block.
        i = close_pos + 1;

        let tool = match tool_value {
            Some(t) => t,
            None => continue, // No tool= line — invalid block, skip.
        };

        match tool {
            "read_file" => {
                let path = match path_value {
                    Some(p) if !p.is_empty() => p.to_string(),
                    // Missing or whitespace-only path is invalid for read_file.
                    _ => continue,
                };
                return Some(ModelToolRequest {
                    kind: ModelToolRequestKind::ReadFile,
                    path,
                });
            }
            "list_files" => {
                let path = match path_value {
                    Some(p) if !p.is_empty() => p.to_string(),
                    // Missing or empty path defaults to the current directory.
                    _ => ".".to_string(),
                };
                return Some(ModelToolRequest {
                    kind: ModelToolRequestKind::ListFiles,
                    path,
                });
            }
            // Unsupported tool — skip this block and continue scanning.
            _ => continue,
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_block(tool: &str, path: Option<&str>) -> String {
        let mut lines = vec!["CARAVAN_TOOL_REQUEST".to_string()];
        lines.push(format!("tool={}", tool));
        if let Some(p) = path {
            lines.push(format!("path={}", p));
        }
        lines.push("END_CARAVAN_TOOL_REQUEST".to_string());
        lines.join("\n")
    }

    #[test]
    fn parse_read_file_with_valid_path() {
        let text = make_block("read_file", Some("src/main.rs"));
        let result = parse_first_model_tool_request(&text);
        assert_eq!(
            result,
            Some(ModelToolRequest {
                kind: ModelToolRequestKind::ReadFile,
                path: "src/main.rs".to_string(),
            })
        );
    }

    #[test]
    fn parse_list_files_with_valid_path() {
        let text = make_block("list_files", Some("src/"));
        let result = parse_first_model_tool_request(&text);
        assert_eq!(
            result,
            Some(ModelToolRequest {
                kind: ModelToolRequestKind::ListFiles,
                path: "src/".to_string(),
            })
        );
    }

    #[test]
    fn list_files_missing_path_defaults_to_dot() {
        let text = make_block("list_files", None);
        let result = parse_first_model_tool_request(&text);
        assert_eq!(
            result,
            Some(ModelToolRequest {
                kind: ModelToolRequestKind::ListFiles,
                path: ".".to_string(),
            })
        );
    }

    #[test]
    fn read_file_missing_path_returns_none() {
        let text = make_block("read_file", None);
        let result = parse_first_model_tool_request(&text);
        assert_eq!(result, None);
    }

    #[test]
    fn read_file_whitespace_only_path_returns_none() {
        let text = make_block("read_file", Some("   "));
        let result = parse_first_model_tool_request(&text);
        assert_eq!(result, None);
    }

    #[test]
    fn unsupported_tool_write_file_returns_none() {
        let text = make_block("write_file", Some("output.txt"));
        let result = parse_first_model_tool_request(&text);
        assert_eq!(result, None);
    }

    /// A `write_file` block must be skipped entirely; a subsequent valid block is returned.
    /// This guards against the parser accidentally accepting `write_file` as a model tool.
    #[test]
    fn write_file_block_before_valid_read_file_is_skipped() {
        let write_block = make_block("write_file", Some("output.txt"));
        let read_block = make_block("read_file", Some("README.md"));
        let text = format!("{}\n{}", write_block, read_block);
        let result = parse_first_model_tool_request(&text);
        assert_eq!(
            result,
            Some(ModelToolRequest {
                kind: ModelToolRequestKind::ReadFile,
                path: "README.md".to_string(),
            }),
            "write_file block must be skipped; the subsequent read_file block must be returned"
        );
    }

    #[test]
    fn malformed_block_no_closing_delimiter_returns_none() {
        let text = "CARAVAN_TOOL_REQUEST\ntool=read_file\npath=README.md\n";
        let result = parse_first_model_tool_request(text);
        assert_eq!(result, None);
    }

    #[test]
    fn delimiter_with_surrounding_whitespace_not_recognized() {
        let text =
            "  CARAVAN_TOOL_REQUEST  \ntool=read_file\npath=README.md\nEND_CARAVAN_TOOL_REQUEST\n";
        let result = parse_first_model_tool_request(text);
        assert_eq!(result, None);
    }

    #[test]
    fn multiple_valid_blocks_returns_first() {
        let first = make_block("read_file", Some("first.rs"));
        let second = make_block("list_files", Some("src/"));
        let text = format!("{}\n{}", first, second);
        let result = parse_first_model_tool_request(&text);
        assert_eq!(
            result,
            Some(ModelToolRequest {
                kind: ModelToolRequestKind::ReadFile,
                path: "first.rs".to_string(),
            })
        );
    }

    #[test]
    fn unsupported_block_before_valid_block_is_skipped() {
        let shell_block = make_block("shell", None);
        let valid_block = make_block("read_file", Some("README.md"));
        let text = format!("{}\n{}", shell_block, valid_block);
        let result = parse_first_model_tool_request(&text);
        assert_eq!(
            result,
            Some(ModelToolRequest {
                kind: ModelToolRequestKind::ReadFile,
                path: "README.md".to_string(),
            })
        );
    }

    #[test]
    fn detail_read_file_exact_string() {
        let req = ModelToolRequest {
            kind: ModelToolRequestKind::ReadFile,
            path: "README.md".to_string(),
        };
        assert_eq!(
            req.detail(),
            "source=model tool=read_file path=\"README.md\" risk=read_only status=detected"
        );
    }

    #[test]
    fn detail_list_files_default_path_exact_string() {
        let req = ModelToolRequest {
            kind: ModelToolRequestKind::ListFiles,
            path: ".".to_string(),
        };
        assert_eq!(
            req.detail(),
            "source=model tool=list_files path=\".\" risk=read_only status=detected"
        );
    }

    #[test]
    fn suggested_command_read_file_returns_tool_read_path() {
        let req = ModelToolRequest {
            kind: ModelToolRequestKind::ReadFile,
            path: "src/main.rs".to_string(),
        };
        assert_eq!(req.suggested_command(), "/tool read src/main.rs");
    }

    #[test]
    fn suggested_command_list_files_returns_tool_list_path() {
        let req = ModelToolRequest {
            kind: ModelToolRequestKind::ListFiles,
            path: "src/".to_string(),
        };
        assert_eq!(req.suggested_command(), "/tool list src/");
    }

    #[test]
    fn user_guidance_contains_suggested_command_line() {
        let req = ModelToolRequest {
            kind: ModelToolRequestKind::ReadFile,
            path: "README.md".to_string(),
        };
        let guidance = req.user_guidance();
        let expected_run_line = format!("Run: {}", req.suggested_command());
        assert!(
            guidance.iter().any(|line| line == &expected_run_line),
            "expected guidance to contain '{}'",
            expected_run_line
        );
    }

    #[test]
    fn user_guidance_contains_attach_last_tool() {
        let req = ModelToolRequest {
            kind: ModelToolRequestKind::ListFiles,
            path: ".".to_string(),
        };
        let guidance = req.user_guidance();
        assert!(
            guidance
                .iter()
                .any(|line| line.contains("/context attach-last-tool")),
            "expected guidance to contain '/context attach-last-tool'"
        );
    }

    #[test]
    fn user_guidance_contains_did_not_execute_automatically() {
        let req = ModelToolRequest {
            kind: ModelToolRequestKind::ReadFile,
            path: "Cargo.toml".to_string(),
        };
        let guidance = req.user_guidance();
        assert!(
            guidance
                .iter()
                .any(|line| line.contains("did not execute it automatically")),
            "expected guidance to contain 'did not execute it automatically'"
        );
    }

    #[test]
    fn model_tool_request_read_file_to_tool_request() {
        let req = ModelToolRequest {
            kind: ModelToolRequestKind::ReadFile,
            path: "src/main.rs".to_string(),
        };
        assert_eq!(
            req.to_tool_request(),
            ToolRequest::ReadFile {
                path: "src/main.rs".to_string(),
            }
        );
    }

    #[test]
    fn model_tool_request_list_files_to_tool_request() {
        let req = ModelToolRequest {
            kind: ModelToolRequestKind::ListFiles,
            path: "src/".to_string(),
        };
        assert_eq!(
            req.to_tool_request(),
            ToolRequest::ListFiles {
                path: "src/".to_string(),
            }
        );
    }

    // Model-visibility rejection: the parser must NOT accept preview_write,
    // preview-write, or write_file as valid tool names. They must all return None.
    #[test]
    fn preview_write_snake_case_is_rejected_by_model_parser() {
        let text = make_block("preview_write", Some("README.md"));
        let result = parse_first_model_tool_request(&text);
        assert_eq!(
            result, None,
            "preview_write must not be accepted by the model tool-call parser"
        );
    }

    #[test]
    fn preview_write_kebab_case_is_rejected_by_model_parser() {
        let text = make_block("preview-write", Some("README.md"));
        let result = parse_first_model_tool_request(&text);
        assert_eq!(
            result, None,
            "preview-write must not be accepted by the model tool-call parser"
        );
    }

    #[test]
    fn write_file_is_rejected_by_model_parser() {
        let text = make_block("write_file", Some("output.txt"));
        let result = parse_first_model_tool_request(&text);
        assert_eq!(
            result, None,
            "write_file must not be accepted by the model tool-call parser"
        );
    }
}
