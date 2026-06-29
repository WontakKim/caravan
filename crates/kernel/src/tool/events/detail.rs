//! Detail-string formatters for ToolCall, ToolResult, and ToolError events.

use crate::approval::ApprovalRequest;
use crate::tool::registry::{ToolError, ToolOutput};

pub(super) fn format_approval_request_detail(request: &ApprovalRequest) -> String {
    format!(
        "tool={} path={:?} risk={} reason={}",
        request.tool, request.path, request.risk, request.reason
    )
}

pub(super) fn format_tool_call_detail(
    tool_name: &str,
    path: &str,
    offset: Option<usize>,
    limit: Option<usize>,
) -> String {
    match (offset, limit) {
        (None, None) => format!("tool={} path={:?} risk=read_only", tool_name, path),
        _ => format!(
            "tool={} path={:?} offset={} limit={} risk=read_only",
            tool_name,
            path,
            offset.unwrap_or(1),
            limit.unwrap_or(crate::tool::registry::DEFAULT_READ_RANGE_LIMIT_LINES),
        ),
    }
}

pub(super) fn format_tool_result_detail(
    tool_name: &str,
    path: &str,
    output: &ToolOutput,
) -> String {
    match output {
        ToolOutput::FileList { entries, .. } => {
            format!(
                "tool={} path={:?} entries={}",
                tool_name,
                path,
                entries.len()
            )
        }
        ToolOutput::FileContent {
            content,
            start_line,
            line_count,
            truncated,
            ..
        } => match start_line {
            None => format!("tool={} path={:?} bytes={}", tool_name, path, content.len()),
            Some(sl) => format!(
                "tool={} path={:?} start_line={} line_count={} bytes={} truncated={}",
                tool_name,
                path,
                sl,
                line_count.unwrap_or(0),
                content.len(),
                truncated,
            ),
        },
        ToolOutput::WritePreview { preview, .. } => {
            format!("tool={} {}", tool_name, preview.detail())
        }
        ToolOutput::SearchResults {
            matches, truncated, ..
        } => {
            format!(
                "tool={} path={:?} matches={} truncated={}",
                tool_name,
                path,
                matches.len(),
                truncated
            )
        }
        ToolOutput::FileMatches {
            paths, truncated, ..
        } => {
            format!(
                "tool={} pattern={:?} matches={} truncated={}",
                tool_name,
                path,
                paths.len(),
                truncated
            )
        }
    }
}

pub(super) fn format_tool_error_detail(tool_name: &str, path: &str, error: &ToolError) -> String {
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
        ToolError::PolicyDenied { .. } => "policy_denied".to_string(),
        ToolError::ApprovalRequired { .. } => "approval_required".to_string(),
        ToolError::InvalidPattern { .. } => "invalid_pattern".to_string(),
    };
    format!("tool={} path={:?} error={}", tool_name, path, token)
}
