//! Pure in-memory data model representing a proposed file write.
//!
//! This module deliberately performs NO file I/O, NO path canonicalization,
//! and NO diff computation. It expresses *what* a future write would be;
//! it never executes anything.

/// Default maximum preview byte length used by [`WriteIntent::summary`].
pub const WRITE_INTENT_PREVIEW_BYTES: usize = 1024;

/// How the file should be written if the intent is eventually executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteIntentMode {
    /// Create the file if it does not exist, or replace it in full if it does.
    CreateOrReplace,
}

impl WriteIntentMode {
    /// Returns the lowercase snake_case string representation of this mode.
    pub fn as_str(&self) -> &'static str {
        match self {
            WriteIntentMode::CreateOrReplace => "create_or_replace",
        }
    }
}

/// Who or what originated the write intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteIntentSource {
    /// Directly initiated by a human operator.
    Operator,
    /// Proposed by the model as part of a response.
    ModelProposal,
    /// Resumed from a previously approved write that was deferred.
    ApprovalResume,
}

impl WriteIntentSource {
    /// Returns the lowercase snake_case string representation of this source.
    pub fn as_str(&self) -> &'static str {
        match self {
            WriteIntentSource::Operator => "operator",
            WriteIntentSource::ModelProposal => "model_proposal",
            WriteIntentSource::ApprovalResume => "approval_resume",
        }
    }
}

/// Errors that may occur when constructing a [`WriteIntent`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteIntentError {
    /// The supplied path was an empty string.
    EmptyPath,
}

impl std::fmt::Display for WriteIntentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WriteIntentError::EmptyPath => f.write_str("write intent path must not be empty"),
        }
    }
}

impl std::error::Error for WriteIntentError {}

/// A pure, in-memory record of a proposed file write.
///
/// No file I/O is performed when constructing or inspecting this value.
/// Use [`new_text`] to construct one; use [`WriteIntent::summary`] to obtain
/// a metadata snapshot safe for logging.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteIntent {
    /// Destination path as supplied by the caller (not canonicalized).
    pub path: String,
    /// Full content to be written.
    pub content: String,
    /// Write strategy to apply if this intent is eventually executed.
    pub mode: WriteIntentMode,
    /// Origin of this write intent.
    pub source: WriteIntentSource,
}

/// Constructs a [`WriteIntent`] for a plain-text write.
///
/// Returns [`Err(WriteIntentError::EmptyPath)`] when `path` converts to an
/// empty string. No filesystem access, no canonicalization, no panic.
pub fn new_text(
    path: impl Into<String>,
    content: impl Into<String>,
    mode: WriteIntentMode,
    source: WriteIntentSource,
) -> Result<WriteIntent, WriteIntentError> {
    let path = path.into();
    if path.is_empty() {
        return Err(WriteIntentError::EmptyPath);
    }
    Ok(WriteIntent {
        path,
        content: content.into(),
        mode,
        source,
    })
}

impl WriteIntent {
    /// Returns a metadata snapshot of this intent, including a bounded content
    /// preview safe for display.
    ///
    /// * `bytes` — `content.len()` in bytes.
    /// * `lines` — `content.lines().count()`.
    /// * `preview` — a UTF-8-safe prefix of `content` of at most
    ///   `max_preview_bytes` bytes, found via a descending `is_char_boundary`
    ///   scan so the cut never splits a multi-byte character.
    /// * `truncated` — `content.len() > max_preview_bytes`; content whose
    ///   length equals the limit exactly is **not** considered truncated.
    pub fn summary(&self, max_preview_bytes: usize) -> WriteIntentSummary {
        let bytes = self.content.len();
        let lines = self.content.lines().count();
        let truncated = bytes > max_preview_bytes;

        let preview = if truncated {
            let mut cut = max_preview_bytes;
            while cut > 0 && !self.content.is_char_boundary(cut) {
                cut -= 1;
            }
            self.content[..cut].to_string()
        } else {
            self.content.clone()
        };

        WriteIntentSummary {
            path: self.path.clone(),
            mode: self.mode,
            source: self.source,
            bytes,
            lines,
            preview,
            truncated,
        }
    }
}

/// A metadata snapshot of a [`WriteIntent`] suitable for logging and display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteIntentSummary {
    /// Destination path (not canonicalized).
    pub path: String,
    /// Write strategy.
    pub mode: WriteIntentMode,
    /// Origin of the intent.
    pub source: WriteIntentSource,
    /// Total byte length of the content.
    pub bytes: usize,
    /// Total line count of the content (`content.lines().count()`).
    pub lines: usize,
    /// UTF-8-safe content prefix bounded to the requested preview length.
    pub preview: String,
    /// `true` when the content exceeded the requested preview bound.
    pub truncated: bool,
}

