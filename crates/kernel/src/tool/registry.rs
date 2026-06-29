//! Read-only tool harness type definitions for Caravan.
//!
//! This module defines the type system for the tool harness. Execution logic
//! (`execute`) lands in T-2; `ReadFile`/`FileContent` variants land in T-3.

mod glob;
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
    ListFiles {
        path: String,
    },
    ReadFile {
        path: String,
        offset: Option<usize>,
        limit: Option<usize>,
    },
    PlanWrite {
        path: String,
    },
    PreviewWrite {
        path: String,
        content: String,
    },
    SearchText {
        query: String,
    },
    GlobFiles {
        pattern: String,
    },
}

/// Outputs produced by the tool harness.
#[derive(Debug, PartialEq)]
pub enum ToolOutput {
    FileList {
        path: String,
        entries: Vec<String>,
    },
    FileContent {
        path: String,
        content: String,
        /// First 1-based line number returned; `None` for full reads.
        start_line: Option<usize>,
        /// Number of lines actually returned; `None` for full reads.
        line_count: Option<usize>,
        /// `true` when the output byte cap cut content short.
        truncated: bool,
    },
    WritePreview {
        path: String,
        preview: WritePreview,
    },
    SearchResults {
        query: String,
        matches: Vec<SearchMatch>,
        truncated: bool,
    },
    FileMatches {
        pattern: String,
        paths: Vec<String>,
        truncated: bool,
    },
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
    InvalidPattern { pattern: String },
}

/// Maximum bytes allowed when reading a file (full read).
pub const MAX_READ_FILE_BYTES: u64 = 64 * 1024;

/// Default number of lines returned by a range read when no `limit` is supplied.
pub const DEFAULT_READ_RANGE_LIMIT_LINES: usize = 120;

/// Hard cap on the number of lines a range read may request.
pub const MAX_READ_RANGE_LIMIT_LINES: usize = 500;

/// Maximum bytes the content of a range read may occupy before truncation.
pub const MAX_READ_RANGE_OUTPUT_BYTES: usize = 64 * 1024;

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
            ToolRequest::ReadFile {
                path,
                offset,
                limit,
            } => self.read_file(context, &path, offset, limit),
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
            ToolRequest::GlobFiles { pattern } => {
                let outcome = glob::glob_workspace(&context.workspace_root, &pattern)?;
                Ok(ToolOutput::FileMatches {
                    pattern,
                    paths: outcome.paths,
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

    /// Reads a file's UTF-8 contents.
    ///
    /// When both `offset` and `limit` are `None` (full read), the size cap is
    /// checked from file metadata **before** reading any bytes into memory, and
    /// the full [`MAX_READ_FILE_BYTES`] limit applies.
    ///
    /// When `offset` or `limit` is `Some` (range read), the file is read
    /// line-by-line via [`BufReader`]; the byte cap is
    /// [`MAX_READ_RANGE_OUTPUT_BYTES`]. `offset` is 1-based; `limit` defaults to
    /// [`DEFAULT_READ_RANGE_LIMIT_LINES`] and is clamped to
    /// [`MAX_READ_RANGE_LIMIT_LINES`].
    fn read_file(
        &self,
        context: &ToolExecutionContext,
        requested: &str,
        offset: Option<usize>,
        limit: Option<usize>,
    ) -> Result<ToolOutput, ToolError> {
        let canonical = resolve_in_workspace(context, requested)?;

        // Use Path::is_file() (not fs::metadata) for the not-a-file guard so
        // the only fs::metadata call in this module is the size check below.
        if !canonical.is_file() {
            return Err(ToolError::NotAFile {
                path: requested.to_string(),
            });
        }

        if offset.is_none() && limit.is_none() {
            // ── Full read path (unchanged) ──────────────────────────────────
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
                start_line: None,
                line_count: None,
                truncated: false,
            })
        } else {
            // ── Range read path ─────────────────────────────────────────────
            use std::io::{BufRead, BufReader};

            let start = offset.unwrap_or(1).max(1);
            let count = limit
                .unwrap_or(DEFAULT_READ_RANGE_LIMIT_LINES)
                .min(MAX_READ_RANGE_LIMIT_LINES);

            let file = std::fs::File::open(&canonical).map_err(|e| ToolError::Io {
                message: e.to_string(),
            })?;
            let reader = BufReader::new(file);

            let mut lines_collected: Vec<String> = Vec::new();
            let mut total_bytes: usize = 0;
            let mut truncated = false;
            let mut line_index: usize = 0;
            let mut seen_start = false;

            for line_result in reader.lines() {
                let line = line_result.map_err(|e| {
                    if e.kind() == std::io::ErrorKind::InvalidData {
                        ToolError::NonUtf8 {
                            path: requested.to_string(),
                        }
                    } else {
                        ToolError::Io {
                            message: e.to_string(),
                        }
                    }
                })?;

                line_index += 1;

                if line_index < start {
                    continue;
                }

                seen_start = true;

                if lines_collected.len() >= count {
                    break;
                }

                // Byte cap: count line content + newline separator.
                let line_bytes = line.len() + 1;
                if total_bytes + line_bytes > MAX_READ_RANGE_OUTPUT_BYTES {
                    let remaining = MAX_READ_RANGE_OUTPUT_BYTES.saturating_sub(total_bytes);
                    let mut cut = remaining.min(line.len());
                    while cut > 0 && !line.is_char_boundary(cut) {
                        cut -= 1;
                    }
                    if cut > 0 {
                        lines_collected.push(line[..cut].to_string());
                    }
                    truncated = true;
                    break;
                }

                total_bytes += line_bytes;
                lines_collected.push(line);
            }

            // EOF was reached before the requested start line.
            if !seen_start {
                return Ok(ToolOutput::FileContent {
                    path: requested.to_string(),
                    content: String::new(),
                    start_line: Some(start),
                    line_count: Some(0),
                    truncated: false,
                });
            }

            let actual_count = lines_collected.len();
            let content = lines_collected.join("\n");

            Ok(ToolOutput::FileContent {
                path: requested.to_string(),
                content,
                start_line: Some(start),
                line_count: Some(actual_count),
                truncated,
            })
        }
    }
}

#[cfg(test)]
mod tests;
