//! Read-only tool harness type definitions for Caravan.
//!
//! This module defines the type system for the tool harness. Execution logic
//! (`execute`) lands in T-2; `ReadFile`/`FileContent` variants land in T-3.

mod path;
mod search;

pub use search::SearchMatch;

use std::fs;

use path::resolve_in_workspace;

use crate::write_intent::{WriteIntentMode, WriteIntentSource, new_text};
use crate::write_preview::{WritePreview, preview_write_intent};

/// Identifies a tool by name.
#[derive(Debug, PartialEq)]
pub enum ToolName {
    ListFiles,
    ReadFile,
    SearchText,
}

/// Risk classification for a tool.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ToolRisk {
    ReadOnly,
    WorkspaceWrite,
}

impl ToolRisk {
    /// Returns the canonical snake_case string for this risk level.
    pub fn as_str(&self) -> &'static str {
        match self {
            ToolRisk::ReadOnly => "read_only",
            ToolRisk::WorkspaceWrite => "workspace_write",
        }
    }
}

/// Per-call execution context passed to every tool invocation.
#[derive(Debug, PartialEq)]
pub struct ToolExecutionContext {
    pub workspace_root: std::path::PathBuf,
}

/// Inputs accepted by the tool harness.
#[derive(Debug, PartialEq)]
pub enum ToolRequest {
    ListFiles { path: String },
    ReadFile { path: String },
    PlanWrite { path: String },
    PreviewWrite { path: String, content: String },
    SearchText { query: String },
}

/// Outputs produced by the tool harness.
#[derive(Debug, PartialEq)]
pub enum ToolOutput {
    FileList { path: String, entries: Vec<String> },
    FileContent { path: String, content: String },
    WritePreview { path: String, preview: WritePreview },
    SearchResults { query: String, matches: Vec<SearchMatch>, truncated: bool },
}

/// Structured error taxonomy for tool execution failures.
#[derive(Debug, PartialEq)]
pub enum ToolError {
    WorkspaceViolation { path: String },
    NotFound { path: String },
    NotAFile { path: String },
    NotADirectory { path: String },
    NonUtf8 { path: String },
    TooLarge { path: String, max_bytes: u64 },
    Io { message: String },
    PolicyDenied { reason: String },
    ApprovalRequired { reason: String },
}

/// Maximum bytes allowed when reading a file.
pub const MAX_READ_FILE_BYTES: u64 = 64 * 1024;

/// Stateless registry that vends tool executors.
///
/// `workspace_root` is intentionally absent here; it lives on
/// [`ToolExecutionContext`] and is supplied per call.
#[derive(Debug, PartialEq)]
pub struct ToolRegistry;

impl ToolRegistry {
    /// Creates a registry that only exposes read-only tools.
    pub fn new_readonly() -> Self {
        ToolRegistry
    }

    /// Dispatches a [`ToolRequest`] to the appropriate tool implementation.
    pub fn execute(
        &self,
        context: &ToolExecutionContext,
        request: ToolRequest,
    ) -> Result<ToolOutput, ToolError> {
        match request {
            ToolRequest::ListFiles { path } => self.list_files(context, &path),
            ToolRequest::ReadFile { path } => self.read_file(context, &path),
            ToolRequest::PlanWrite { .. } => Err(ToolError::ApprovalRequired {
                reason: "workspace_write_requires_approval".to_string(),
            }),
            ToolRequest::PreviewWrite { path, content } => {
                self.preview_write(context, path, content)
            }
            ToolRequest::SearchText { query } => {
                let outcome = search::search_workspace(&context.workspace_root, &query)?;
                Ok(ToolOutput::SearchResults {
                    query,
                    matches: outcome.matches,
                    truncated: outcome.truncated,
                })
            }
        }
    }

