//! Detail-string formatters for ToolCall, ToolResult, and ToolError events.

use crate::approval::ApprovalRequest;
use crate::tool::registry::{ToolError, ToolOutput};

pub(super) fn format_approval_request_detail(request: &ApprovalRequest) -> String {
    format!(
        "tool={} path={:?} risk={} reason={}",
        request.tool, request.path, request.risk, request.reason
    )
}

pub(super) fn format_tool_call_detail(tool_name: &str, path: &str) -> String {
    format!("tool={} path={:?} risk=read_only", tool_name, path)
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
        ToolOutput::FileContent { content, .. } => {
            format!("tool={} path={:?} bytes={}", tool_name, path, content.len())
        }
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
