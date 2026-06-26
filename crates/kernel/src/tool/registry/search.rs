//! Workspace text-search module.
//!
//! Provides a literal, case-sensitive, recursive workspace search with bounded
//! output and deterministic ordering. No regex, no glob, no fuzzy matching.
//! Designed to be wired into the tool harness in T-2.

use std::ffi::OsStr;
use std::fs;
use std::path::Path;

use super::ToolError;

/// Maximum file size (in bytes) checked via `fs::metadata` before reading.
/// Files whose size is strictly greater than this value are skipped.
pub const SEARCH_MAX_FILE_BYTES: u64 = 256 * 1024;

/// Maximum number of matches returned. The walk stops as soon as one extra
/// match beyond this limit has been collected.
pub const SEARCH_MAX_MATCHES: usize = 50;

/// Maximum line length (in bytes) stored in a [`SearchMatch`]'s `text` field.
/// Lines longer than this are truncated at the nearest UTF-8 char boundary.
pub const SEARCH_MAX_LINE_CHARS: usize = 240;

/// Directory base-names that are never recursed into.
pub const SKIP_DIR_NAMES: &[&str] = &[".git", ".caravan", "target", "node_modules"];

/// A single line within a file that contains the search query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchMatch {
    /// Workspace-relative path to the file (never absolute).
    pub path: String,
    /// 1-based line number within the file.
    pub line: usize,
    /// Line content, truncated to [`SEARCH_MAX_LINE_CHARS`] bytes at the
    /// nearest valid UTF-8 char boundary.
    pub text: String,
}

/// The result of a [`search_workspace`] call.
pub struct SearchOutcome {
    /// Matches in deterministic walk order (sorted directory names, depth-first,
    /// ascending line number within each file).
    pub matches: Vec<SearchMatch>,
    /// `true` when the result was truncated to [`SEARCH_MAX_MATCHES`] because
    /// more matches existed in the workspace.
    pub truncated: bool,
}

/// Recursively searches `workspace_root` for files containing the literal
/// `query` string (case-sensitive, no regex).
///
/// # Errors
///
/// Returns [`ToolError::Io`] if `query` is empty, the workspace root cannot
/// be canonicalized, or the root directory cannot be read. Per-file failures
/// are silently skipped.
pub(super) fn search_workspace(
    workspace_root: &Path,
    query: &str,
) -> Result<SearchOutcome, ToolError> {
    if query.is_empty() {
        return Err(ToolError::Io {
            message: "search query must not be empty".to_string(),
        });
    }

    let canonical_root = fs::canonicalize(workspace_root).map_err(|e| ToolError::Io {
        message: format!("failed to canonicalize workspace root: {e}"),
    })?;

    // Verify root is readable before starting the walk.
    fs::read_dir(&canonical_root).map_err(|e| ToolError::Io {
        message: format!("failed to read workspace root: {e}"),
    })?;

    let mut matches: Vec<SearchMatch> = Vec::new();
    walk_dir(&canonical_root, &canonical_root, query, &mut matches);

    let truncated = matches.len() > SEARCH_MAX_MATCHES;
    if truncated {
        matches.truncate(SEARCH_MAX_MATCHES);
    }

    Ok(SearchOutcome { matches, truncated })
}

