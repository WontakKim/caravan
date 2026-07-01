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
    /// The exact text consumed after `@` (used for display as `@{raw}`).
    pub raw: String,
    /// The path passed to the tool harness for resolution.
    pub path: String,
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

/// Extracts `@path` reference tokens from plain-text `input`.
///
/// Performs no filesystem I/O and never panics on any input, including
/// malformed UTF-8 boundaries (handled transparently since `input` is
/// already a valid `&str`).
pub fn parse_workspace_references(input: &str) -> Vec<WorkspaceReference> {
    let chars: Vec<(usize, char)> = input.char_indices().collect();
    let len = chars.len();

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

            let start_byte = if i + 1 < len {
                chars[i + 1].0
            } else {
                input.len()
            };
            let end_byte = if j < len { chars[j].0 } else { input.len() };
            let token = &input[start_byte..end_byte];

            if !token.is_empty()
                && token.len() <= WORKSPACE_REFERENCE_MAX_PATH_CHARS
                && !token.chars().all(|c| c == '.' || c == '/')
                && seen.insert(token.to_string())
            {
                result.push(WorkspaceReference {
                    raw: token.to_string(),
                    path: token.to_string(),
                });
            }

            // Advance past the whole attempted token (or just past `@` when
            // nothing qualifying followed it), tracking `prev` accordingly so
            // a subsequent `@` sees the correct boundary character.
            prev = Some(if j > i + 1 { chars[j - 1].1 } else { ch });
            i = j.max(i + 1);
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

/// Resolves one [`WorkspaceReference`] against the tool harness.
fn resolve_reference(
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

/// Renders the exact per-item block [`WorkspaceReferences::render_prompt_section`]
/// emits for one resolved reference; used both for the final rendering and to
/// measure the aggregate byte budget.
fn render_item_block(item: &ResolvedWorkspaceReference) -> String {
    format!(
        "Source:\n  reference: @{}\n  kind: {}\n  risk: read_only\n\nContent:\n{}",
        item.reference.raw,
        item.kind.as_str(),
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
    fn parse_deduplicates_by_path_keeping_first_occurrence() {
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
}
