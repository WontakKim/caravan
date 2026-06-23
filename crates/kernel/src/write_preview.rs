//! Read-only dry-run preview layer for proposed file writes.
//!
//! This module validates a [`WriteIntent`] against the current workspace and
//! produces a bounded line-diff summary. It performs **NO write**, creates
//! **NO temp file**, and appends **NOTHING to the EventLog**.
//!
//! # Content-exposure policy
//!
//! [`WriteDiffSummary::preview`] is a bounded rendering of diff lines (at most
//! [`WRITE_DIFF_PREVIEW_LINES`] lines) and MAY legitimately contain file content
//! lines — that is what a diff preview is. Because it may contain content or
//! secrets, it is NEVER auto-appended to the EventLog. [`WritePreview::detail()`]
//! is a content-free key=value summary safe for logging and future
//! `ApprovalRequest` summaries; it MUST contain no preview lines and no
//! proposed/existing file content.

use std::fs;
use std::path::{Component, Path};

use crate::ToolExecutionContext;
use crate::tool::registry::MAX_READ_FILE_BYTES;
use crate::write_intent::{WRITE_INTENT_PREVIEW_BYTES, WriteIntent, WriteIntentSummary};

/// Maximum number of lines emitted in a [`WriteDiffSummary::preview`].
pub const WRITE_DIFF_PREVIEW_LINES: usize = 40;

/// Maximum number of bytes retained per emitted preview line. A single very long
/// line is truncated (UTF-8-safe) so the bounded preview cannot leak an arbitrarily
/// large amount of content even within the [`WRITE_DIFF_PREVIEW_LINES`] line cap.
const MAX_PREVIEW_LINE_BYTES: usize = 256;

/// Renders one diff line for display: strips the trailing line terminator(s) so
/// the preview entry is a single physical line, then bounds it to
/// [`MAX_PREVIEW_LINE_BYTES`] on a UTF-8 character boundary.
fn render_preview_line(prefix: char, raw_line: &str) -> String {
    let trimmed = raw_line.strip_suffix('\n').unwrap_or(raw_line);
    let trimmed = trimmed.strip_suffix('\r').unwrap_or(trimmed);
    if trimmed.len() <= MAX_PREVIEW_LINE_BYTES {
        return format!("{prefix} {trimmed}");
    }
    let mut cut = MAX_PREVIEW_LINE_BYTES;
    while cut > 0 && !trimmed.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{prefix} {}…", &trimmed[..cut])
}

/// Classifies how a [`WriteIntent`] would affect the target path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WritePreviewKind {
    /// The target does not exist; the write would create a new file.
    NewFile,
    /// The target exists and its content would change.
    ReplaceExisting,
    /// The target exists and its content is already identical; no change needed.
    NoChange,
}

impl WritePreviewKind {
    /// Returns the lowercase snake_case string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            WritePreviewKind::NewFile => "new_file",
            WritePreviewKind::ReplaceExisting => "replace_existing",
            WritePreviewKind::NoChange => "no_change",
        }
    }
}

/// A bounded line-diff summary between an existing file and proposed content.
///
/// `preview` MAY contain file content lines (bounded to [`WRITE_DIFF_PREVIEW_LINES`]).
/// It is never auto-appended to the EventLog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteDiffSummary {
    /// Byte length of the existing file, or `None` for a new file.
    pub old_bytes: Option<usize>,
    /// Byte length of the proposed content.
    pub new_bytes: usize,
    /// Line count of the existing file, or `None` for a new file.
    pub old_lines: Option<usize>,
    /// Line count of the proposed content.
    pub new_lines: usize,
    /// Total number of lines that would be added over the full comparison.
    pub additions: usize,
    /// Total number of lines that would be removed over the full comparison.
    pub removals: usize,
    /// Bounded diff lines (at most [`WRITE_DIFF_PREVIEW_LINES`] entries).
    pub preview: Vec<String>,
    /// `true` if the diff exceeded [`WRITE_DIFF_PREVIEW_LINES`] and was truncated.
    pub truncated: bool,
}

/// A read-only preview of what a [`WriteIntent`] would do to the workspace.
///
/// No file is written, no temp file is created, and nothing is appended to the
/// EventLog. Use [`WritePreview::detail()`] for a content-free log-safe summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WritePreview {
    /// The path as supplied in the intent (not canonicalized).
    pub path: String,
    /// Classification of the write operation.
    pub kind: WritePreviewKind,
    /// Metadata snapshot of the intent (bounded content preview for display).
    pub intent_summary: WriteIntentSummary,
    /// Bounded line-diff summary.
    pub diff: WriteDiffSummary,
}