/// Recursively visits `dir`, appending matching lines to `out`.
///
/// Returns `true` when the collection limit (`SEARCH_MAX_MATCHES + 1`) has
/// been reached and the caller should stop immediately.
fn walk_dir(
    dir: &Path,
    canonical_root: &Path,
    query: &str,
    out: &mut Vec<SearchMatch>,
) -> bool {
    let read_dir = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return false,
    };

    // Collect all entries then sort by file name for determinism.
    let mut entries: Vec<_> = read_dir.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        if out.len() >= SEARCH_MAX_MATCHES + 1 {
            return true;
        }

        let entry_path = entry.path();

        // Non-following metadata (equivalent to lstat).
        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };

        if file_type.is_dir() {
            // Real directory: skip names in SKIP_DIR_NAMES, recurse into others.
            let name = entry.file_name();
            if SKIP_DIR_NAMES.iter().any(|s| OsStr::new(s) == name) {
                continue;
            }
            if walk_dir(&entry_path, canonical_root, query, out) {
                return true;
            }
        } else if file_type.is_file() {
            // Real file: confinement-check then search.
            if search_file(&entry_path, canonical_root, query, out) {
                return true;
            }
        } else if file_type.is_symlink() {
            // Resolve the symlink using following metadata.
            let following_meta = match fs::metadata(&entry_path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if following_meta.is_file() {
                // Symlinked file: confinement check determines if it is
                // inside the workspace before any read.
                if search_file(&entry_path, canonical_root, query, out) {
                    return true;
                }
            }
            // Symlink to a directory or other: skip — never recurse.
        }
    }

    out.len() >= SEARCH_MAX_MATCHES + 1
}

/// Applies the confinement check then searches a single file for `query`.
///
/// Returns `true` when the collection limit has been reached.
/// Any per-file failure silently skips the file (returns `false`).
fn search_file(
    path: &Path,
    canonical_root: &Path,
    query: &str,
    out: &mut Vec<SearchMatch>,
) -> bool {
    // Confinement check FIRST — no metadata or read before this.
    let canonical_file = match fs::canonicalize(path) {
        Ok(p) => p,
        Err(_) => return false,
    };
    if !canonical_file.starts_with(canonical_root) {
        return false;
    }

    // Size check: strictly greater than the limit is skipped.
    let len = match fs::metadata(path) {
        Ok(m) => m.len(),
        Err(_) => return false,
    };
    if len > SEARCH_MAX_FILE_BYTES {
        return false;
    }

    // Read bytes and validate UTF-8 strictly.
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(_) => return false,
    };
    let content = match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => return false,
    };

    // Workspace-relative path for all matches in this file.
    let rel_path = match canonical_file.strip_prefix(canonical_root) {
        Ok(p) => p.to_string_lossy().into_owned(),
        Err(_) => return false,
    };

    // Emit at most one match per line that contains the query.
    for (idx, line_str) in content.lines().enumerate() {
        if out.len() >= SEARCH_MAX_MATCHES + 1 {
            return true;
        }
        if line_str.contains(query) {
            let text = truncate_to_byte_boundary(line_str, SEARCH_MAX_LINE_CHARS);
            out.push(SearchMatch {
                path: rel_path.clone(),
                line: idx + 1,
                text,
            });
        }
    }

    out.len() >= SEARCH_MAX_MATCHES + 1
}

