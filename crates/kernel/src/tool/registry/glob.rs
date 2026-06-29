//! Workspace glob matcher and file-discovery primitive.
//!
//! Provides a pure, deterministic, workspace-confined glob discovery function
//! plus supporting helpers. No coupling to `ToolRequest`/`ToolOutput` yet —
//! that lands in T-2.

use std::ffi::OsStr;
use std::fs;
use std::path::Path;

use super::ToolError;
use super::search::SKIP_DIR_NAMES;

/// Maximum number of matching file paths returned by [`glob_workspace`].
pub(crate) const GLOB_MAX_MATCHES: usize = 200;

/// Splits a glob pattern by `/` into segments, collapsing runs of consecutive
/// `**` into a single `**` to prevent exponential backtracking on adversarial
/// patterns such as `**/**/**/**`.
fn split_pattern_segments(pattern: &str) -> Vec<&str> {
    let mut result: Vec<&str> = Vec::new();
    for seg in pattern.split('/') {
        if seg == "**" && result.last() == Some(&"**") {
            continue;
        }
        result.push(seg);
    }
    result
}

/// Matches a single glob pattern segment against a single path segment.
///
/// Within a segment: `*` matches zero or more characters; `?` matches exactly
/// one character. `**` within a segment behaves like `*` — globstar semantics
/// apply only when `**` is a whole path segment (handled by [`matches_path`]).
fn matches_segment(pat: &str, seg: &str) -> bool {
    let pat_chars: Vec<char> = pat.chars().collect();
    let seg_chars: Vec<char> = seg.chars().collect();
    match_segment_chars(&pat_chars, &seg_chars)
}

fn match_segment_chars(pat: &[char], seg: &[char]) -> bool {
    match pat.first() {
        None => seg.is_empty(),
        Some('*') => {
            // `*` (and `**` within a segment) matches zero or more chars.
            match_segment_chars(&pat[1..], seg)
                || (!seg.is_empty() && match_segment_chars(pat, &seg[1..]))
        }
        Some('?') => !seg.is_empty() && match_segment_chars(&pat[1..], &seg[1..]),
        Some(p) => seg.first() == Some(p) && match_segment_chars(&pat[1..], &seg[1..]),
    }
}

/// Matches a sequence of glob pattern segments against a sequence of path segments.
///
/// Algorithm (recursive backtracking; terminates because each branch strictly
/// shrinks pattern or path):
/// - Empty pattern matches only empty path.
/// - A `**` segment matches zero or more path segments (globstar).
/// - Any other segment must match the first path segment via [`matches_segment`].
fn matches_path(pat: &[&str], path: &[&str]) -> bool {
    match pat.first() {
        None => path.is_empty(),
        Some(&"**") => {
            matches_path(&pat[1..], path) || (!path.is_empty() && matches_path(pat, &path[1..]))
        }
        Some(p) => {
            !path.is_empty() && matches_segment(p, path[0]) && matches_path(&pat[1..], &path[1..])
        }
    }
}

/// Validates a glob pattern before any filesystem operation.
///
/// # Errors
/// - [`ToolError::InvalidPattern`] for empty or whitespace-only patterns.
/// - [`ToolError::WorkspaceViolation`] for patterns starting with `/` (absolute)
///   or containing a `..` path segment.
fn validate_glob_pattern(pattern: &str) -> Result<(), ToolError> {
    if pattern.trim().is_empty() {
        return Err(ToolError::InvalidPattern {
            pattern: pattern.to_string(),
        });
    }
    if pattern.starts_with('/') {
        return Err(ToolError::WorkspaceViolation {
            path: pattern.to_string(),
        });
    }
    for seg in pattern.split('/') {
        if seg == ".." {
            return Err(ToolError::WorkspaceViolation {
                path: pattern.to_string(),
            });
        }
    }
    Ok(())
}

/// The outcome of a [`glob_workspace`] call.
#[derive(Debug)]
pub(crate) struct GlobMatchOutcome {
    /// Workspace-relative file paths (never directories) using `/` separators,
    /// in deterministic sorted (depth-first, name-sorted) order.
    pub paths: Vec<String>,
    /// `true` when results were truncated to [`GLOB_MAX_MATCHES`].
    pub truncated: bool,
}

