//! Parses and resolves `@path` workspace references for prompt injection.
//!
//! [`parse_workspace_references`] hand-scans plain text for `@path` tokens; it
//! performs no filesystem I/O and never panics. [`resolve_workspace_references`]
//! resolves each parsed token to bounded, read-only content by calling
//! [`crate::ToolRegistry::execute`] directly — all path safety (absolute-path
//! and `..` rejection, symlink-escape detection) is delegated to that layer.
//! This module never emits an event and never touches `ToolEventRunner`; no
//! `ToolPolicy`/`ToolCall`/`ToolResult` event is produced for a resolved `@`
//! reference.

use crate::ToolExecutionContext;
use crate::file_snippet::render_numbered_file_snippet;
// Reused (not duplicated) as the max-line-span cap for `@file:N-M` /
// `@file#LN-LM` range suffixes.
use crate::MAX_READ_RANGE_LIMIT_LINES;
use crate::tool::registry::{ToolError, ToolOutput, ToolRegistry, ToolRequest};

/// Maximum number of `@` references resolved per message; excess references
/// are counted in [`WorkspaceReferences::omitted`].
pub const WORKSPACE_REFERENCE_MAX_ITEMS: usize = 5;

/// Maximum aggregate bytes of rendered per-item blocks (as
/// [`WorkspaceReferences::render_prompt_section`] would emit them) included
/// in a single resolution pass.
pub const WORKSPACE_REFERENCE_MAX_BYTES: usize = 24 * 1024;

/// Maximum bytes of a single file's rendered preview.
pub const WORKSPACE_REFERENCE_FILE_PREVIEW_BYTES: usize = 8 * 1024;

/// Maximum number of directory entries rendered for a directory reference.
pub const WORKSPACE_REFERENCE_DIRECTORY_MAX_ENTRIES: usize = 100;

/// Maximum number of characters consumed for a single `@` token; longer runs
/// are dropped entirely at parse time.
pub const WORKSPACE_REFERENCE_MAX_PATH_CHARS: usize = 1024;

/// The exact marker [`render_numbered_file_snippet`] appends when its byte
/// cap fires; authoritative for detecting preview truncation.
const TRUNCATION_MARKER: &str = "\n... [truncated]";

/// A single `@path` token parsed out of user input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceReference {
    /// The exact text consumed after `@`, including any range suffix (used
    /// for display as `@{raw}`).
    pub raw: String,
    /// The path passed to the tool harness for resolution (range suffix
    /// stripped).
    pub path: String,
    /// An optional line-range suffix (`:90`, `:90-130`, `#L90`, `#L90-L130`);
    /// `None` when no suffix was present at all, `Some(Malformed)` when a
    /// suffix was attempted but did not parse.
    pub range: Option<WorkspaceReferenceRange>,
}

/// A parsed line-range suffix on a FILE `@` reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceReferenceRange {
    /// A valid, 1-based, inclusive line range.
    Lines { start_line: usize, end_line: usize },
    /// A range suffix was attempted but failed validation; carries a safe,
    /// user-facing detail describing why.
    Malformed { detail: String },
}

/// Classifies how a [`WorkspaceReference`] resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceReferenceKind {
    File,
    Directory,
    Missing,
    Error,
}

impl WorkspaceReferenceKind {
    /// Returns the lowercase label used in rendered output.
    fn as_str(self) -> &'static str {
        match self {
            WorkspaceReferenceKind::File => "file",
            WorkspaceReferenceKind::Directory => "directory",
            WorkspaceReferenceKind::Missing => "missing",
            WorkspaceReferenceKind::Error => "error",
        }
    }
}

/// A resolved `@path` reference, ready for prompt injection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedWorkspaceReference {
    pub reference: WorkspaceReference,
    pub kind: WorkspaceReferenceKind,
    pub content: String,
    pub truncated: bool,
}

/// The full result of resolving a set of parsed [`WorkspaceReference`]s.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceReferences {
    pub items: Vec<ResolvedWorkspaceReference>,
    /// Number of parsed references dropped due to the item cap or the
    /// aggregate byte budget.
    pub omitted: usize,
}

/// A concise, content-free summary of one resolved reference, suitable for
/// screen-log rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceReferenceSummary {
    pub raw: String,
    pub ok: bool,
    pub detail: String,
}

/// Returns `true` when a `@` at this position in the scan qualifies as the
/// start of a reference: start-of-string, or immediately preceded by ASCII
/// whitespace or one of the opening delimiters `(`, `[`, `{`.
fn is_qualifying_prev(prev: Option<char>) -> bool {
    match prev {
        None => true,
        Some(c) => c.is_ascii_whitespace() || matches!(c, '(' | '[' | '{'),
    }
}

/// Returns `true` when `c` may appear inside a reference's path token.
fn is_allowed_path_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '/' | '-')
}

/// Returns `true` when `c` may appear inside a range-suffix "run" following
/// `:` or `#L`: digits, ASCII letters (so a non-numeric attempt like `:abc`
/// is captured and classified as `Malformed` rather than silently ignored),
/// and `-` (the inclusive-range separator).
fn is_range_run_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-'
}

/// Parses digits-only unsigned integers (`^\d+$`); rejects empty strings and
/// any non-ASCII-digit character (including signs), so `"0"`, `"090"` parse
/// but `"-1"`, `"+1"`, `""`, `"1.0"` do not.
fn parse_digits(s: &str) -> Option<usize> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse::<usize>().ok()
}

