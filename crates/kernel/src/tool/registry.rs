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
            // Stream the file in buffered chunks. Lines before `offset` are only
            // scanned for newlines (never allocated), and the emitted output is
            // accumulated directly into a byte buffer that never grows past
            // `MAX_READ_RANGE_OUTPUT_BYTES`. This keeps memory and input
            // processing bounded even for a single pathologically long line
            // (e.g. a multi-megabyte minified file), which a line-at-a-time
            // reader would otherwise load whole into memory.
            use std::io::{BufRead, BufReader};

            let start = offset.unwrap_or(1).max(1);
            let count = limit
                .unwrap_or(DEFAULT_READ_RANGE_LIMIT_LINES)
                .min(MAX_READ_RANGE_LIMIT_LINES);

            // A zero-line range is a valid no-op. Return before opening the file
            // so an invalid/oversized line inside the requested window can never
            // surface as an error for a request that asked for no lines.
            if count == 0 {
                return Ok(ToolOutput::FileContent {
                    path: requested.to_string(),
                    content: String::new(),
                    start_line: Some(start),
                    line_count: Some(0),
                    truncated: false,
                });
            }

            let file = std::fs::File::open(&canonical).map_err(|e| ToolError::Io {
                message: e.to_string(),
            })?;
            let mut reader = BufReader::new(file);

            // `out` holds the eventual `lines.join("\n")` bytes and is hard-capped
            // at MAX_READ_RANGE_OUTPUT_BYTES. `line_no` is the 1-based line we are
            // currently scanning; `completed` counts window lines terminated by a
            // newline. `need_separator` defers the inter-line `\n` so empty lines
            // still contribute their separator (matching `lines().join("\n")`).
            // `pending_cr` defers a `\r` until the next byte is known: a `\r`
            // immediately before `\n` is the "\r\n" terminator and is dropped
            // (matching `BufRead::lines()`); any other deferred `\r` is flushed.
            let mut out: Vec<u8> = Vec::new();
            let mut line_no: usize = 1;
            let mut completed: usize = 0;
            let mut truncated = false;
            let mut hit_cap = false;
            let mut need_separator = false;
            let mut cur_has_content = false;
            let mut pending_cr = false;
            // True only when the byte cap cut a multi-byte UTF-8 char mid-sequence
            // (the over-cap byte is a continuation byte). Distinguishes a benign
            // cap split of a valid char from genuinely invalid bytes, so the
            // latter still surface as NonUtf8 rather than being silently dropped.
            let mut cap_split_content = false;

            'scan: loop {
                let chunk = reader.fill_buf().map_err(|e| ToolError::Io {
                    message: e.to_string(),
                })?;
                if chunk.is_empty() {
                    break; // EOF
                }
                let mut consumed = 0usize;
                for &b in chunk.iter() {
                    consumed += 1;

                    if line_no < start {
                        // Skipping a line before the window: only newlines matter.
                        if b == b'\n' {
                            line_no += 1;
                        }
                        continue;
                    }

                    if b == b'\n' {
                        // "\r\n": a deferred '\r' is this line's terminator -> drop.
                        pending_cr = false;
                        // Terminate the current window line. Emit the owed
                        // separator even for an empty line.
                        if need_separator {
                            if out.len() + 1 > MAX_READ_RANGE_OUTPUT_BYTES {
                                truncated = true;
                                hit_cap = true;
                                reader.consume(consumed);
                                break 'scan;
                            }
                            out.push(b'\n');
                            need_separator = false;
                        }
                        completed += 1;
                        cur_has_content = false;
                        line_no += 1;
                        if completed == count {
                            reader.consume(consumed);
                            break 'scan;
                        }
                        need_separator = line_no > start;
                        continue;
                    }

                    // Content byte of a window line. A deferred '\r' followed by a
                    // non-newline is a real content byte -> flush it first.
                    if pending_cr {
                        if need_separator {
                            if out.len() + 1 > MAX_READ_RANGE_OUTPUT_BYTES {
                                truncated = true;
                                hit_cap = true;
                                reader.consume(consumed);
                                break 'scan;
                            }
                            out.push(b'\n');
                            need_separator = false;
                        }
                        if out.len() + 1 > MAX_READ_RANGE_OUTPUT_BYTES {
                            truncated = true;
                            hit_cap = true;
                            reader.consume(consumed);
                            break 'scan;
                        }
                        out.push(b'\r');
                        pending_cr = false;
                    }
                    if b == b'\r' {
                        // Defer until the next byte decides "\r\n" (drop) vs. content.
                        pending_cr = true;
                        cur_has_content = true;
                        continue;
                    }
                    if need_separator {
                        if out.len() + 1 > MAX_READ_RANGE_OUTPUT_BYTES {
                            truncated = true;
                            hit_cap = true;
                            reader.consume(consumed);
                            break 'scan;
                        }
                        out.push(b'\n');
                        need_separator = false;
                    }
                    if out.len() + 1 > MAX_READ_RANGE_OUTPUT_BYTES {
                        truncated = true;
                        hit_cap = true;
                        // The cap split a valid multi-byte char only if this
                        // over-cap byte is a UTF-8 continuation byte (0b10xxxxxx).
                        cap_split_content = (b & 0b1100_0000) == 0b1000_0000;
                        reader.consume(consumed);
                        break 'scan;
                    }
                    out.push(b);
                    cur_has_content = true;
                }
                reader.consume(consumed);
            }

            // Flush a bare trailing '\r' at natural EOF (no following '\n'):
            // `BufRead::lines()` keeps a final lone CR. Skip when the byte cap was
            // already hit (content is truncated) or a count-break cleared it.
            if pending_cr && !hit_cap {
                if need_separator && out.len() + 1 <= MAX_READ_RANGE_OUTPUT_BYTES {
                    out.push(b'\n');
                    need_separator = false;
                }
                if out.len() + 1 <= MAX_READ_RANGE_OUTPUT_BYTES {
                    out.push(b'\r');
                } else {
                    truncated = true;
                    hit_cap = true;
                }
            }

            // EOF (or cap) reached before the requested start line ever produced
            // any content => empty range, reported at the requested offset.
            let seen_start = completed > 0 || cur_has_content || hit_cap;
            if !seen_start {
                return Ok(ToolOutput::FileContent {
                    path: requested.to_string(),
                    content: String::new(),
                    start_line: Some(start),
                    line_count: Some(0),
                    truncated: false,
                });
            }

            // A final line without a trailing newline, or a cap-truncated partial
            // line, counts as one returned line.
            let line_count = if hit_cap || cur_has_content {
                completed + 1
            } else {
                completed
            };

            let content = match String::from_utf8(out) {
                Ok(s) => s,
                Err(e) => {
                    let utf8_err = e.utf8_error();
                    // Keep the valid prefix ONLY when the byte cap split a valid
                    // multi-byte char mid-sequence (`cap_split_content`) and the
                    // sole decode defect is that incomplete trailing char
                    // (`error_len()` is `None`). Any genuinely invalid byte — or an
                    // incomplete tail that was NOT produced by a content cap split
                    // (e.g. an invalid lead byte already terminated by a newline) —
                    // must surface as NonUtf8 instead of being silently dropped.
                    if cap_split_content && utf8_err.error_len().is_none() {
                        let valid = utf8_err.valid_up_to();
                        let mut bytes = e.into_bytes();
                        bytes.truncate(valid);
                        String::from_utf8(bytes).unwrap_or_default()
                    } else {
                        return Err(ToolError::NonUtf8 {
                            path: requested.to_string(),
                        });
                    }
                }
            };

            Ok(ToolOutput::FileContent {
                path: requested.to_string(),
                content,
                start_line: Some(start),
                line_count: Some(line_count),
                truncated,
            })
        }
    }
}

#[cfg(test)]
mod tests;