/// Walks `workspace_root` recursively and returns all workspace-relative file
/// paths whose `/`-separated segment sequence matches the glob `pattern`.
///
/// # Security
/// - The pattern is validated before any filesystem access.
/// - Symlinked directories are never followed.
/// - Symlinked files are included only when their canonicalized path stays
///   inside the canonicalized workspace root.
///
/// # Errors
/// - [`ToolError::InvalidPattern`] for empty or whitespace-only patterns.
/// - [`ToolError::WorkspaceViolation`] for patterns starting with `/` or containing `..`.
/// - [`ToolError::Io`] if the workspace root cannot be canonicalized or read.
pub(crate) fn glob_workspace(
    workspace_root: &Path,
    pattern: &str,
) -> Result<GlobMatchOutcome, ToolError> {
    validate_glob_pattern(pattern)?;

    let canonical_root = fs::canonicalize(workspace_root).map_err(|e| ToolError::Io {
        message: format!("failed to canonicalize workspace root: {e}"),
    })?;

    let pat_segments = split_pattern_segments(pattern);

    let mut paths: Vec<String> = Vec::new();
    walk_dir(
        &canonical_root,
        &canonical_root,
        &[],
        &pat_segments,
        &mut paths,
    );

    let truncated = paths.len() > GLOB_MAX_MATCHES;
    if truncated {
        paths.truncate(GLOB_MAX_MATCHES);
    }

    Ok(GlobMatchOutcome { paths, truncated })
}