/// Classifies two already-split numeric range endpoints per the `@reference`
/// range rules. Non-numeric input, `start < 1`, and `end < start` are all
/// `"invalid range"`; a span exceeding [`MAX_READ_RANGE_LIMIT_LINES`] is
/// `"range too large (max 500 lines)"`.
fn classify_range(start_str: &str, end_str: &str) -> WorkspaceReferenceRange {
    let (Some(start_line), Some(end_line)) = (parse_digits(start_str), parse_digits(end_str))
    else {
        return WorkspaceReferenceRange::Malformed {
            detail: "invalid range".to_string(),
        };
    };

    if start_line < 1 || end_line < start_line {
        return WorkspaceReferenceRange::Malformed {
            detail: "invalid range".to_string(),
        };
    }

    if end_line - start_line + 1 > MAX_READ_RANGE_LIMIT_LINES {
        return WorkspaceReferenceRange::Malformed {
            detail: "range too large (max 500 lines)".to_string(),
        };
    }

    WorkspaceReferenceRange::Lines {
        start_line,
        end_line,
    }
}

/// Validates a colon-form range run (the text after `:`, e.g. `"90"` or
/// `"90-130"`) against `^\d+(-\d+)?$` plus the range invariants.
fn parse_colon_range_run(run: &str) -> WorkspaceReferenceRange {
    match run.split_once('-') {
        Some((start, end)) => classify_range(start, end),
        None => classify_range(run, run),
    }
}

/// Validates a GitHub-form range run (the text after `#L`, e.g. `"90"` or
/// `"90-L130"`) against `^\d+(-L\d+)?$`; the end component must keep its own
/// `L` prefix, so `"90-130"` is `Malformed`.
fn parse_github_range_run(run: &str) -> WorkspaceReferenceRange {
    match run.split_once('-') {
        Some((start, end)) => match end.strip_prefix('L') {
            Some(end_digits) => classify_range(start, end_digits),
            None => WorkspaceReferenceRange::Malformed {
                detail: "invalid range".to_string(),
            },
        },
        None => classify_range(run, run),
    }
}

/// Attempts to parse an optional line-range suffix starting at char-index `j`
/// (the position right after a reference's path token, in the `chars` index
/// space used by [`parse_workspace_references`]). Returns the char-index just
/// past the consumed suffix (`j` itself when nothing was consumed) and the
/// parsed range, if any.
///
/// An empty run after `:` or `#L` means the delimiter is ordinary prose, not
/// a range attempt, and is left completely unconsumed. Never panics.
fn parse_range_suffix(
    chars: &[(usize, char)],
    len: usize,
    input: &str,
    j: usize,
) -> (usize, Option<WorkspaceReferenceRange>) {
    let byte_at = |idx: usize| if idx < len { chars[idx].0 } else { input.len() };

    if j >= len {
        return (j, None);
    }

    match chars[j].1 {
        ':' => {
            let run_start = j + 1;
            let mut k = run_start;
            while k < len && is_range_run_char(chars[k].1) {
                k += 1;
            }
            if k == run_start {
                return (j, None);
            }
            let run = &input[byte_at(run_start)..byte_at(k)];
            (k, Some(parse_colon_range_run(run)))
        }
        '#' if j + 1 < len && chars[j + 1].1 == 'L' => {
            let run_start = j + 2;
            let mut k = run_start;
            while k < len && is_range_run_char(chars[k].1) {
                k += 1;
            }
            if k == run_start {
                return (j, None);
            }
            let run = &input[byte_at(run_start)..byte_at(k)];
            (k, Some(parse_github_range_run(run)))
        }
        _ => (j, None),
    }
}

/// Extracts `@path[:range]`/`@path[#Lrange]` reference tokens from plain-text
/// `input`.
///
/// Performs no filesystem I/O and never panics on any input, including
/// malformed UTF-8 boundaries (handled transparently since `input` is
/// already a valid `&str`).
pub fn parse_workspace_references(input: &str) -> Vec<WorkspaceReference> {
    let chars: Vec<(usize, char)> = input.char_indices().collect();
    let len = chars.len();
    let byte_at = |idx: usize| if idx < len { chars[idx].0 } else { input.len() };

    let mut result: Vec<WorkspaceReference> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut prev: Option<char> = None;
    let mut i = 0usize;

    while i < len {
        let ch = chars[i].1;

        if ch == '@' && is_qualifying_prev(prev) {
            let mut j = i + 1;
            while j < len && is_allowed_path_char(chars[j].1) {
                j += 1;
            }

            let start_byte = byte_at(i + 1);
            let path_end_byte = byte_at(j);
            let path_token = &input[start_byte..path_end_byte];

            // Attempt an optional line-range suffix immediately following the
            // path token: colon form (`:90`, `:90-130`) or GitHub form
            // (`#L90`, `#L90-L130`).
            let (suffix_end, range) = parse_range_suffix(&chars, len, input, j);
            let raw_end_byte = byte_at(suffix_end);
            let raw_token = &input[start_byte..raw_end_byte];

            if !path_token.is_empty()
                && path_token.len() <= WORKSPACE_REFERENCE_MAX_PATH_CHARS
                && !path_token.chars().all(|c| c == '.' || c == '/')
                && seen.insert(raw_token.to_string())
            {
                result.push(WorkspaceReference {
                    raw: raw_token.to_string(),
                    path: path_token.to_string(),
                    range,
                });
            }

            // Advance past the whole attempted token, including any consumed
            // range suffix (or just past `@` when nothing qualifying followed
            // it), tracking `prev` accordingly so a subsequent `@` sees the
            // correct boundary character.
            prev = Some(if suffix_end > i + 1 {
                chars[suffix_end - 1].1
            } else {
                ch
            });
            i = suffix_end.max(i + 1);
            continue;
        }

        prev = Some(ch);
        i += 1;
    }

    result
}