/// Truncates `s` to at most `max_bytes` bytes, walking back to the nearest
/// valid UTF-8 char boundary so the result is always a valid `&str`.
fn truncate_to_byte_boundary(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    // ── TempDir helper ────────────────────────────────────────────────────────

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new() -> Self {
            let count = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
            let name = format!("caravan_search_test_{}_{}", std::process::id(), count);
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

    // ── Tests ─────────────────────────────────────────────────────────────────

    #[test]
    fn search_simple_match() {
        let dir = TempDir::new();
        std::fs::write(dir.path().join("hello.txt"), "hello world\n").unwrap();

        let result = search_workspace(dir.path(), "hello").unwrap();
        assert_eq!(result.matches.len(), 1);
        assert_eq!(result.matches[0].text, "hello world");
        assert!(!result.truncated);
    }

    #[test]
    fn search_no_matches_returns_empty_ok() {
        let dir = TempDir::new();
        std::fs::write(dir.path().join("file.txt"), "nothing here\n").unwrap();

        let result = search_workspace(dir.path(), "ZZZNOMATCH").unwrap();
        assert!(result.matches.is_empty());
        assert!(!result.truncated);
    }

    #[test]
    fn search_correct_one_based_line_numbers() {
        let dir = TempDir::new();
        std::fs::write(
            dir.path().join("lines.txt"),
            "line one\nline two\nTARGET\nline four\n",
        )
        .unwrap();

        let result = search_workspace(dir.path(), "TARGET").unwrap();
        assert_eq!(result.matches.len(), 1);
        assert_eq!(result.matches[0].line, 3);
    }

    #[test]
    fn search_case_sensitive_foo_does_not_match_foo_lowercase() {
        let dir = TempDir::new();
        std::fs::write(dir.path().join("case.txt"), "foo bar\n").unwrap();

        let result = search_workspace(dir.path(), "Foo").unwrap();
        assert!(result.matches.is_empty());
    }

    #[test]
    fn search_no_regex_literal_dot_does_not_match_any_char() {
        let dir = TempDir::new();
        // "a.c" should match the literal string "a.c" but NOT "abc".
        std::fs::write(dir.path().join("regex.txt"), "abc\na.c\n").unwrap();

        let result = search_workspace(dir.path(), "a.c").unwrap();
        assert_eq!(result.matches.len(), 1);
        assert_eq!(result.matches[0].text, "a.c");
        assert_eq!(result.matches[0].line, 2);
    }

    #[test]
    fn search_one_match_per_line_even_with_multiple_occurrences() {
        let dir = TempDir::new();
        std::fs::write(dir.path().join("multi.txt"), "NEEDLE NEEDLE NEEDLE\n").unwrap();

        let result = search_workspace(dir.path(), "NEEDLE").unwrap();
        assert_eq!(result.matches.len(), 1);
    }

    #[test]
    fn search_deterministic_ordering_across_nested_sorted_directories() {
        let dir = TempDir::new();
        // Create directories "b_dir" and "a_dir" and a root file "c.txt".
        // Sorted order: a_dir, b_dir, c.txt → depth-first gives:
        //   a_dir/f.txt line 1, b_dir/f.txt line 1, c.txt line 1
        std::fs::create_dir(dir.path().join("b_dir")).unwrap();
        std::fs::create_dir(dir.path().join("a_dir")).unwrap();
        std::fs::write(dir.path().join("b_dir").join("f.txt"), "NEEDLE\n").unwrap();
        std::fs::write(dir.path().join("a_dir").join("f.txt"), "NEEDLE\n").unwrap();
        std::fs::write(dir.path().join("c.txt"), "NEEDLE\n").unwrap();

        let result = search_workspace(dir.path(), "NEEDLE").unwrap();
        assert_eq!(result.matches.len(), 3);

        let paths: Vec<&str> = result.matches.iter().map(|m| m.path.as_str()).collect();
        // All three should be present; order must be a_dir/f.txt, b_dir/f.txt, c.txt.
        assert!(paths[0].contains("a_dir"), "first: {}", paths[0]);
        assert!(paths[1].contains("b_dir"), "second: {}", paths[1]);
        assert!(paths[2].contains("c.txt"), "third: {}", paths[2]);
    }

    #[test]
    fn search_paths_are_workspace_relative_not_absolute() {
        let dir = TempDir::new();
        std::fs::write(dir.path().join("rel.txt"), "NEEDLE\n").unwrap();

        let result = search_workspace(dir.path(), "NEEDLE").unwrap();
        assert_eq!(result.matches.len(), 1);
        let path = &result.matches[0].path;
        assert!(
            !path.starts_with('/'),
            "path must be workspace-relative, got: {path}"
        );
        assert!(path.contains("rel.txt"));
    }

    #[test]
    fn search_bounded_truncation_more_than_max_gives_truncated_true() {
        let dir = TempDir::new();
        // Write a file with SEARCH_MAX_MATCHES + 1 matching lines.
        let content = "NEEDLE\n".repeat(SEARCH_MAX_MATCHES + 1);
        std::fs::write(dir.path().join("many.txt"), content).unwrap();

        let result = search_workspace(dir.path(), "NEEDLE").unwrap();
        assert_eq!(result.matches.len(), SEARCH_MAX_MATCHES);
        assert!(result.truncated);
    }

    #[test]
    fn search_bounded_truncation_exactly_max_gives_truncated_false() {
        let dir = TempDir::new();
        // Write a file with exactly SEARCH_MAX_MATCHES matching lines.
        let content = "NEEDLE\n".repeat(SEARCH_MAX_MATCHES);
        std::fs::write(dir.path().join("exact.txt"), content).unwrap();

        let result = search_workspace(dir.path(), "NEEDLE").unwrap();
        assert_eq!(result.matches.len(), SEARCH_MAX_MATCHES);
        assert!(!result.truncated);
    }

    #[test]
    fn search_non_utf8_file_is_skipped() {
        let dir = TempDir::new();
        // Valid UTF-8 file alongside an invalid one.
        let invalid: &[u8] = &[0xFF, 0xFE, 0xFF, 0x4E, 0x45, 0x45, 0x44, 0x4C, 0x45];
        std::fs::write(dir.path().join("bad.bin"), invalid).unwrap();
        std::fs::write(dir.path().join("good.txt"), "NEEDLE\n").unwrap();

        let result = search_workspace(dir.path(), "NEEDLE").unwrap();
        // Only the valid UTF-8 file contributes a match.
        assert_eq!(result.matches.len(), 1);
        assert!(result.matches[0].path.contains("good.txt"));
    }

    #[test]
    fn search_file_larger_than_max_bytes_is_skipped() {
        let dir = TempDir::new();
        // File strictly larger than SEARCH_MAX_FILE_BYTES → skipped.
        let oversized = "NEEDLE".as_bytes().iter()
            .chain(b" ".repeat(SEARCH_MAX_FILE_BYTES as usize - 5).iter())
            .copied()
            .collect::<Vec<u8>>();
        assert_eq!(oversized.len() as u64, SEARCH_MAX_FILE_BYTES + 1);
        std::fs::write(dir.path().join("big.txt"), &oversized).unwrap();

        let result = search_workspace(dir.path(), "NEEDLE").unwrap();
        assert!(
            result.matches.is_empty(),
            "oversized file should be skipped"
        );
    }

    #[test]
    fn search_file_exactly_max_bytes_is_searched() {
        let dir = TempDir::new();
        // File of exactly SEARCH_MAX_FILE_BYTES → NOT skipped (strictly-greater guard).
        let mut content = "NEEDLE".to_string();
        content.extend(std::iter::repeat(' ').take(SEARCH_MAX_FILE_BYTES as usize - 6));
        assert_eq!(content.len() as u64, SEARCH_MAX_FILE_BYTES);
        std::fs::write(dir.path().join("exact.txt"), &content).unwrap();

        let result = search_workspace(dir.path(), "NEEDLE").unwrap();
        assert_eq!(
            result.matches.len(),
            1,
            "file of exactly SEARCH_MAX_FILE_BYTES must be searched"
        );
    }

    #[test]
    fn search_long_line_truncated_at_utf8_char_boundary() {
        let dir = TempDir::new();
        // Build a line where a naive s[..SEARCH_MAX_LINE_CHARS] would panic:
        //   79 × "あ" (237 bytes) + "ab" (2 bytes) + "あ" (3 bytes) = 242 bytes prefix
        //   then " NEEDLE" (7 bytes) → total 249 bytes.
        // Truncating at byte 240 lands inside the 3-byte "あ" at bytes 239-241,
        // which is NOT a char boundary, so we walk back to 239 = "あ"×79+"ab".
        let prefix = "あ".repeat(79) + "ab" + "あ";
        assert_eq!(prefix.len(), 242);
        let line = format!("{prefix} NEEDLE");
        std::fs::write(dir.path().join("long.txt"), &line).unwrap();

        let result = search_workspace(dir.path(), "NEEDLE").unwrap();
        assert_eq!(result.matches.len(), 1);

        let text = &result.matches[0].text;
        // Must be valid UTF-8 (no panic on creation) and shorter than the original.
        assert!(text.len() <= SEARCH_MAX_LINE_CHARS, "text.len()={}", text.len());
        // Exact expected value: 79 "あ" + "ab"
        let expected = "あ".repeat(79) + "ab";
        assert_eq!(text, &expected);
    }

    #[test]
    fn search_empty_query_returns_err() {
        let dir = TempDir::new();
        let result = search_workspace(dir.path(), "");
        assert!(matches!(result, Err(ToolError::Io { .. })));
    }

    #[test]
    fn search_skip_directories_git_caravan_target_node_modules() {
        let dir = TempDir::new();
        for skip_name in SKIP_DIR_NAMES {
            let subdir = dir.path().join(skip_name);
            std::fs::create_dir(&subdir).unwrap();
            std::fs::write(subdir.join("secret.txt"), "NEEDLE\n").unwrap();
        }
        // Place a matching file in the root so we know search is working.
        std::fs::write(dir.path().join("ok.txt"), "NEEDLE\n").unwrap();

        let result = search_workspace(dir.path(), "NEEDLE").unwrap();
        // Only ok.txt should contribute a match.
        assert_eq!(result.matches.len(), 1);
        assert!(result.matches[0].path.contains("ok.txt"));
    }

    #[cfg(unix)]
    #[test]
    fn search_symlinked_directory_outside_workspace_not_traversed() {
        let workspace = TempDir::new();
        let outside = TempDir::new();
        // Put a matching file in the outside directory.
        std::fs::write(outside.path().join("secret.txt"), "NEEDLE\n").unwrap();
        // Symlink inside workspace → outside directory.
        std::os::unix::fs::symlink(outside.path(), workspace.path().join("escape_dir")).unwrap();
        // Put a matching file in the workspace root to confirm search runs.
        std::fs::write(workspace.path().join("ok.txt"), "NEEDLE\n").unwrap();

        let result = search_workspace(workspace.path(), "NEEDLE").unwrap();
        // Only ok.txt; the symlinked directory must not be traversed.
        assert_eq!(result.matches.len(), 1);
        assert!(
            result.matches[0].path.contains("ok.txt"),
            "unexpected match: {:?}",
            result.matches
        );
    }

    #[cfg(unix)]
    #[test]
    fn search_symlinked_file_outside_workspace_not_read() {
        let workspace = TempDir::new();
        let outside = TempDir::new();
        // Put a matching file outside the workspace.
        std::fs::write(outside.path().join("secret.txt"), "NEEDLE\n").unwrap();
        // Symlink inside workspace → file outside.
        std::os::unix::fs::symlink(
            outside.path().join("secret.txt"),
            workspace.path().join("escape_file.txt"),
        )
        .unwrap();
        // Put a non-matching file in workspace so we know search ran.
        std::fs::write(workspace.path().join("ok.txt"), "no match here\n").unwrap();

        let result = search_workspace(workspace.path(), "NEEDLE").unwrap();
        // The symlinked file outside the workspace must not be read.
        assert!(
            result.matches.is_empty(),
            "symlinked file outside workspace must not be read: {:?}",
            result.matches
        );
    }

    #[cfg(unix)]
    #[test]
    fn search_symlinked_file_inside_workspace_is_searched() {
        let workspace = TempDir::new();
        // Place real file inside workspace.
        std::fs::write(workspace.path().join("real.txt"), "NEEDLE\n").unwrap();
        // Symlink to that real file, also inside the workspace.
        std::os::unix::fs::symlink(
            workspace.path().join("real.txt"),
            workspace.path().join("link.txt"),
        )
        .unwrap();

        let result = search_workspace(workspace.path(), "NEEDLE").unwrap();
        // Both the real file and the intra-workspace symlink may appear.
        assert!(
            !result.matches.is_empty(),
            "at least one match expected (real file)"
        );
        // All paths must be workspace-relative.
        for m in &result.matches {
            assert!(
                !m.path.starts_with('/'),
                "path must be relative, got: {}",
                m.path
            );
        }
    }
}
