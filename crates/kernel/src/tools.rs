//! Read-only tool harness type definitions for Caravan.
//!
//! This module defines the type system for the tool harness. Execution logic
//! (`execute`) lands in T-2; `ReadFile`/`FileContent` variants land in T-3.

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

/// Per-call execution context passed to every tool invocation.
#[derive(Debug, PartialEq)]
pub struct ToolExecutionContext {
    pub workspace_root: std::path::PathBuf,
}

/// Inputs accepted by the tool harness.
///
/// `ReadFile` variant is added in T-3 alongside the `read_file` implementation.
#[derive(Debug, PartialEq)]
pub enum ToolRequest {
    ListFiles { path: String },
}

/// Outputs produced by the tool harness.
///
/// `FileContent` variant is added in T-3 alongside the `read_file` implementation.
#[derive(Debug, PartialEq)]
pub enum ToolOutput {
    FileList { path: String, entries: Vec<String> },
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_registry_new_readonly_constructs() {
        let registry = ToolRegistry::new_readonly();
        assert_eq!(registry, ToolRegistry);
    }

    #[test]
    fn max_read_file_bytes_is_64_kib() {
        assert_eq!(MAX_READ_FILE_BYTES, 64 * 1024);
    }
}