/// Maps a [`ToolError`] to the safe `(label, kind)` pair used in error
/// rendering. `NotFound` is the only variant classified as `Missing`; every
/// other variant is `Error`.
fn classify_tool_error(err: &ToolError) -> (&'static str, WorkspaceReferenceKind) {
    match err {
        ToolError::NotFound { .. } => ("not found", WorkspaceReferenceKind::Missing),
        ToolError::WorkspaceViolation { .. } => {
            ("workspace violation", WorkspaceReferenceKind::Error)
        }
        ToolError::NonUtf8 { .. } => ("non-UTF8 file", WorkspaceReferenceKind::Error),
        ToolError::TooLarge { .. } => ("file too large", WorkspaceReferenceKind::Error),
        ToolError::NotADirectory { .. } | ToolError::NotAFile { .. } => {
            ("not a file or directory", WorkspaceReferenceKind::Error)
        }
        ToolError::Io { .. } => ("read error", WorkspaceReferenceKind::Error),
        // Not reachable via the ReadFile/ListFiles dispatch this module uses,
        // but handled defensively so this function can never panic.
        ToolError::PolicyDenied { .. }
        | ToolError::ApprovalRequired { .. }
        | ToolError::InvalidPattern { .. } => ("read error", WorkspaceReferenceKind::Error),
    }
}

/// Builds a safe error item: `Reference: @{raw}\nError: {label}`.
fn error_item(
    reference: &WorkspaceReference,
    label: &str,
    kind: WorkspaceReferenceKind,
) -> ResolvedWorkspaceReference {
    ResolvedWorkspaceReference {
        reference: reference.clone(),
        kind,
        content: format!("Reference: @{}\nError: {}", reference.raw, label),
        truncated: false,
    }
}

/// Renders a `list_files` result as a bounded, deterministic `- entry` listing.
fn render_directory_listing(entries: &[String]) -> (String, bool) {
    let truncated = entries.len() > WORKSPACE_REFERENCE_DIRECTORY_MAX_ENTRIES;
    let take = entries.len().min(WORKSPACE_REFERENCE_DIRECTORY_MAX_ENTRIES);

    let mut lines: Vec<String> = entries[..take].iter().map(|e| format!("- {e}")).collect();
    if truncated {
        lines.push("... listing truncated".to_string());
    }

    (lines.join("\n"), truncated)
}

/// Resolves one [`WorkspaceReference`] against the tool harness, branching on
/// its optional line-range suffix.
fn resolve_reference(
    registry: &ToolRegistry,
    ctx: &ToolExecutionContext,
    reference: &WorkspaceReference,
) -> ResolvedWorkspaceReference {
    match &reference.range {
        Some(WorkspaceReferenceRange::Malformed { detail }) => {
            // Malformed never touches the filesystem.
            error_item(reference, detail, WorkspaceReferenceKind::Error)
        }
        Some(WorkspaceReferenceRange::Lines {
            start_line,
            end_line,
        }) => resolve_ranged_reference(registry, ctx, reference, *start_line, *end_line),
        None => resolve_full_reference(registry, ctx, reference),
    }
}

/// Resolves a reference with no range suffix: a full file read, falling back
/// to a directory listing when the path is not a file.
fn resolve_full_reference(
    registry: &ToolRegistry,
    ctx: &ToolExecutionContext,
    reference: &WorkspaceReference,
) -> ResolvedWorkspaceReference {
    let read_result = registry.execute(
        ctx,
        ToolRequest::ReadFile {
            path: reference.path.clone(),
            offset: None,
            limit: None,
        },
    );

    match read_result {
        Ok(ToolOutput::FileContent { content, .. }) => {
            let snippet = render_numbered_file_snippet(
                &reference.path,
                &content,
                1,
                None,
                false,
                WORKSPACE_REFERENCE_FILE_PREVIEW_BYTES,
            );
            // Authoritative truncation detection: the renderer appends this
            // exact marker only when its byte cap fired.
            let truncated = snippet.ends_with(TRUNCATION_MARKER);
            ResolvedWorkspaceReference {
                reference: reference.clone(),
                kind: WorkspaceReferenceKind::File,
                content: snippet,
                truncated,
            }
        }
        // `ReadFile` only ever yields `FileContent` on success; handled
        // defensively so a future output variant can never panic here.
        Ok(_) => error_item(reference, "read error", WorkspaceReferenceKind::Error),
        Err(ToolError::NotAFile { .. }) => {
            match registry.execute(
                ctx,
                ToolRequest::ListFiles {
                    path: reference.path.clone(),
                },
            ) {
                Ok(ToolOutput::FileList { entries, .. }) => {
                    let (content, truncated) = render_directory_listing(&entries);
                    ResolvedWorkspaceReference {
                        reference: reference.clone(),
                        kind: WorkspaceReferenceKind::Directory,
                        content,
                        truncated,
                    }
                }
                Ok(_) => error_item(reference, "read error", WorkspaceReferenceKind::Error),
                Err(err) => {
                    let (label, kind) = classify_tool_error(&err);
                    error_item(reference, label, kind)
                }
            }
        }
        Err(err) => {
            let (label, kind) = classify_tool_error(&err);
            error_item(reference, label, kind)
        }
    }
}