/// Recursively walks `dir`, appending matching workspace-relative file paths to `out`.
///
/// `rel_segments` holds the path components from the workspace root to `dir`.
/// Returns `true` once `GLOB_MAX_MATCHES + 1` paths have been collected, signalling
/// the caller to stop early.
fn walk_dir(
    dir: &Path,
    canonical_root: &Path,
    rel_segments: &[String],
    pat_segments: &[&str],
    out: &mut Vec<String>,
) -> bool {
    let read_dir = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return false,
    };

    let mut entries: Vec<_> = read_dir.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in &entries {
        if out.len() >= GLOB_MAX_MATCHES + 1 {
            return true;
        }

        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };

        let os_name = entry.file_name();

        if file_type.is_dir() {
            // Skip directories in SKIP_DIR_NAMES; symlinked dirs are never followed
            // because file_type() uses lstat semantics and won't report is_dir() for symlinks.
            if SKIP_DIR_NAMES.iter().any(|s| OsStr::new(s) == os_name) {
                continue;
            }
            let name_str = os_name.to_string_lossy().into_owned();
            let mut child_segments = rel_segments.to_vec();
            child_segments.push(name_str);
            if walk_dir(
                &entry.path(),
                canonical_root,
                &child_segments,
                pat_segments,
                out,
            ) {
                return true;
            }
        } else if file_type.is_file() {
            let name_str = os_name.to_string_lossy().into_owned();
            let mut file_segments = rel_segments.to_vec();
            file_segments.push(name_str);
            let seg_refs: Vec<&str> = file_segments.iter().map(String::as_str).collect();
            if matches_path(pat_segments, &seg_refs) {
                out.push(file_segments.join("/"));
            }
        } else if file_type.is_symlink() {
            // Canonicalize to follow the symlink and confirm it stays inside the workspace.
            let canonical_file = match fs::canonicalize(entry.path()) {
                Ok(p) => p,
                Err(_) => continue,
            };
            if !canonical_file.starts_with(canonical_root) {
                continue;
            }
            // Exclude symlinks to directories (never recurse through them).
            let meta = match fs::metadata(&canonical_file) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if !meta.is_file() {
                continue;
            }
            let name_str = os_name.to_string_lossy().into_owned();
            let mut file_segments = rel_segments.to_vec();
            file_segments.push(name_str);
            let seg_refs: Vec<&str> = file_segments.iter().map(String::as_str).collect();
            if matches_path(pat_segments, &seg_refs) {
                out.push(file_segments.join("/"));
            }
        }
    }

    out.len() >= GLOB_MAX_MATCHES + 1
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
            let name = format!("caravan_glob_test_{}_{}", std::process::id(), count);
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

    // ── matches_segment tests ─────────────────────────────────────────────────

    #[test]
    fn matches_segment_star_matches_empty_string() {
        assert!(matches_segment("*", ""));
    }

    #[test]
    fn matches_segment_star_matches_single_char() {
        assert!(matches_segment("*", "a"));
    }

    #[test]
    fn matches_segment_star_matches_multiple_chars() {
        assert!(matches_segment("*", "hello"));
    }

    #[test]
    fn matches_segment_star_suffix_pattern_matches_extension() {
        assert!(matches_segment("*.rs", "foo.rs"));
        assert!(matches_segment("*.rs", ".rs"));
        assert!(!matches_segment("*.rs", "foo.go"));
    }

    #[test]
    fn matches_segment_star_prefix_pattern() {
        assert!(matches_segment("foo*", "foobar"));
        assert!(matches_segment("foo*", "foo"));
        assert!(!matches_segment("foo*", "barfoo"));
    }

    #[test]
    fn matches_segment_double_star_within_segment_acts_like_star() {
        // Within a segment, ** behaves like *.
        assert!(matches_segment("foo**bar", "fooXYZbar"));
        assert!(matches_segment("foo**bar", "foobar"));
        assert!(!matches_segment("foo**bar", "fooXYZ"));
    }

    #[test]
    fn matches_segment_question_matches_exactly_one_char() {
        assert!(matches_segment("?", "a"));
        assert!(!matches_segment("?", ""));
        assert!(!matches_segment("?", "ab"));
    }

    #[test]
    fn matches_segment_question_in_pattern() {
        assert!(matches_segment("fo?", "foo"));
        assert!(matches_segment("fo?", "fob"));
        assert!(!matches_segment("fo?", "fo"));
        assert!(!matches_segment("fo?", "foab"));
    }

    #[test]
    fn matches_segment_literal_match() {
        assert!(matches_segment("main.rs", "main.rs"));
        assert!(!matches_segment("main.rs", "main.go"));
    }

    #[test]
    fn matches_segment_empty_pattern_matches_only_empty_segment() {
        assert!(matches_segment("", ""));
        assert!(!matches_segment("", "a"));
    }

    // ── matches_path tests ────────────────────────────────────────────────────

    #[test]
    fn matches_path_root_level_star_rs_matches_single_segment() {
        assert!(matches_path(&["*.rs"], &["main.rs"]));
        assert!(matches_path(&["*.rs"], &["lib.rs"]));
    }

    #[test]
    fn matches_path_root_level_star_rs_does_not_match_nested() {
        // *.rs should not match a/b.rs because it is one segment deep.
        assert!(!matches_path(&["*.rs"], &["a", "b.rs"]));
    }

    #[test]
    fn matches_path_globstar_slash_star_rs_matches_any_depth() {
        assert!(matches_path(&["**", "*.rs"], &["main.rs"]));
        assert!(matches_path(&["**", "*.rs"], &["src", "main.rs"]));
        assert!(matches_path(&["**", "*.rs"], &["a", "b", "c.rs"]));
        assert!(!matches_path(&["**", "*.rs"], &["a", "b.go"]));
    }

    #[test]
    fn matches_path_nested_crates_globstar_tests_rs() {
        assert!(matches_path(
            &["crates", "**", "tests.rs"],
            &["crates", "kernel", "tests.rs"]
        ));
        assert!(matches_path(
            &["crates", "**", "tests.rs"],
            &["crates", "a", "b", "tests.rs"]
        ));
        // Must not match a different root.
        assert!(!matches_path(
            &["crates", "**", "tests.rs"],
            &["src", "kernel", "tests.rs"]
        ));
    }

    #[test]
    fn matches_path_leading_globstar_matches_anywhere() {
        assert!(matches_path(&["**", "foo.rs"], &["foo.rs"]));
        assert!(matches_path(&["**", "foo.rs"], &["a", "foo.rs"]));
        assert!(matches_path(&["**", "foo.rs"], &["a", "b", "c", "foo.rs"]));
        assert!(!matches_path(&["**", "foo.rs"], &["a", "bar.rs"]));
    }

    #[test]
    fn matches_path_trailing_globstar_matches_any_file_under_dir() {
        assert!(matches_path(&["src", "**"], &["src", "main.rs"]));
        assert!(matches_path(&["src", "**"], &["src", "a", "b.rs"]));
        // Must not match the directory itself (empty remainder).
        assert!(matches_path(&["src", "**"], &["src"]));
        assert!(!matches_path(&["src", "**"], &["lib", "main.rs"]));
    }

    #[test]
    fn matches_path_adjacent_globstar_equivalent_to_single_globstar() {
        let paths: &[&[&str]] = &[&[], &["a"], &["a", "b"], &["a", "b", "c"], &["foo.rs"]];
        for path in paths {
            let single = matches_path(&["**"], path);
            let double = matches_path(&["**", "**"], path);
            assert_eq!(
                single, double,
                "mismatch for path {:?}: single={} double={}",
                path, single, double
            );
        }
    }

    #[test]
    fn matches_path_empty_pattern_matches_only_empty_path() {
        assert!(matches_path(&[], &[]));
        assert!(!matches_path(&[], &["a"]));
    }

    // ── validate_glob_pattern / error-variant tests ───────────────────────────

    #[test]
    fn validate_glob_pattern_empty_string_returns_invalid_pattern() {
        let result = validate_glob_pattern("");
        assert!(
            matches!(result, Err(ToolError::InvalidPattern { .. })),
            "expected InvalidPattern, got: {result:?}"
        );
    }

    #[test]
    fn validate_glob_pattern_whitespace_only_returns_invalid_pattern() {
        for ws in &["   ", "\t", "\n", "  \t  "] {
            let result = validate_glob_pattern(ws);
            assert!(
                matches!(result, Err(ToolError::InvalidPattern { .. })),
                "expected InvalidPattern for {ws:?}, got: {result:?}"
            );
        }
    }

    #[test]
    fn validate_glob_pattern_absolute_prefix_returns_workspace_violation() {
        let result = validate_glob_pattern("/etc/passwd");
        assert!(
            matches!(result, Err(ToolError::WorkspaceViolation { .. })),
            "expected WorkspaceViolation, got: {result:?}"
        );
    }

    #[test]
    fn validate_glob_pattern_dotdot_segment_returns_workspace_violation() {
        for pat in &["../escape", "foo/../bar", "a/b/../c"] {
            let result = validate_glob_pattern(pat);
            assert!(
                matches!(result, Err(ToolError::WorkspaceViolation { .. })),
                "expected WorkspaceViolation for {pat:?}, got: {result:?}"
            );
        }
    }

    #[test]
    fn validate_glob_pattern_valid_patterns_return_ok() {
        for pat in &["*.rs", "**/*.rs", "src/**/tests.rs", "a/b/c.txt"] {
            assert!(
                validate_glob_pattern(pat).is_ok(),
                "expected Ok for {pat:?}"
            );
        }
    }

    // ── glob_workspace tests ──────────────────────────────────────────────────

    #[test]
    fn glob_workspace_simple_match() {
        let dir = TempDir::new();
        std::fs::write(dir.path().join("hello.rs"), "fn main() {}").unwrap();
        std::fs::write(dir.path().join("hello.txt"), "not rust").unwrap();

        let outcome = glob_workspace(dir.path(), "*.rs").unwrap();
        assert_eq!(outcome.paths, vec!["hello.rs"]);
        assert!(!outcome.truncated);
    }

    #[test]
    fn glob_workspace_nested_match() {
        let dir = TempDir::new();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src").join("lib.rs"), "").unwrap();
        std::fs::write(dir.path().join("src").join("main.rs"), "").unwrap();
        std::fs::write(dir.path().join("README.md"), "").unwrap();

        let outcome = glob_workspace(dir.path(), "**/*.rs").unwrap();
        assert_eq!(outcome.paths, vec!["src/lib.rs", "src/main.rs"]);
        assert!(!outcome.truncated);
    }

    #[test]
    fn glob_workspace_no_match_returns_empty_and_not_truncated() {
        let dir = TempDir::new();
        std::fs::write(dir.path().join("file.txt"), "content").unwrap();

        let outcome = glob_workspace(dir.path(), "*.rs").unwrap();
        assert!(outcome.paths.is_empty());
        assert!(!outcome.truncated);
    }

    #[test]
    fn glob_workspace_deterministic_sorted_order() {
        let dir = TempDir::new();
        // Create files in reverse alphabetical order; walk must produce sorted output.
        std::fs::write(dir.path().join("c.txt"), "").unwrap();
        std::fs::write(dir.path().join("a.txt"), "").unwrap();
        std::fs::write(dir.path().join("b.txt"), "").unwrap();

        let outcome = glob_workspace(dir.path(), "*.txt").unwrap();
        assert_eq!(outcome.paths, vec!["a.txt", "b.txt", "c.txt"]);
        assert!(!outcome.truncated);
    }

    #[test]
    fn glob_workspace_truncation_past_max_sets_truncated_true() {
        let dir = TempDir::new();
        // Create GLOB_MAX_MATCHES + 1 files — one more than the cap.
        for i in 0..=GLOB_MAX_MATCHES {
            std::fs::write(dir.path().join(format!("f{:04}.txt", i)), "").unwrap();
        }

        let outcome = glob_workspace(dir.path(), "*.txt").unwrap();
        assert_eq!(outcome.paths.len(), GLOB_MAX_MATCHES);
        assert!(outcome.truncated);
    }

    #[test]
    fn glob_workspace_exactly_max_matches_not_truncated() {
        let dir = TempDir::new();
        for i in 0..GLOB_MAX_MATCHES {
            std::fs::write(dir.path().join(format!("f{:04}.txt", i)), "").unwrap();
        }

        let outcome = glob_workspace(dir.path(), "*.txt").unwrap();
        assert_eq!(outcome.paths.len(), GLOB_MAX_MATCHES);
        assert!(!outcome.truncated);
    }

    #[test]
    fn glob_workspace_skips_git_caravan_target_node_modules() {
        let dir = TempDir::new();
        for skip_name in SKIP_DIR_NAMES {
            let subdir = dir.path().join(skip_name);
            std::fs::create_dir(&subdir).unwrap();
            std::fs::write(subdir.join("hidden.rs"), "").unwrap();
        }
        // Place a matching file outside any skip dir.
        std::fs::write(dir.path().join("visible.rs"), "").unwrap();

        let outcome = glob_workspace(dir.path(), "**/*.rs").unwrap();
        assert_eq!(outcome.paths, vec!["visible.rs"]);
    }

    #[test]
    fn glob_workspace_empty_pattern_returns_invalid_pattern() {
        let dir = TempDir::new();
        let result = glob_workspace(dir.path(), "");
        assert!(
            matches!(result, Err(ToolError::InvalidPattern { .. })),
            "expected InvalidPattern, got: {result:?}"
        );
    }

    #[test]
    fn glob_workspace_whitespace_pattern_returns_invalid_pattern() {
        let dir = TempDir::new();
        let result = glob_workspace(dir.path(), "   ");
        assert!(
            matches!(result, Err(ToolError::InvalidPattern { .. })),
            "expected InvalidPattern for whitespace pattern, got: {result:?}"
        );
    }

    #[test]
    fn glob_workspace_absolute_pattern_returns_workspace_violation() {
        let dir = TempDir::new();
        let result = glob_workspace(dir.path(), "/etc/passwd");
        assert!(
            matches!(result, Err(ToolError::WorkspaceViolation { .. })),
            "expected WorkspaceViolation, got: {result:?}"
        );
    }

    #[test]
    fn glob_workspace_dotdot_pattern_returns_workspace_violation() {
        let dir = TempDir::new();
        let result = glob_workspace(dir.path(), "../escape");
        assert!(
            matches!(result, Err(ToolError::WorkspaceViolation { .. })),
            "expected WorkspaceViolation, got: {result:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn glob_workspace_symlinked_file_outside_workspace_is_excluded() {
        let workspace = TempDir::new();
        let outside = TempDir::new();
        std::fs::write(outside.path().join("secret.rs"), "secret").unwrap();
        // Symlink inside workspace → file outside workspace.
        std::os::unix::fs::symlink(
            outside.path().join("secret.rs"),
            workspace.path().join("escape.rs"),
        )
        .unwrap();
        // A real file inside the workspace that should match.
        std::fs::write(workspace.path().join("real.rs"), "").unwrap();

        let outcome = glob_workspace(workspace.path(), "*.rs").unwrap();
        assert_eq!(
            outcome.paths,
            vec!["real.rs"],
            "symlinked file outside workspace must be excluded; got: {:?}",
            outcome.paths
        );
    }

    #[cfg(unix)]
    #[test]
    fn glob_workspace_symlinked_directory_outside_workspace_not_traversed() {
        let workspace = TempDir::new();
        let outside = TempDir::new();
        std::fs::write(outside.path().join("secret.rs"), "secret").unwrap();
        // Symlink inside workspace → directory outside.
        std::os::unix::fs::symlink(outside.path(), workspace.path().join("escape_dir")).unwrap();
        std::fs::write(workspace.path().join("ok.rs"), "").unwrap();

        let outcome = glob_workspace(workspace.path(), "**/*.rs").unwrap();
        assert_eq!(outcome.paths, vec!["ok.rs"]);
    }

    #[cfg(unix)]
    #[test]
    fn glob_workspace_symlinked_file_inside_workspace_is_included() {
        let workspace = TempDir::new();
        std::fs::write(workspace.path().join("real.rs"), "fn main() {}").unwrap();
        std::os::unix::fs::symlink(
            workspace.path().join("real.rs"),
            workspace.path().join("link.rs"),
        )
        .unwrap();

        let outcome = glob_workspace(workspace.path(), "*.rs").unwrap();
        // Both real.rs and link.rs are valid; at least one must appear.
        assert!(
            !outcome.paths.is_empty(),
            "expected at least one matching .rs file"
        );
        for path in &outcome.paths {
            assert!(
                !path.starts_with('/'),
                "path must be workspace-relative: {path}"
            );
        }
    }
}