impl WritePreview {
    /// Returns a content-free key=value summary suitable for logging.
    ///
    /// MUST NOT contain any preview lines or proposed/existing file content.
    pub fn detail(&self) -> String {
        let old_bytes = match self.diff.old_bytes {
            Some(n) => n.to_string(),
            None => "none".to_string(),
        };
        let old_lines = match self.diff.old_lines {
            Some(n) => n.to_string(),
            None => "none".to_string(),
        };
        format!(
            "path={:?} kind={} mode={} source={} old_bytes={} new_bytes={} old_lines={} new_lines={} additions={} removals={} truncated={}",
            self.path,
            self.kind.as_str(),
            self.intent_summary.mode.as_str(),
            self.intent_summary.source.as_str(),
            old_bytes,
            self.diff.new_bytes,
            old_lines,
            self.diff.new_lines,
            self.diff.additions,
            self.diff.removals,
            self.diff.truncated,
        )
    }
}

/// Errors that can occur during write preview generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WritePreviewError {
    /// The path escapes the workspace boundary (absolute path, `..`, or symlink escape).
    WorkspaceViolation { path: String },
    /// The parent directory of the target does not exist or is not a directory.
    ParentNotFound { path: String },
    /// The target path exists but is not a regular file.
    NotAFile { path: String },
    /// The existing file is not valid UTF-8.
    NonUtf8 { path: String },
    /// The existing file exceeds the read size limit.
    TooLarge { path: String, max_bytes: u64 },
    /// An unexpected I/O error occurred.
    Io { message: String },
}

impl std::fmt::Display for WritePreviewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WritePreviewError::WorkspaceViolation { path } => {
                write!(f, "path escapes workspace boundary: {path:?}")
            }
            WritePreviewError::ParentNotFound { path } => {
                write!(f, "parent directory not found for path: {path:?}")
            }
            WritePreviewError::NotAFile { path } => {
                write!(f, "target exists but is not a regular file: {path:?}")
            }
            WritePreviewError::NonUtf8 { path } => {
                write!(f, "existing file is not valid UTF-8: {path:?}")
            }
            WritePreviewError::TooLarge { path, max_bytes } => {
                write!(
                    f,
                    "existing file exceeds the {max_bytes}-byte read limit: {path:?}"
                )
            }
            WritePreviewError::Io { message } => {
                write!(f, "I/O error: {message}")
            }
        }
    }
}

impl std::error::Error for WritePreviewError {}

/// Internal result of path-safety validation and leaf classification.
enum Classification {
    NewFile,
    ExistingFile { canonical: std::path::PathBuf },
}