/// Resolves a reference carrying a [`WorkspaceReferenceRange::Lines`] suffix
/// via a bounded range read; never falls back to a directory listing.
///
/// Re-validates the range invariants before computing `end_line - start_line
/// + 1` or touching the filesystem: both [`WorkspaceReferenceRange`] and
/// `resolve_workspace_references` are `pub`, so a hand-constructed
/// `Lines { start_line: 0, end_line: usize::MAX }` must be rejected here too,
/// not only at parse time — this guard is what keeps that arithmetic from
/// underflowing/panicking.
fn resolve_ranged_reference(
    registry: &ToolRegistry,
    ctx: &ToolExecutionContext,
    reference: &WorkspaceReference,
    start_line: usize,
    end_line: usize,
) -> ResolvedWorkspaceReference {
    if start_line < 1
        || end_line < start_line
        || end_line - start_line + 1 > MAX_READ_RANGE_LIMIT_LINES
    {
        return error_item(reference, "invalid range", WorkspaceReferenceKind::Error);
    }
    let count = end_line - start_line + 1;

    let read_result = registry.execute(
        ctx,
        ToolRequest::ReadFile {
            path: reference.path.clone(),
            offset: Some(start_line),
            limit: Some(count),
        },
    );

    match read_result {
        Ok(ToolOutput::FileContent {
            content,
            start_line: sl,
            line_count,
            ..
        }) => {
            let snippet = render_numbered_file_snippet(
                &reference.path,
                &content,
                sl.unwrap_or(start_line),
                line_count,
                false,
                WORKSPACE_REFERENCE_FILE_PREVIEW_BYTES,
            );
            // Authoritative truncation detection: the renderer appends this
            // exact marker only when its byte cap fired.
            let truncated = snippet.ends_with(TRUNCATION_MARKER);
            ResolvedWorkspaceReference {
                reference: reference.clone(),
                kind: WorkspaceReferenceKind::File,
                content: snippet,
                truncated,
            }
        }
        // `ReadFile` only ever yields `FileContent` on success; handled
        // defensively so a future output variant can never panic here.
        Ok(_) => error_item(reference, "read error", WorkspaceReferenceKind::Error),
        // A line range only makes sense for a file; never fall back to a
        // directory listing here.
        Err(ToolError::NotAFile { .. }) => error_item(
            reference,
            "line range is only supported for files",
            WorkspaceReferenceKind::Error,
        ),
        Err(err) => {
            let (label, kind) = classify_tool_error(&err);
            error_item(reference, label, kind)
        }
    }
}

/// Renders the exact per-item block [`WorkspaceReferences::render_prompt_section`]
/// emits for one resolved reference; used both for the final rendering and to
/// measure the aggregate byte budget.
fn render_item_block(item: &ResolvedWorkspaceReference) -> String {
    let range_line = match (item.kind, &item.reference.range) {
        (
            WorkspaceReferenceKind::File,
            Some(WorkspaceReferenceRange::Lines {
                start_line,
                end_line,
            }),
        ) => format!("  range: {start_line}-{end_line}\n"),
        _ => String::new(),
    };

    format!(
        "Source:\n  reference: @{}\n  kind: {}\n{}  risk: read_only\n\nContent:\n{}",
        item.reference.raw,
        item.kind.as_str(),
        range_line,
        item.content
    )
}

/// Resolves parsed `@` references to bounded, read-only content.
///
/// Includes at most [`WORKSPACE_REFERENCE_MAX_ITEMS`] references. Beyond that
/// cap — or once the aggregate rendered-block byte budget
/// ([`WORKSPACE_REFERENCE_MAX_BYTES`]) would be exceeded — all remaining
/// references (in order) are counted in [`WorkspaceReferences::omitted`]
/// instead of being resolved. Never touches `ToolEventRunner` and never
/// emits an event; never panics.
pub fn resolve_workspace_references(
    ctx: &ToolExecutionContext,
    refs: &[WorkspaceReference],
) -> WorkspaceReferences {
    let registry = ToolRegistry::new_readonly();

    let mut items: Vec<ResolvedWorkspaceReference> = Vec::new();
    let mut used_bytes: usize = 0;
    let mut omitted: usize = 0;
    let mut budget_exceeded = false;

    for reference in refs {
        if budget_exceeded || items.len() >= WORKSPACE_REFERENCE_MAX_ITEMS {
            omitted += 1;
            continue;
        }

        let resolved = resolve_reference(&registry, ctx, reference);
        let block_len = render_item_block(&resolved).len();

        if used_bytes + block_len > WORKSPACE_REFERENCE_MAX_BYTES {
            budget_exceeded = true;
            omitted += 1;
            continue;
        }

        used_bytes += block_len;
        items.push(resolved);
    }

    WorkspaceReferences { items, omitted }
}

impl WorkspaceReferences {
    /// Renders the `Referenced Workspace Context:` prompt section.
    ///
    /// Returns an empty `String` when there is nothing to show. Otherwise the
    /// header is followed by each item's `Source:`/`Content:` block, blank-line
    /// separated, ending with an `Omitted N additional @ references due to
    /// context limit.` line when `omitted > 0`.
    pub fn render_prompt_section(&self) -> String {
        if self.items.is_empty() && self.omitted == 0 {
            return String::new();
        }

        let mut parts: Vec<String> = vec!["Referenced Workspace Context:".to_string()];
        for item in &self.items {
            parts.push(render_item_block(item));
        }
        if self.omitted > 0 {
            parts.push(format!(
                "Omitted {} additional @ references due to context limit.",
                self.omitted
            ));
        }

        parts.join("\n\n")
    }

