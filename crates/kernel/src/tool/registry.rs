//! Read-only tool harness type definitions for Caravan.
//!
//! This module defines the type system for the tool harness. Execution logic
//! (`execute`) lands in T-2; `ReadFile`/`FileContent` variants land in T-3.

mod path;

use std::fs;

use path::resolve_in_workspace;

/// Identifies a tool by name.
#[derive(Debug, PartialEq)]
pub enum ToolName {
    ListFiles,
    ReadFile,
}

/// Risk classification for a tool.
#[derive(Debug, PartialEq)]
pub enum ToolRisk {
    ReadOnly,
}

impl ToolRisk {
    /// Returns the canonical snake_case string for this risk level.
    pub fn as_str(&self) -> &'static str {
        match self {
            ToolRisk::ReadOnly => "read_only",
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
}

/// Outputs produced by the tool harness.
#[derive(Debug, PartialEq)]
pub enum ToolOutput {
    FileList { path: String, entries: Vec<String> },
    FileContent { path: String, content: String },
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