/// Validates `intent.path` against the workspace and classifies the leaf.
///
/// # Security
///
/// - Lexically rejects absolute paths and any `..` / `RootDir` / `Prefix`
///   component before any filesystem operation (existence-independent).
/// - Canonicalizes the workspace root and verifies the candidate path starts
///   with the canonical root after resolution (defense-in-depth against symlink
///   escapes).
/// - Uses `symlink_metadata` (NOT `Path::exists`) to detect broken symlinks and
///   fail closed: a broken symlink is a `WorkspaceViolation`, never `NewFile`.
fn validate_and_classify(
    context: &ToolExecutionContext,
    intent: &WriteIntent,
) -> Result<Classification, WritePreviewError> {
    // Lexical validation: reject absolute paths and any `..` before any
    // filesystem operation or join (existence-independent).
    for component in Path::new(&intent.path).components() {
        match component {
            Component::RootDir | Component::Prefix(_) | Component::ParentDir => {
                return Err(WritePreviewError::WorkspaceViolation {
                    path: intent.path.clone(),
                });
            }
            Component::Normal(_) | Component::CurDir => {}
        }
    }

    // Canonicalize the workspace root (handles platform path aliasing such as
    // macOS /var -> /private/var).
    let canonical_root =
        fs::canonicalize(&context.workspace_root).map_err(|e| WritePreviewError::Io {
            message: e.to_string(),
        })?;

    // Form the candidate by joining the canonical root with the relative path.
    let candidate = canonical_root.join(&intent.path);

    // Classify the leaf with symlink_metadata (does NOT follow the final symlink).
    // This ensures broken symlinks are detected and fail closed rather than
    // being treated as missing targets.
    match fs::symlink_metadata(&candidate) {
        Ok(_) => {
            // Entry exists (may be a regular file, directory, symlink, etc.).
            // Canonicalize the candidate, following all symlinks.
            let canonical_target = fs::canonicalize(&candidate).map_err(|_| {
                // Broken symlink or other resolution failure — fail closed.
                WritePreviewError::WorkspaceViolation {
                    path: intent.path.clone(),
                }
            })?;

            // Defense-in-depth: the canonicalized target must remain inside the
            // workspace (catches a symlink whose target escapes the workspace).
            if !canonical_target.starts_with(&canonical_root) {
                return Err(WritePreviewError::WorkspaceViolation {
                    path: intent.path.clone(),
                });
            }

            // The leaf must be a regular file.
            //
            // NOTE: There is an accepted TOCTOU window between this metadata check
            // and the subsequent read in `preview_write_intent` (consistent with the
            // pattern in `tool/registry.rs`). Reading from the canonicalized path
            // reduces a simple symlink-swap race since the path is already resolved.
            let meta = fs::metadata(&canonical_target).map_err(|e| WritePreviewError::Io {
                message: e.to_string(),
            })?;
            if !meta.is_file() {
                return Err(WritePreviewError::NotAFile {
                    path: intent.path.clone(),
                });
            }

            Ok(Classification::ExistingFile {
                canonical: canonical_target,
            })
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Target does not exist. Validate the parent directory.
            let parent = candidate
                .parent()
                .ok_or_else(|| WritePreviewError::ParentNotFound {
                    path: intent.path.clone(),
                })?;

            let canonical_parent =
                fs::canonicalize(parent).map_err(|_| WritePreviewError::ParentNotFound {
                    path: intent.path.clone(),
                })?;

            // The canonical parent must be inside the workspace.
            if !canonical_parent.starts_with(&canonical_root) {
                return Err(WritePreviewError::WorkspaceViolation {
                    path: intent.path.clone(),
                });
            }

            // The parent must actually be a directory (not a file or other node).
            if !canonical_parent.is_dir() {
                return Err(WritePreviewError::ParentNotFound {
                    path: intent.path.clone(),
                });
            }

            Ok(Classification::NewFile)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotADirectory => {
            // A component of the path is a regular file rather than a directory,
            // so the effective parent does not exist as a directory.
            Err(WritePreviewError::ParentNotFound {
                path: intent.path.clone(),
            })
        }
        Err(e) => Err(WritePreviewError::Io {
            message: e.to_string(),
        }),
    }
}

/// Produces a read-only preview of what `intent` would do to the workspace.
///
/// No file is written, no temp file is created, and nothing is appended to the
/// EventLog. The returned [`WritePreview`] describes exactly what *would* happen
/// if the intent were executed.
pub fn preview_write_intent(
    context: &ToolExecutionContext,
    intent: &WriteIntent,
) -> Result<WritePreview, WritePreviewError> {
    let classification = validate_and_classify(context, intent)?;

    let (kind, old_content): (WritePreviewKind, Option<String>) = match classification {
        Classification::NewFile => (WritePreviewKind::NewFile, None),
        Classification::ExistingFile { canonical } => {
            // Size cap: check metadata BEFORE reading any bytes into memory.
            let len = fs::metadata(&canonical)
                .map_err(|e| WritePreviewError::Io {
                    message: e.to_string(),
                })?
                .len();

            if len > MAX_READ_FILE_BYTES {
                return Err(WritePreviewError::TooLarge {
                    path: intent.path.clone(),
                    max_bytes: MAX_READ_FILE_BYTES,
                });
            }

            let bytes = fs::read(&canonical).map_err(|e| WritePreviewError::Io {
                message: e.to_string(),
            })?;

            // Strict UTF-8 decode — lossy conversion is never used.
            let content = String::from_utf8(bytes).map_err(|_| WritePreviewError::NonUtf8 {
                path: intent.path.clone(),
            })?;

            let kind = if content == intent.content {
                WritePreviewKind::NoChange
            } else {
                WritePreviewKind::ReplaceExisting
            };

            (kind, Some(content))
        }
    };

    let diff = build_diff(kind, old_content.as_deref(), &intent.content);
    let intent_summary = intent.summary(WRITE_INTENT_PREVIEW_BYTES);

    Ok(WritePreview {
        path: intent.path.clone(),
        kind,
        intent_summary,
        diff,
    })
}

/// Builds a bounded line-diff summary from old and new content.
///
/// `old_content` is `None` for new files and `Some(existing)` for replacements.
/// `additions` and `removals` are the TOTAL counts over the full comparison;
/// only `preview` is truncated at [`WRITE_DIFF_PREVIEW_LINES`].
///
/// Lines are tokenized with `split_inclusive('\n')` so that line terminators are
/// preserved: a byte-level change that `str::lines()` would hide — such as a
/// trailing-newline-only edit (`"a"` vs `"a\n"`) or a line-ending change
/// (`"a\r\n"` vs `"a\n"`) — still produces a visible diff entry. Because
/// `split_inclusive` is lossless, any byte difference yields at least one differing
/// token, so a `ReplaceExisting` preview is never empty.
///
/// This is a deliberately simple positional (index-by-index) comparison, NOT a
/// minimal-edit diff: a line inserted near the top shifts subsequent lines and is
/// reported as several changed lines. Per the design, a bounded, deterministic,
/// dependency-free preview is preferred over an exact semantic diff.
fn build_diff(
    kind: WritePreviewKind,
    old_content: Option<&str>,
    new_content: &str,
) -> WriteDiffSummary {
    let new_bytes = new_content.len();
    let new_lines = new_content.lines().count();
    let old_bytes = old_content.map(|s| s.len());
    let old_lines = old_content.map(|s| s.lines().count());

    // NoChange uses a fixed sentinel preview; no diff lines are emitted.
    if kind == WritePreviewKind::NoChange {
        return WriteDiffSummary {
            old_bytes,
            new_bytes,
            old_lines,
            new_lines,
            additions: 0,
            removals: 0,
            preview: vec!["No changes.".to_string()],
            truncated: false,
        };
    }

    // Terminator-preserving tokenization so trailing-newline / line-ending-only
    // differences are detected (see the function doc comment).
    let old_vec: Vec<&str> = old_content
        .map(|s| s.split_inclusive('\n').collect())
        .unwrap_or_default();
    let new_vec: Vec<&str> = new_content.split_inclusive('\n').collect();

    let max_idx = old_vec.len().max(new_vec.len());
    let mut additions = 0usize;
    let mut removals = 0usize;
    let mut preview: Vec<String> = Vec::new();
    let mut truncated = false;

    for i in 0..max_idx {
        let old_line = old_vec.get(i).copied();
        let new_line = new_vec.get(i).copied();

        match (old_line, new_line) {
            (Some(o), Some(n)) if o == n => {
                // Lines are equal at this index: no change, nothing to emit.
            }
            (Some(o), Some(n)) => {
                // Both present but differ: one removal + one addition.
                removals += 1;
                additions += 1;
                if preview.len() < WRITE_DIFF_PREVIEW_LINES {
                    preview.push(render_preview_line('-', o));
                } else {
                    truncated = true;
                }
                if preview.len() < WRITE_DIFF_PREVIEW_LINES {
                    preview.push(render_preview_line('+', n));
                } else {
                    truncated = true;
                }
            }
            (None, Some(n)) => {
                // New content has more lines: pure addition.
                additions += 1;
                if preview.len() < WRITE_DIFF_PREVIEW_LINES {
                    preview.push(render_preview_line('+', n));
                } else {
                    truncated = true;
                }
            }
            (Some(o), None) => {
                // Old content has more lines: pure removal.
                removals += 1;
                if preview.len() < WRITE_DIFF_PREVIEW_LINES {
                    preview.push(render_preview_line('-', o));
                } else {
                    truncated = true;
                }
            }
            (None, None) => unreachable!("index is bounded by max of both lengths"),
        }
    }

    WriteDiffSummary {
        old_bytes,
        new_bytes,
        old_lines,
        new_lines,
        additions,
        removals,
        preview,
        truncated,
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::write_intent::{WriteIntentMode, WriteIntentSource, new_text};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    /// RAII guard that creates a unique temp directory and removes it on drop,
    /// even when a test panics. Uses process ID + atomic counter to stay
    /// parallel-safe.
    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new() -> Self {
            let count = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
            let name = format!(
                "caravan_write_preview_test_{}_{}",
                std::process::id(),
                count
            );
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

    fn make_intent(path: &str, content: &str) -> WriteIntent {
        new_text(
            path,
            content,
            WriteIntentMode::CreateOrReplace,
            WriteIntentSource::Operator,
        )
        .expect("valid intent")
    }

    // (a) New-file preview: NewFile kind, additions == new line count,
    //     "+" prefix on all lines, old_bytes/old_lines are None.
    #[test]
    fn new_file_preview_kind_and_additions() {
        let dir = TempDir::new();
        let ctx = make_context(dir.path());
        let intent = make_intent("new.txt", "line one\nline two\nline three\n");

        let preview = preview_write_intent(&ctx, &intent).expect("should succeed");

        assert_eq!(preview.kind, WritePreviewKind::NewFile);
        assert!(preview.diff.old_bytes.is_none());
        assert!(preview.diff.old_lines.is_none());
        assert_eq!(preview.diff.additions, 3);
        assert_eq!(preview.diff.removals, 0);
        assert!(preview.diff.preview.iter().all(|l| l.starts_with("+ ")));
    }

    // (b) Missing target + empty content -> NewFile with new_bytes=0, new_lines=0.
    #[test]
    fn new_file_empty_content() {
        let dir = TempDir::new();
        let ctx = make_context(dir.path());
        let intent = make_intent("empty_new.txt", "");

        let preview = preview_write_intent(&ctx, &intent).expect("should succeed");

        assert_eq!(preview.kind, WritePreviewKind::NewFile);
        assert_eq!(preview.diff.new_bytes, 0);
        assert_eq!(preview.diff.new_lines, 0);
        assert!(preview.diff.old_bytes.is_none());
        assert!(preview.diff.old_lines.is_none());
    }

    // (c) Replace with equal line count -> ReplaceExisting, additions == removals
    //     == number of changed lines.
    #[test]
    fn replace_equal_line_count() {
        let dir = TempDir::new();
        std::fs::write(dir.path().join("target.txt"), "old line 1\nold line 2\n")
            .expect("write file");
        let ctx = make_context(dir.path());
        let intent = make_intent("target.txt", "new line 1\nnew line 2\n");

        let preview = preview_write_intent(&ctx, &intent).expect("should succeed");

        assert_eq!(preview.kind, WritePreviewKind::ReplaceExisting);
        assert_eq!(preview.diff.additions, 2);
        assert_eq!(preview.diff.removals, 2);
    }

    // (d) Replace where new has more lines than old -> tail additions counted
    //     and present in preview.
    #[test]
    fn replace_new_has_more_lines() {
        let dir = TempDir::new();
        std::fs::write(dir.path().join("target.txt"), "only line\n").expect("write file");
        let ctx = make_context(dir.path());
        let intent = make_intent("target.txt", "only line\nextra line 1\nextra line 2\n");

        let preview = preview_write_intent(&ctx, &intent).expect("should succeed");

        assert_eq!(preview.kind, WritePreviewKind::ReplaceExisting);
        assert_eq!(preview.diff.additions, 2);
        assert_eq!(preview.diff.removals, 0);
        let preview_text = preview.diff.preview.join("\n");
        assert!(preview_text.contains("+ extra line 1"));
        assert!(preview_text.contains("+ extra line 2"));
    }

    // (e) Replace where old has more lines than new -> tail removals counted.
    #[test]
    fn replace_old_has_more_lines() {
        let dir = TempDir::new();
        std::fs::write(
            dir.path().join("target.txt"),
            "line a\nline b\nline c\nline d\n",
        )
        .expect("write file");
        let ctx = make_context(dir.path());
        let intent = make_intent("target.txt", "line a\n");

        let preview = preview_write_intent(&ctx, &intent).expect("should succeed");

        assert_eq!(preview.kind, WritePreviewKind::ReplaceExisting);
        assert_eq!(preview.diff.removals, 3);
        assert_eq!(preview.diff.additions, 0);
    }

    // (f) Existing non-empty file + empty content -> ReplaceExisting with
    //     removals > 0 and new_bytes == 0.
    #[test]
    fn replace_existing_with_empty_content() {
        let dir = TempDir::new();
        std::fs::write(dir.path().join("target.txt"), "some content\n").expect("write file");
        let ctx = make_context(dir.path());
        let intent = make_intent("target.txt", "");

        let preview = preview_write_intent(&ctx, &intent).expect("should succeed");

        assert_eq!(preview.kind, WritePreviewKind::ReplaceExisting);
        assert!(preview.diff.removals > 0);
        assert_eq!(preview.diff.new_bytes, 0);
    }

    // (g) Existing empty file + empty content -> NoChange.
    #[test]
    fn existing_empty_plus_empty_is_no_change() {
        let dir = TempDir::new();
        std::fs::write(dir.path().join("empty.txt"), "").expect("write file");
        let ctx = make_context(dir.path());
        let intent = make_intent("empty.txt", "");

        let preview = preview_write_intent(&ctx, &intent).expect("should succeed");

        assert_eq!(preview.kind, WritePreviewKind::NoChange);
    }

    // (h) Identical content -> NoChange with preview == ["No changes."],
    //     additions == 0, removals == 0.
    #[test]
    fn identical_content_is_no_change() {
        let dir = TempDir::new();
        std::fs::write(dir.path().join("same.txt"), "hello world\n").expect("write file");
        let ctx = make_context(dir.path());
        let intent = make_intent("same.txt", "hello world\n");

        let preview = preview_write_intent(&ctx, &intent).expect("should succeed");

        assert_eq!(preview.kind, WritePreviewKind::NoChange);
        assert_eq!(preview.diff.preview, vec!["No changes.".to_string()]);
        assert_eq!(preview.diff.additions, 0);
        assert_eq!(preview.diff.removals, 0);
        assert!(!preview.diff.truncated);
    }

    // (i) Trailing-newline-only difference -> ReplaceExisting with equal
    //     old_lines/new_lines but differing byte counts.
    #[test]
    fn trailing_newline_difference_is_replace_existing() {
        let dir = TempDir::new();
        std::fs::write(dir.path().join("trailing.txt"), "hello\n").expect("write file");
        let ctx = make_context(dir.path());
        let intent = make_intent("trailing.txt", "hello");

        let preview = preview_write_intent(&ctx, &intent).expect("should succeed");

        assert_eq!(preview.kind, WritePreviewKind::ReplaceExisting);
        // Both have 1 logical line via .lines()
        assert_eq!(preview.diff.old_lines, Some(1));
        assert_eq!(preview.diff.new_lines, 1);
        // But byte counts differ
        assert_ne!(preview.diff.old_bytes.unwrap(), preview.diff.new_bytes);
        // A ReplaceExisting must surface the change: the terminator-preserving
        // tokenizer reports a non-empty preview and non-zero change counts even
        // though the logical line counts are equal.
        assert!(
            preview.diff.additions + preview.diff.removals > 0,
            "trailing-newline-only change must be counted"
        );
        assert!(
            !preview.diff.preview.is_empty(),
            "ReplaceExisting preview must not be empty"
        );
    }

    // (i2) Line-ending-only difference (\r\n vs \n) -> ReplaceExisting with a
    //      non-empty preview and non-zero change counts (regression guard: a
    //      str::lines() based comparison would wrongly report no change).
    #[test]
    fn line_ending_only_difference_is_replace_existing() {
        let dir = TempDir::new();
        std::fs::write(dir.path().join("crlf.txt"), "alpha\r\nbeta\r\n").expect("write file");
        let ctx = make_context(dir.path());
        let intent = make_intent("crlf.txt", "alpha\nbeta\n");

        let preview = preview_write_intent(&ctx, &intent).expect("should succeed");

        assert_eq!(preview.kind, WritePreviewKind::ReplaceExisting);
        assert!(
            preview.diff.additions + preview.diff.removals > 0,
            "line-ending-only change must be counted"
        );
        assert!(
            !preview.diff.preview.is_empty(),
            "ReplaceExisting preview must not be empty"
        );
    }

    // (i3) A single very long preview line is byte-bounded (UTF-8-safe) so the
    //      bounded preview cannot leak arbitrarily large content even within the
    //      line-count cap.
    #[test]
    fn long_preview_line_is_byte_bounded() {
        let dir = TempDir::new();
        let ctx = make_context(dir.path());
        let huge_line = "x".repeat(10_000);
        let intent = make_intent("huge_line.txt", &huge_line);

        let preview = preview_write_intent(&ctx, &intent).expect("should succeed");

        assert_eq!(preview.kind, WritePreviewKind::NewFile);
        for line in &preview.diff.preview {
            assert!(
                line.len() <= MAX_PREVIEW_LINE_BYTES + 8,
                "preview line must be byte-bounded, got {} bytes",
                line.len()
            );
        }
    }

    // (j) Directory target -> NotAFile.
    #[test]
    fn directory_target_returns_not_a_file() {
        let dir = TempDir::new();
        std::fs::create_dir_all(dir.path().join("subdir")).expect("create subdir");
        let ctx = make_context(dir.path());
        let intent = make_intent("subdir", "content");

        let result = preview_write_intent(&ctx, &intent);

        assert!(matches!(result, Err(WritePreviewError::NotAFile { .. })));
    }

    // (k) [unix] Non-regular existing target (socket) -> NotAFile.
    #[cfg(unix)]
    #[test]
    fn unix_socket_target_returns_not_a_file() {
        let dir = TempDir::new();
        let socket_path = dir.path().join("test.sock");
        // Create a Unix domain socket (non-regular file, no extra crates needed).
        let _listener = std::os::unix::net::UnixListener::bind(&socket_path).expect("bind socket");

        let ctx = make_context(dir.path());
        let intent = make_intent("test.sock", "content");

        let result = preview_write_intent(&ctx, &intent);

        assert!(matches!(result, Err(WritePreviewError::NotAFile { .. })));
    }

    // (l) Non-UTF-8 existing file -> NonUtf8.
    #[test]
    fn non_utf8_file_returns_non_utf8() {
        let dir = TempDir::new();
        let invalid_utf8: &[u8] = &[0xFF, 0xFE, 0x00];
        std::fs::write(dir.path().join("bad.bin"), invalid_utf8).expect("write file");
        let ctx = make_context(dir.path());
        let intent = make_intent("bad.bin", "replacement");

        let result = preview_write_intent(&ctx, &intent);

        assert!(matches!(result, Err(WritePreviewError::NonUtf8 { .. })));
    }

    // (m) Existing file of exactly MAX_READ_FILE_BYTES is allowed.
    #[test]
    fn file_exactly_max_size_is_allowed() {
        let dir = TempDir::new();
        let data = vec![b'a'; MAX_READ_FILE_BYTES as usize];
        std::fs::write(dir.path().join("exact.bin"), &data).expect("write file");
        let ctx = make_context(dir.path());
        let intent = make_intent("exact.bin", "replacement");

        let result = preview_write_intent(&ctx, &intent);

        // Should not return TooLarge — may fail NonUtf8 since the bytes are 'a'
        // (all ASCII), so it should actually succeed.
        assert!(
            result.is_ok(),
            "exactly MAX_READ_FILE_BYTES should be allowed"
        );
    }

    // (n) Existing file of MAX_READ_FILE_BYTES + 1 -> TooLarge.
    #[test]
    fn file_over_max_size_returns_too_large() {
        let dir = TempDir::new();
        let data = vec![b'a'; MAX_READ_FILE_BYTES as usize + 1];
        std::fs::write(dir.path().join("oversized.bin"), &data).expect("write file");
        let ctx = make_context(dir.path());
        let intent = make_intent("oversized.bin", "replacement");

        let result = preview_write_intent(&ctx, &intent);

        assert!(matches!(
            result,
            Err(WritePreviewError::TooLarge {
                max_bytes: MAX_READ_FILE_BYTES,
                ..
            })
        ));
    }

    // (o) Absolute path -> WorkspaceViolation.
    #[test]
    fn absolute_path_returns_workspace_violation() {
        let dir = TempDir::new();
        let ctx = make_context(dir.path());
        let intent = make_intent("/etc/passwd", "content");

        let result = preview_write_intent(&ctx, &intent);

        assert!(matches!(
            result,
            Err(WritePreviewError::WorkspaceViolation { .. })
        ));
    }

    // (p) ../ escape -> WorkspaceViolation.
    #[test]
    fn dotdot_path_returns_workspace_violation() {
        let dir = TempDir::new();
        let ctx = make_context(dir.path());
        let intent = make_intent("../escape.txt", "content");

        let result = preview_write_intent(&ctx, &intent);

        assert!(matches!(
            result,
            Err(WritePreviewError::WorkspaceViolation { .. })
        ));
    }

    // (q) [unix] Existing symlink whose target is outside the workspace ->
    //     WorkspaceViolation.
    #[cfg(unix)]
    #[test]
    fn unix_symlink_escaping_workspace_returns_violation() {
        let dir = TempDir::new();
        let outside = TempDir::new();
        // Create a file outside the workspace.
        std::fs::write(outside.path().join("secret.txt"), "secret").expect("write file");
        // Create a symlink inside the workspace pointing to the outside file.
        let link_path = dir.path().join("escape.txt");
        std::os::unix::fs::symlink(outside.path().join("secret.txt"), &link_path)
            .expect("create symlink");

        let ctx = make_context(dir.path());
        let intent = make_intent("escape.txt", "replacement");

        let result = preview_write_intent(&ctx, &intent);

        assert!(matches!(
            result,
            Err(WritePreviewError::WorkspaceViolation { .. })
        ));
    }

    // (r) [unix] Broken symlink leaf -> fails closed (WorkspaceViolation), not NewFile.
    #[cfg(unix)]
    #[test]
    fn unix_broken_symlink_fails_closed() {
        let dir = TempDir::new();
        // Create a symlink pointing to a nonexistent target.
        let link_path = dir.path().join("broken.txt");
        std::os::unix::fs::symlink("/nonexistent/path/nowhere", &link_path)
            .expect("create broken symlink");

        let ctx = make_context(dir.path());
        let intent = make_intent("broken.txt", "content");

        let result = preview_write_intent(&ctx, &intent);

        // Must fail closed as WorkspaceViolation, NOT succeed as NewFile.
        assert!(matches!(
            result,
            Err(WritePreviewError::WorkspaceViolation { .. })
        ));
    }

    // (s) Missing parent directory -> ParentNotFound.
    #[test]
    fn missing_parent_directory_returns_parent_not_found() {
        let dir = TempDir::new();
        let ctx = make_context(dir.path());
        let intent = make_intent("nonexistent_dir/child.txt", "content");

        let result = preview_write_intent(&ctx, &intent);

        assert!(matches!(
            result,
            Err(WritePreviewError::ParentNotFound { .. })
        ));
    }

    // (t) Missing target whose parent path is an existing regular file ->
    //     ParentNotFound (not NewFile).
    #[test]
    fn parent_is_regular_file_returns_parent_not_found() {
        let dir = TempDir::new();
        std::fs::write(dir.path().join("regular.txt"), "data").expect("write file");
        let ctx = make_context(dir.path());
        // "regular.txt/child.txt": parent is a file, not a directory.
        let intent = make_intent("regular.txt/child.txt", "content");

        let result = preview_write_intent(&ctx, &intent);

        assert!(matches!(
            result,
            Err(WritePreviewError::ParentNotFound { .. })
        ));
    }

    // (u) [unix] Missing target whose parent is a symlink to outside the workspace ->
    //     WorkspaceViolation.
    #[cfg(unix)]
    #[test]
    fn unix_parent_symlink_escaping_workspace_returns_violation() {
        let dir = TempDir::new();
        let outside_dir = TempDir::new();
        // Create a symlink inside workspace pointing to an outside directory.
        let link_path = dir.path().join("outside_link");
        std::os::unix::fs::symlink(outside_dir.path(), &link_path).expect("create symlink");

        let ctx = make_context(dir.path());
        // "outside_link/new_file.txt": parent resolves to outside workspace.
        let intent = make_intent("outside_link/new_file.txt", "content");

        let result = preview_write_intent(&ctx, &intent);

        assert!(matches!(
            result,
            Err(WritePreviewError::WorkspaceViolation { .. })
        ));
    }

    // (v) Bounded diff truncation: more than 40 changed lines sets truncated=true
    //     and preview.len() <= WRITE_DIFF_PREVIEW_LINES.
    #[test]
    fn diff_truncation_over_40_lines() {
        let dir = TempDir::new();
        // Create a file with 50 distinct lines.
        let old_content: String = (0..50).map(|i| format!("old line {i}\n")).collect();
        std::fs::write(dir.path().join("big.txt"), &old_content).expect("write file");
        let ctx = make_context(dir.path());
        // Replace with 50 different lines.
        let new_content: String = (0..50).map(|i| format!("new line {i}\n")).collect();
        let intent = make_intent("big.txt", &new_content);

        let preview = preview_write_intent(&ctx, &intent).expect("should succeed");

        assert!(preview.diff.truncated);
        assert!(preview.diff.preview.len() <= WRITE_DIFF_PREVIEW_LINES);
        assert_eq!(preview.diff.additions, 50);
        assert_eq!(preview.diff.removals, 50);
    }

    // (w) Multi-byte UTF-8 content yields valid (non-split) preview lines.
    #[test]
    fn multibyte_utf8_content_valid_preview() {
        let dir = TempDir::new();
        let ctx = make_context(dir.path());
        // '€' is U+20AC, encoded as 3 bytes in UTF-8.
        let content: String = (0..5).map(|i| format!("€ line {i}\n")).collect();
        let intent = make_intent("unicode.txt", &content);

        let preview = preview_write_intent(&ctx, &intent).expect("should succeed");

        for line in &preview.diff.preview {
            assert!(std::str::from_utf8(line.as_bytes()).is_ok());
        }
    }

    // (x) detail() contains none of the file content.
    #[test]
    fn detail_contains_no_file_content() {
        let dir = TempDir::new();
        let sentinel = "UNIQUE_SENTINEL_VALUE_XYZ_9182736";
        std::fs::write(
            dir.path().join("secret.txt"),
            format!("{sentinel}\nmore content\n"),
        )
        .expect("write file");
        let ctx = make_context(dir.path());
        let intent = make_intent("secret.txt", "replacement content\n");

        let preview = preview_write_intent(&ctx, &intent).expect("should succeed");
        let detail = preview.detail();

        assert!(
            !detail.contains(sentinel),
            "detail() must not contain existing file content: {detail}"
        );
    }

    // (y) A one-line new file: the content line appears in diff.preview but NOT
    //     in detail().
    #[test]
    fn new_file_content_in_preview_not_in_detail() {
        let dir = TempDir::new();
        let ctx = make_context(dir.path());
        let unique_content = "UNIQUE_CONTENT_MARKER_ABCDEF_12345";
        let intent = make_intent("oneliner.txt", unique_content);

        let preview = preview_write_intent(&ctx, &intent).expect("should succeed");
        let detail = preview.detail();

        // Content must appear in diff preview.
        let in_preview = preview
            .diff
            .preview
            .iter()
            .any(|l| l.contains(unique_content));
        assert!(in_preview, "content line should appear in diff.preview");

        // Content must NOT appear in detail().
        assert!(
            !detail.contains(unique_content),
            "detail() must not contain file content: {detail}"
        );
    }

    // (z) Preview does not create the target file (it is still absent afterward).
    #[test]
    fn preview_does_not_create_target_file() {
        let dir = TempDir::new();
        let ctx = make_context(dir.path());
        let target = dir.path().join("will_not_exist.txt");
        let intent = make_intent("will_not_exist.txt", "content");

        assert!(!target.exists(), "target should not exist before preview");
        let _ = preview_write_intent(&ctx, &intent).expect("should succeed");
        assert!(!target.exists(), "target should not exist after preview");
    }

    // (aa) Preview does not alter an existing file's bytes.
    #[test]
    fn preview_does_not_alter_existing_file() {
        let dir = TempDir::new();
        let original = "original content that must not change\n";
        std::fs::write(dir.path().join("existing.txt"), original).expect("write file");
        let ctx = make_context(dir.path());
        let intent = make_intent("existing.txt", "completely different replacement\n");

        let _ = preview_write_intent(&ctx, &intent).expect("should succeed");

        let after = std::fs::read_to_string(dir.path().join("existing.txt")).expect("read file");
        assert_eq!(after, original, "preview must not modify the existing file");
    }
}