    /// Returns a concise, content-free summary of each resolved item for
    /// screen-log rendering.
    pub fn summaries(&self) -> Vec<WorkspaceReferenceSummary> {
        self.items
            .iter()
            .map(|item| {
                let ok = matches!(
                    item.kind,
                    WorkspaceReferenceKind::File | WorkspaceReferenceKind::Directory
                );

                let mut detail = if ok {
                    item.kind.as_str().to_string()
                } else {
                    // Error/Missing content is always exactly
                    // "Reference: @{raw}\nError: {label}" (built by `error_item`).
                    item.content
                        .split_once("Error: ")
                        .map(|(_, label)| label.to_string())
                        .unwrap_or_default()
                };

                if ok && item.truncated {
                    detail.push_str(" (preview truncated)");
                }

                WorkspaceReferenceSummary {
                    raw: item.reference.raw.clone(),
                    ok,
                    detail,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::MAX_READ_FILE_BYTES;

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    /// RAII guard that creates a unique temp directory and removes it on
    /// drop, mirroring `crates/kernel/src/tool/registry/tests.rs`.
    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new() -> Self {
            let count = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
            let name = format!(
                "caravan_workspace_reference_test_{}_{}",
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

    fn reference(raw: &str) -> WorkspaceReference {
        WorkspaceReference {
            raw: raw.to_string(),
            path: raw.to_string(),
            range: None,
        }
    }

    // ─── parse_workspace_references ─────────────────────────────────────────

    #[test]
    fn parse_single_file_reference() {
        let refs = parse_workspace_references("please check @README.md today");
        assert_eq!(
            refs,
            vec![WorkspaceReference {
                raw: "README.md".to_string(),
                path: "README.md".to_string(),
                range: None,
            }]
        );
    }

    #[test]
    fn parse_directory_reference_preserves_trailing_slash() {
        let refs = parse_workspace_references("see @crates/kernel/src/ for details");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].path, "crates/kernel/src/");
    }

    #[test]
    fn parse_multiple_references_in_first_occurrence_order() {
        let refs = parse_workspace_references("compare @a.rs against @b.rs and @c.rs");
        let paths: Vec<&str> = refs.iter().map(|r| r.path.as_str()).collect();
        assert_eq!(paths, vec!["a.rs", "b.rs", "c.rs"]);
    }

    #[test]
    fn parse_stops_at_trailing_punctuation() {
        let refs = parse_workspace_references("read @README.md, then summarize");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].path, "README.md");
    }

    #[test]
    fn parse_stops_at_closing_paren_and_starts_after_open_paren() {
        let refs = parse_workspace_references("(see @README.md) for context");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].path, "README.md");
    }

    #[test]
    fn parse_skips_mid_word_at_sign_in_email() {
        let refs = parse_workspace_references("contact wontak@wrtn.io for help");
        assert!(refs.is_empty(), "expected no reference, got: {refs:?}");
    }

    #[test]
    fn parse_deduplicates_same_raw_token_keeping_first_occurrence() {
        let refs = parse_workspace_references("@a.rs then again @a.rs and @b.rs");
        let paths: Vec<&str> = refs.iter().map(|r| r.path.as_str()).collect();
        assert_eq!(paths, vec!["a.rs", "b.rs"]);
    }

    #[test]
    fn parse_degenerate_tokens_yield_no_reference() {
        for input in ["@", "@ ", "(@)", "@,", "@."] {
            let refs = parse_workspace_references(input);
            assert!(
                refs.is_empty(),
                "expected no reference for input {input:?}, got: {refs:?}"
            );
        }
    }

    #[test]
    fn parse_overlong_token_is_dropped() {
        let overlong = "a".repeat(WORKSPACE_REFERENCE_MAX_PATH_CHARS + 1);
        let input = format!("@{overlong} rest of message");
        let refs = parse_workspace_references(&input);
        assert!(
            refs.is_empty(),
            "expected overlong token to be dropped, got: {refs:?}"
        );
    }