    /// Lists directory entries at `requested` non-recursively, sorted
    /// lexicographically.
    ///
    /// Returns [`ToolError::NotADirectory`] if the resolved path is not a
    /// directory. Uses [`Path::is_dir`] (not `fs::metadata`) so that the
    /// size-check ordering AC in T-3 stays scoped to `read_file`.
    fn list_files(
        &self,
        context: &ToolExecutionContext,
        requested: &str,
    ) -> Result<ToolOutput, ToolError> {
        let canonical = resolve_in_workspace(context, requested)?;

        if !canonical.is_dir() {
            return Err(ToolError::NotADirectory {
                path: requested.to_string(),
            });
        }

        let mut entries: Vec<String> = fs::read_dir(&canonical)
            .map_err(|e| ToolError::Io {
                message: e.to_string(),
            })?
            .filter_map(|entry| {
                entry
                    .ok()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
            })
            .collect();

        entries.sort();

        Ok(ToolOutput::FileList {
            path: requested.to_string(),
            entries,
        })
    }

    /// Runs a dry-run diff preview of a proposed write against the workspace.
    ///
    /// No file is written, no temp file is created, and no `ApprovalRequest` is
    /// recorded. All path validation and workspace-boundary checks are delegated
    /// to [`preview_write_intent`].
    fn preview_write(
        &self,
        context: &ToolExecutionContext,
        path: String,
        content: String,
    ) -> Result<ToolOutput, ToolError> {
        use crate::write_preview::WritePreviewError;

        let intent = new_text(
            &path,
            content,
            WriteIntentMode::CreateOrReplace,
            WriteIntentSource::Operator,
        )
        .map_err(|_| ToolError::WorkspaceViolation { path: path.clone() })?;

        preview_write_intent(context, &intent)
            .map(|preview| ToolOutput::WritePreview { path, preview })
            .map_err(|e| match e {
                WritePreviewError::WorkspaceViolation { path } => {
                    ToolError::WorkspaceViolation { path }
                }
                WritePreviewError::ParentNotFound { path } => ToolError::NotFound { path },
                WritePreviewError::NotAFile { path } => ToolError::NotAFile { path },
                WritePreviewError::NonUtf8 { path } => ToolError::NonUtf8 { path },
                WritePreviewError::TooLarge { path, max_bytes } => {
                    ToolError::TooLarge { path, max_bytes }
                }
                WritePreviewError::Io { message } => ToolError::Io { message },
            })
    }

    /// Reads a file's UTF-8 contents, capping at [`MAX_READ_FILE_BYTES`].
    ///
    /// The size cap is checked from file metadata **before** reading any bytes
    /// into memory. UTF-8 is validated strictly with `String::from_utf8`; lossy
    /// conversion is never used.
    fn read_file(
        &self,
        context: &ToolExecutionContext,
        requested: &str,
    ) -> Result<ToolOutput, ToolError> {
        let canonical = resolve_in_workspace(context, requested)?;

        // Use Path::is_file() (not fs::metadata) for the not-a-file guard so
        // the only fs::metadata call in this module is the size check below.
        if !canonical.is_file() {
            return Err(ToolError::NotAFile {
                path: requested.to_string(),
            });
        }

        // Size cap: check metadata BEFORE reading bytes into memory.
        let len = fs::metadata(&canonical)
            .map_err(|e| ToolError::Io {
                message: e.to_string(),
            })?
            .len();

        if len > MAX_READ_FILE_BYTES {
            return Err(ToolError::TooLarge {
                path: requested.to_string(),
                max_bytes: MAX_READ_FILE_BYTES,
            });
        }

        let bytes = fs::read(&canonical).map_err(|e| ToolError::Io {
            message: e.to_string(),
        })?;

        let content = String::from_utf8(bytes).map_err(|_| ToolError::NonUtf8 {
            path: requested.to_string(),
        })?;

        Ok(ToolOutput::FileContent {
            path: requested.to_string(),
            content,
        })
    }
}

#[cfg(test)]
mod tests;