impl WriteIntentSummary {
    /// Returns a summary-only detail string suitable for event logs.
    ///
    /// Deliberately omits `preview` so no file content or secret is emitted.
    pub fn detail(&self) -> String {
        format!(
            "path={:?} mode={} source={} bytes={} lines={} truncated={}",
            self.path,
            self.mode.as_str(),
            self.source.as_str(),
            self.bytes,
            self.lines,
            self.truncated,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_text_ok() {
        let intent = new_text(
            "src/main.rs",
            "fn main() {}",
            WriteIntentMode::CreateOrReplace,
            WriteIntentSource::Operator,
        )
        .unwrap();
        assert_eq!(intent.path, "src/main.rs");
        assert_eq!(intent.content, "fn main() {}");
        assert_eq!(intent.mode, WriteIntentMode::CreateOrReplace);
        assert_eq!(intent.source, WriteIntentSource::Operator);
    }

    #[test]
    fn new_text_empty_path_returns_err() {
        let result = new_text(
            "",
            "content",
            WriteIntentMode::CreateOrReplace,
            WriteIntentSource::Operator,
        );
        assert_eq!(result, Err(WriteIntentError::EmptyPath));
    }

    #[test]
    fn empty_path_display_message() {
        assert_eq!(
            WriteIntentError::EmptyPath.to_string(),
            "write intent path must not be empty"
        );
    }

    #[test]
    fn summary_bytes_equals_content_len() {
        let content = "hello world";
        let intent = new_text(
            "file.txt",
            content,
            WriteIntentMode::CreateOrReplace,
            WriteIntentSource::Operator,
        )
        .unwrap();
        let summary = intent.summary(WRITE_INTENT_PREVIEW_BYTES);
        assert_eq!(summary.bytes, content.len());
    }

    #[test]
    fn summary_lines_empty_string() {
        let intent = new_text(
            "file.txt",
            "",
            WriteIntentMode::CreateOrReplace,
            WriteIntentSource::Operator,
        )
        .unwrap();
        let summary = intent.summary(WRITE_INTENT_PREVIEW_BYTES);
        assert_eq!(summary.lines, 0);
    }

    #[test]
    fn summary_lines_single_word() {
        let intent = new_text(
            "file.txt",
            "hello",
            WriteIntentMode::CreateOrReplace,
            WriteIntentSource::Operator,
        )
        .unwrap();
        let summary = intent.summary(WRITE_INTENT_PREVIEW_BYTES);
        assert_eq!(summary.lines, 1);
    }

    #[test]
    fn summary_lines_trailing_newline() {
        let intent = new_text(
            "file.txt",
            "hello\n",
            WriteIntentMode::CreateOrReplace,
            WriteIntentSource::Operator,
        )
        .unwrap();
        let summary = intent.summary(WRITE_INTENT_PREVIEW_BYTES);
        assert_eq!(summary.lines, 1);
    }

    #[test]
    fn summary_preview_within_bound_not_truncated() {
        let content = "hello";
        let intent = new_text(
            "file.txt",
            content,
            WriteIntentMode::CreateOrReplace,
            WriteIntentSource::Operator,
        )
        .unwrap();
        let summary = intent.summary(1024);
        assert_eq!(summary.preview, content);
        assert!(!summary.truncated);
    }

    #[test]
    fn summary_preview_exceeds_bound_truncated() {
        let content = "a".repeat(2000);
        let intent = new_text(
            "file.txt",
            content.clone(),
            WriteIntentMode::CreateOrReplace,
            WriteIntentSource::Operator,
        )
        .unwrap();
        let summary = intent.summary(1024);
        assert!(summary.truncated);
        assert!(summary.preview.len() <= 1024);
        assert!(content.starts_with(&summary.preview));
    }

    #[test]
    fn summary_utf8_multibyte_char_boundary() {
        // '€' is 3 bytes (U+20AC). Build content of repeated '€'.
        let content = "€".repeat(100); // 300 bytes total
        // max_preview_bytes = 10; since 10 % 3 != 0, a naive cut would split a char
        let max_preview_bytes = 10;
        let intent = new_text(
            "file.txt",
            content.clone(),
            WriteIntentMode::CreateOrReplace,
            WriteIntentSource::Operator,
        )
        .unwrap();
        let summary = intent.summary(max_preview_bytes);
        assert!(summary.truncated);
        // Preview must be valid UTF-8
        assert!(std::str::from_utf8(summary.preview.as_bytes()).is_ok());
        // Preview byte length must be <= max_preview_bytes
        assert!(summary.preview.len() <= max_preview_bytes);
        // Content must start with the preview
        assert!(content.starts_with(&summary.preview));
    }

    #[test]
    fn summary_truncated_boundary_exact_length_not_truncated() {
        let content = "a".repeat(1024);
        let intent = new_text(
            "file.txt",
            content.clone(),
            WriteIntentMode::CreateOrReplace,
            WriteIntentSource::Operator,
        )
        .unwrap();
        let summary = intent.summary(1024);
        // content.len() == max_preview_bytes exactly → NOT truncated
        assert!(!summary.truncated);
        assert_eq!(summary.preview, content);
    }

    #[test]
    fn detail_does_not_contain_file_content() {
        let sentinel = "SUPER_SECRET_SENTINEL_VALUE_XYZ";
        let intent = new_text(
            "file.txt",
            sentinel,
            WriteIntentMode::CreateOrReplace,
            WriteIntentSource::Operator,
        )
        .unwrap();
        let summary = intent.summary(WRITE_INTENT_PREVIEW_BYTES);
        let detail = summary.detail();
        assert!(
            !detail.contains(sentinel),
            "detail() must not contain file content: {detail}"
        );
    }
}