    #[test]
    fn parse_max_length_token_is_kept() {
        let exact = "a".repeat(WORKSPACE_REFERENCE_MAX_PATH_CHARS);
        let input = format!("@{exact} rest");
        let refs = parse_workspace_references(&input);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].path.len(), WORKSPACE_REFERENCE_MAX_PATH_CHARS);
    }

    // ─── @reference line-range parsing ──────────────────────────────────────

    #[test]
    fn parse_no_range_suffix_is_none() {
        let refs = parse_workspace_references("@README.md today");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].range, None);
    }

    #[test]
    fn parse_colon_single_line_range() {
        let refs = parse_workspace_references("@file.rs:10 rest");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].path, "file.rs");
        assert_eq!(refs[0].raw, "file.rs:10");
        assert_eq!(
            refs[0].range,
            Some(WorkspaceReferenceRange::Lines {
                start_line: 10,
                end_line: 10
            })
        );
    }

    #[test]
    fn parse_colon_multi_line_range() {
        let refs = parse_workspace_references("@file.rs:10-20 rest");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].path, "file.rs");
        assert_eq!(refs[0].raw, "file.rs:10-20");
        assert_eq!(
            refs[0].range,
            Some(WorkspaceReferenceRange::Lines {
                start_line: 10,
                end_line: 20
            })
        );
    }

    #[test]
    fn parse_github_single_line_range() {
        let refs = parse_workspace_references("@file.rs#L10 rest");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].path, "file.rs");
        assert_eq!(refs[0].raw, "file.rs#L10");
        assert_eq!(
            refs[0].range,
            Some(WorkspaceReferenceRange::Lines {
                start_line: 10,
                end_line: 10
            })
        );
    }

    #[test]
    fn parse_github_multi_line_range() {
        let refs = parse_workspace_references("@file.rs#L10-L20 rest");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].path, "file.rs");
        assert_eq!(refs[0].raw, "file.rs#L10-L20");
        assert_eq!(
            refs[0].range,
            Some(WorkspaceReferenceRange::Lines {
                start_line: 10,
                end_line: 20
            })
        );
    }

    #[test]
    fn parse_colon_range_stops_at_trailing_punctuation() {
        let refs = parse_workspace_references("read @README.md:10, then summarize");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].path, "README.md");
        assert_eq!(refs[0].raw, "README.md:10");
        assert_eq!(
            refs[0].range,
            Some(WorkspaceReferenceRange::Lines {
                start_line: 10,
                end_line: 10
            })
        );
    }

    #[test]
    fn parse_github_range_stops_at_closing_paren() {
        let refs = parse_workspace_references("(@README.md#L10-L20) for context");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].path, "README.md");
        assert_eq!(refs[0].raw, "README.md#L10-L20");
        assert_eq!(
            refs[0].range,
            Some(WorkspaceReferenceRange::Lines {
                start_line: 10,
                end_line: 20
            })
        );
    }

    #[test]
    fn parse_prose_colon_is_not_a_range() {
        let refs = parse_workspace_references("@README.md: please read");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].path, "README.md");
        assert_eq!(refs[0].raw, "README.md");
        assert_eq!(refs[0].range, None);
    }

    #[test]
    fn parse_column_syntax_degrades_to_line_only() {
        // `@file:10:5` (unsupported column syntax): the maximal-run rule
        // consumes `10` and leaves `:5` as unconsumed prose.
        let refs = parse_workspace_references("@file.rs:10:5 rest");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].path, "file.rs");
        assert_eq!(refs[0].raw, "file.rs:10");
        assert_eq!(
            refs[0].range,
            Some(WorkspaceReferenceRange::Lines {
                start_line: 10,
                end_line: 10
            })
        );
    }

    #[test]
    fn parse_malformed_ranges_are_flagged() {
        let cases = [
            ("@f:abc", "invalid range"),
            ("@f:0", "invalid range"),
            ("@f:10-0", "invalid range"),
            ("@f:30-10", "invalid range"),
            ("@f:10-abc", "invalid range"),
            ("@f#Labc", "invalid range"),
            ("@f#L90-130", "invalid range"),
        ];
        for (input, expected_detail) in cases {
            let refs = parse_workspace_references(input);
            assert_eq!(refs.len(), 1, "input {input:?} produced {refs:?}");
            assert_eq!(refs[0].path, "f", "input {input:?}");
            match &refs[0].range {
                Some(WorkspaceReferenceRange::Malformed { detail }) => {
                    assert_eq!(detail, expected_detail, "input {input:?}");
                }
                other => panic!("expected Malformed for input {input:?}, got {other:?}"),
            }
        }
    }

    #[test]
    fn parse_over_cap_range_is_malformed_with_detail() {
        let refs = parse_workspace_references("@big.rs:1-501");
        assert_eq!(refs.len(), 1);
        assert_eq!(
            refs[0].range,
            Some(WorkspaceReferenceRange::Malformed {
                detail: "range too large (max 500 lines)".to_string()
            })
        );
    }

    #[test]
    fn parse_range_suffix_raw_includes_it_path_excludes_it() {
        let refs = parse_workspace_references("@src/lib.rs:5-9 rest");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].path, "src/lib.rs");
        assert_eq!(refs[0].raw, "src/lib.rs:5-9");
    }

    #[test]
    fn parse_dedup_distinct_raw_tokens_by_range() {
        let refs = parse_workspace_references("@a.rs:1 then @a.rs:2 and @a.rs:1 again");
        let raws: Vec<&str> = refs.iter().map(|r| r.raw.as_str()).collect();
        assert_eq!(raws, vec!["a.rs:1", "a.rs:2"]);
    }

    #[test]
    fn parse_never_panics_on_range_like_input() {
        let inputs = [
            "@f:abc",
            "@f:0",
            "@f:10-0",
            "@f:30-10",
            "@f:10-abc",
            "@f#Labc",
            "@f#L90-130",
            "@f:",
            "@f#",
            "@f#L",
            "@f:-",
            "@f#L-",
            "@f:999999999999999999999999999999",
            "@f#L999999999999999999999999999999",
        ];
        for input in inputs {
            let _ = parse_workspace_references(input);
        }
    }

    // ─── resolve_workspace_references ───────────────────────────────────────

    #[test]
    fn resolve_file_produces_numbered_snippet() {
        let dir = TempDir::new();
        std::fs::write(dir.path().join("hello.txt"), "hello, world!").expect("write file");

        let ctx = make_context(dir.path());
        let refs = vec![reference("hello.txt")];
        let resolved = resolve_workspace_references(&ctx, &refs);

        assert_eq!(resolved.items.len(), 1);
        let item = &resolved.items[0];
        assert_eq!(item.kind, WorkspaceReferenceKind::File);
        assert!(item.content.contains("File: "), "missing File: header");
        assert!(item.content.contains("   1 | "), "missing line 1 prefix");
        assert!(!item.truncated);
    }

    #[test]
    fn resolve_directory_lists_entries_not_file_contents() {
        let dir = TempDir::new();
        std::fs::create_dir_all(dir.path().join("subdir")).expect("create subdir");
        std::fs::write(
            dir.path().join("subdir/secret.txt"),
            "SECRET_MARKER_CONTENT",
        )
        .expect("write file");

        let ctx = make_context(dir.path());
        let refs = vec![reference("subdir")];
        let resolved = resolve_workspace_references(&ctx, &refs);

        let item = &resolved.items[0];
        assert_eq!(item.kind, WorkspaceReferenceKind::Directory);
        assert!(item.content.contains("- secret.txt"));
        assert!(!item.content.contains("SECRET_MARKER_CONTENT"));
    }

    #[test]
    fn resolve_missing_path_is_not_found() {
        let dir = TempDir::new();
        let ctx = make_context(dir.path());
        let refs = vec![reference("does_not_exist.txt")];
        let resolved = resolve_workspace_references(&ctx, &refs);

        let item = &resolved.items[0];
        assert_eq!(item.kind, WorkspaceReferenceKind::Missing);
        assert!(item.content.contains("Error: not found"));
    }

    #[test]
    fn resolve_absolute_path_is_workspace_violation() {
        let dir = TempDir::new();
        let ctx = make_context(dir.path());
        let refs = vec![reference("/etc/passwd")];
        let resolved = resolve_workspace_references(&ctx, &refs);

        let item = &resolved.items[0];
        assert_eq!(item.kind, WorkspaceReferenceKind::Error);
        assert!(item.content.contains("Error: workspace violation"));
    }

    #[test]
    fn resolve_parent_escape_is_workspace_violation() {
        let dir = TempDir::new();
        let ctx = make_context(dir.path());
        let refs = vec![reference("../escape")];
        let resolved = resolve_workspace_references(&ctx, &refs);

        let item = &resolved.items[0];
        assert_eq!(item.kind, WorkspaceReferenceKind::Error);
        assert!(item.content.contains("Error: workspace violation"));
    }

    #[cfg(unix)]
    #[test]
    fn resolve_symlink_escape_is_workspace_violation() {
        let dir = TempDir::new();
        let outside = TempDir::new();
        let link_path = dir.path().join("escape_link");
        std::os::unix::fs::symlink(outside.path(), &link_path).expect("failed to create symlink");

        let ctx = make_context(dir.path());
        let refs = vec![reference("escape_link")];
        let resolved = resolve_workspace_references(&ctx, &refs);

        let item = &resolved.items[0];
        assert_eq!(item.kind, WorkspaceReferenceKind::Error);
        assert!(item.content.contains("Error: workspace violation"));
    }

    #[test]
    fn resolve_preview_truncation_over_cap_but_under_read_limit() {
        let dir = TempDir::new();
        let content = "x".repeat(WORKSPACE_REFERENCE_FILE_PREVIEW_BYTES + 2_000);
        assert!((content.len() as u64) < MAX_READ_FILE_BYTES);
        std::fs::write(dir.path().join("big_preview.txt"), &content).expect("write file");

        let ctx = make_context(dir.path());
        let refs = vec![reference("big_preview.txt")];
        let resolved = resolve_workspace_references(&ctx, &refs);

        let item = &resolved.items[0];
        assert_eq!(item.kind, WorkspaceReferenceKind::File);
        assert!(item.truncated, "expected preview truncation");

        let summaries = resolved.summaries();
        assert!(
            summaries[0].detail.contains("(preview truncated)"),
            "detail did not mark preview truncation: {:?}",
            summaries[0]
        );
    }

    #[test]
    fn resolve_oversized_file_maps_to_file_too_large_error() {
        let dir = TempDir::new();
        let content = "y".repeat((MAX_READ_FILE_BYTES as usize) + 1_000);
        std::fs::write(dir.path().join("huge.txt"), &content).expect("write file");

        let ctx = make_context(dir.path());
        let refs = vec![reference("huge.txt")];
        let resolved = resolve_workspace_references(&ctx, &refs);

        let item = &resolved.items[0];
        assert_eq!(item.kind, WorkspaceReferenceKind::Error);
        assert!(item.content.contains("file too large"));

        let summaries = resolved.summaries();
        assert!(summaries[0].detail.contains("file too large"));
    }

    #[test]
    fn resolve_aggregate_byte_budget_omits_when_exceeded() {
        let dir = TempDir::new();
        let content = "z".repeat(WORKSPACE_REFERENCE_FILE_PREVIEW_BYTES + 2_000);
        let names = ["one.txt", "two.txt", "three.txt"];
        for name in names {
            std::fs::write(dir.path().join(name), &content).expect("write file");
        }

        let ctx = make_context(dir.path());
        let refs: Vec<WorkspaceReference> = names.iter().map(|n| reference(n)).collect();
        assert!(refs.len() < WORKSPACE_REFERENCE_MAX_ITEMS);

        let resolved = resolve_workspace_references(&ctx, &refs);

        assert!(
            resolved.omitted > 0,
            "expected omitted > 0 once the aggregate byte budget is exceeded"
        );
        assert!(resolved.render_prompt_section().contains("Omitted"));
    }

    #[test]
    fn resolve_more_than_item_cap_omits_extras() {
        let dir = TempDir::new();
        let mut refs = Vec::new();
        for i in 0..(WORKSPACE_REFERENCE_MAX_ITEMS + 2) {
            let name = format!("f{i}.txt");
            std::fs::write(dir.path().join(&name), "small").expect("write file");
            refs.push(reference(&name));
        }

        let ctx = make_context(dir.path());
        let resolved = resolve_workspace_references(&ctx, &refs);

        assert_eq!(resolved.items.len(), WORKSPACE_REFERENCE_MAX_ITEMS);
        assert!(resolved.omitted > 0);
        assert!(resolved.render_prompt_section().contains("Omitted"));
    }

    #[test]
    fn render_prompt_section_header_and_content_label() {
        let dir = TempDir::new();
        std::fs::write(dir.path().join("hello.txt"), "hi").expect("write file");

        let ctx = make_context(dir.path());
        let refs = vec![reference("hello.txt")];
        let resolved = resolve_workspace_references(&ctx, &refs);
        let section = resolved.render_prompt_section();

        assert!(section.starts_with("Referenced Workspace Context:"));
        let header_idx = section.find("Content:").expect("missing Content: label");
        let hi_idx = section.find("hi").expect("missing file content");
        assert!(hi_idx > header_idx, "content did not follow Content: label");
    }

    #[test]
    fn render_prompt_section_empty_when_nothing_to_show() {
        let empty = WorkspaceReferences {
            items: Vec::new(),
            omitted: 0,
        };
        assert_eq!(empty.render_prompt_section(), "");
    }

    // ─── @reference line-range resolution ───────────────────────────────────

    fn write_numbered_lines(path: &Path, count: usize) {
        let content: String = (1..=count).map(|n| format!("line{n}\n")).collect();
        std::fs::write(path, content).expect("write file");
    }

    #[test]
    fn resolve_colon_range_reads_offset_and_limit() {
        let dir = TempDir::new();
        write_numbered_lines(&dir.path().join("f.txt"), 30);

        let ctx = make_context(dir.path());
        let refs = parse_workspace_references("@f.txt:10-20");
        let resolved = resolve_workspace_references(&ctx, &refs);

        let item = &resolved.items[0];
        assert_eq!(item.kind, WorkspaceReferenceKind::File);
        assert!(
            item.content.contains("Lines: 10-20"),
            "missing Lines header: {}",
            item.content
        );
        assert!(
            item.content.contains("  10 | line10"),
            "missing numbered line 10: {}",
            item.content
        );
    }

    #[test]
    fn resolve_github_range_reads_offset_and_limit() {
        let dir = TempDir::new();
        write_numbered_lines(&dir.path().join("f.txt"), 30);

        let ctx = make_context(dir.path());
        let refs = parse_workspace_references("@f.txt#L10-L20");
        let resolved = resolve_workspace_references(&ctx, &refs);

        let item = &resolved.items[0];
        assert_eq!(item.kind, WorkspaceReferenceKind::File);
        assert!(
            item.content.contains("Lines: 10-20"),
            "missing Lines header: {}",
            item.content
        );
        assert!(
            item.content.contains("  10 | line10"),
            "missing numbered line 10: {}",
            item.content
        );
    }

    #[test]
    fn resolve_directory_with_range_is_error_not_listing() {
        let dir = TempDir::new();
        std::fs::create_dir_all(dir.path().join("subdir")).expect("create subdir");

        let ctx = make_context(dir.path());
        let refs = parse_workspace_references("@subdir:5-9");
        let resolved = resolve_workspace_references(&ctx, &refs);

        let item = &resolved.items[0];
        assert_eq!(item.kind, WorkspaceReferenceKind::Error);
        assert!(
            item.content
                .contains("line range is only supported for files")
        );
    }

    #[test]
    fn resolve_missing_path_with_range_is_safe_summary() {
        let dir = TempDir::new();
        let ctx = make_context(dir.path());
        let refs = parse_workspace_references("@does_not_exist.txt:5-9");
        let resolved = resolve_workspace_references(&ctx, &refs);

        let item = &resolved.items[0];
        assert_eq!(item.kind, WorkspaceReferenceKind::Missing);
        assert!(item.content.contains("Error: not found"));
    }

    #[test]
    fn resolve_malformed_range_never_touches_filesystem() {
        let dir = TempDir::new();
        let ctx = make_context(dir.path());
        // "does_not_exist.txt" is never created; if the resolver read the
        // filesystem it would surface "not found", not "invalid range".
        let refs = parse_workspace_references("@does_not_exist.txt:abc");
        let resolved = resolve_workspace_references(&ctx, &refs);

        let item = &resolved.items[0];
        assert_eq!(item.kind, WorkspaceReferenceKind::Error);
        assert!(item.content.contains("Error: invalid range"));
    }

    #[test]
    fn resolve_over_cap_range_is_error() {
        let dir = TempDir::new();
        let ctx = make_context(dir.path());
        let refs = parse_workspace_references("@does_not_exist.txt:1-600");
        let resolved = resolve_workspace_references(&ctx, &refs);

        let item = &resolved.items[0];
        assert_eq!(item.kind, WorkspaceReferenceKind::Error);
        assert!(item.content.contains("range too large"));
    }

    #[test]
    fn render_item_block_includes_range_line_for_ranged_file_only() {
        let dir = TempDir::new();
        write_numbered_lines(&dir.path().join("f.txt"), 30);
        std::fs::write(dir.path().join("full.txt"), "hello").expect("write file");
        std::fs::create_dir_all(dir.path().join("subdir")).expect("create subdir");

        let ctx = make_context(dir.path());

        let ranged_refs = parse_workspace_references("@f.txt:10-20");
        let ranged = resolve_workspace_references(&ctx, &ranged_refs);
        assert!(render_item_block(&ranged.items[0]).contains("  range: 10-20\n"));

        let full_refs = parse_workspace_references("@full.txt");
        let full = resolve_workspace_references(&ctx, &full_refs);
        assert!(!render_item_block(&full.items[0]).contains("  range:"));

        let dir_refs = parse_workspace_references("@subdir");
        let dir_resolved = resolve_workspace_references(&ctx, &dir_refs);
        assert!(!render_item_block(&dir_resolved.items[0]).contains("  range:"));
    }

    #[test]
    fn resolve_range_beyond_eof_reports_no_content() {
        let dir = TempDir::new();
        std::fs::write(dir.path().join("short.txt"), "one\ntwo\nthree").expect("write file");

        let ctx = make_context(dir.path());
        let refs = parse_workspace_references("@short.txt:10-20");
        let resolved = resolve_workspace_references(&ctx, &refs);

        let item = &resolved.items[0];
        assert_eq!(item.kind, WorkspaceReferenceKind::File);
        assert!(item.content.contains("No content in requested range."));
    }

    #[test]
    fn resolve_hand_constructed_out_of_range_lines_never_panics() {
        let dir = TempDir::new();
        let ctx = make_context(dir.path());
        let refs = vec![WorkspaceReference {
            raw: "weird:range".to_string(),
            path: "does_not_exist.txt".to_string(),
            range: Some(WorkspaceReferenceRange::Lines {
                start_line: 0,
                end_line: usize::MAX,
            }),
        }];
        let resolved = resolve_workspace_references(&ctx, &refs);

        let item = &resolved.items[0];
        assert_eq!(item.kind, WorkspaceReferenceKind::Error);
        assert!(item.content.contains("invalid range"));
    }
}
