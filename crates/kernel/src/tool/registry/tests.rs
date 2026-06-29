use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::*;

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
        let name = format!("caravan_tools_test_{}_{}", std::process::id(), count);
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

#[test]
fn tool_registry_new_readonly_constructs() {
    let registry = ToolRegistry::new_readonly();
    assert_eq!(registry, ToolRegistry);
}

#[test]
fn max_read_file_bytes_is_64_kib() {
    assert_eq!(MAX_READ_FILE_BYTES, 64 * 1024);
}

#[test]
fn list_files_returns_sorted_entries() {
    let dir = TempDir::new();
    std::fs::create_dir_all(dir.path().join("subdir")).expect("create subdir");
    std::fs::write(dir.path().join("beta.txt"), "b").expect("write beta");
    std::fs::write(dir.path().join("alpha.txt"), "a").expect("write alpha");
    std::fs::write(dir.path().join("gamma.txt"), "g").expect("write gamma");

    let registry = ToolRegistry::new_readonly();
    let ctx = make_context(dir.path());
    let output = registry.execute(
        &ctx,
        ToolRequest::ListFiles {
            path: ".".to_string(),
        },
    );

    let ToolOutput::FileList { entries, .. } = output.expect("should succeed") else {
        panic!("expected FileList");
    };
    assert_eq!(
        entries,
        vec!["alpha.txt", "beta.txt", "gamma.txt", "subdir"]
    );
}

#[test]
fn list_files_on_file_returns_not_a_directory() {
    let dir = TempDir::new();
    std::fs::write(dir.path().join("a_file.txt"), "hello").expect("write file");

    let registry = ToolRegistry::new_readonly();
    let ctx = make_context(dir.path());
    let result = registry.execute(
        &ctx,
        ToolRequest::ListFiles {
            path: "a_file.txt".to_string(),
        },
    );

    assert!(matches!(result, Err(ToolError::NotADirectory { .. })));
}

#[test]
fn list_files_dot_lists_workspace_root() {
    let dir = TempDir::new();
    std::fs::write(dir.path().join("hello.txt"), "hi").expect("write file");

    let registry = ToolRegistry::new_readonly();
    let ctx = make_context(dir.path());
    let output = registry.execute(
        &ctx,
        ToolRequest::ListFiles {
            path: ".".to_string(),
        },
    );

    let ToolOutput::FileList { path, entries } = output.expect("should succeed") else {
        panic!("expected FileList");
    };
    assert_eq!(path, ".");
    assert!(entries.contains(&"hello.txt".to_string()));
}

#[test]
fn list_files_absolute_path_is_workspace_violation() {
    let dir = TempDir::new();
    let registry = ToolRegistry::new_readonly();
    let ctx = make_context(dir.path());
    let result = registry.execute(
        &ctx,
        ToolRequest::ListFiles {
            path: "/etc".to_string(),
        },
    );

    assert!(matches!(result, Err(ToolError::WorkspaceViolation { .. })));
}

#[test]
fn list_files_parent_escape_is_workspace_violation() {
    let dir = TempDir::new();
    let registry = ToolRegistry::new_readonly();
    let ctx = make_context(dir.path());
    let result = registry.execute(
        &ctx,
        ToolRequest::ListFiles {
            path: "../escape".to_string(),
        },
    );

    assert!(matches!(result, Err(ToolError::WorkspaceViolation { .. })));
}

#[cfg(unix)]
#[test]
fn list_files_symlink_escape_is_workspace_violation() {
    let dir = TempDir::new();
    let outside = TempDir::new();
    // Create a symlink inside the workspace pointing to a directory outside.
    let link_path = dir.path().join("escape_link");
    std::os::unix::fs::symlink(outside.path(), &link_path).expect("failed to create symlink");

    let registry = ToolRegistry::new_readonly();
    let ctx = make_context(dir.path());
    let result = registry.execute(
        &ctx,
        ToolRequest::ListFiles {
            path: "escape_link".to_string(),
        },
    );

    assert!(matches!(result, Err(ToolError::WorkspaceViolation { .. })));
}

// ─── Full-read regression tests ──────────────────────────────────────────────

#[test]
fn read_file_returns_utf8_content() {
    let dir = TempDir::new();
    std::fs::write(dir.path().join("hello.txt"), "hello, world!").expect("write file");

    let registry = ToolRegistry::new_readonly();
    let ctx = make_context(dir.path());
    let result = registry.execute(
        &ctx,
        ToolRequest::ReadFile {
            path: "hello.txt".to_string(),
            offset: None,
            limit: None,
        },
    );

    let output = result.expect("should succeed");
    match &output {
        ToolOutput::FileContent {
            path,
            content,
            start_line,
            line_count,
            truncated,
        } => {
            assert_eq!(path, "hello.txt");
            assert_eq!(content, "hello, world!");
            assert_eq!(*start_line, None);
            assert_eq!(*line_count, None);
            assert!(!truncated);
        }
        _ => panic!("expected FileContent"),
    }
}

#[test]
fn read_file_on_directory_returns_not_a_file() {
    let dir = TempDir::new();
    std::fs::create_dir_all(dir.path().join("subdir")).expect("create subdir");

    let registry = ToolRegistry::new_readonly();
    let ctx = make_context(dir.path());
    let result = registry.execute(
        &ctx,
        ToolRequest::ReadFile {
            path: "subdir".to_string(),
            offset: None,
            limit: None,
        },
    );

    assert!(matches!(result, Err(ToolError::NotAFile { .. })));
}

#[test]
fn read_file_oversized_returns_too_large() {
    let dir = TempDir::new();
    let oversized = vec![b'x'; (MAX_READ_FILE_BYTES + 1) as usize];
    std::fs::write(dir.path().join("big.bin"), &oversized).expect("write oversized file");

    let registry = ToolRegistry::new_readonly();
    let ctx = make_context(dir.path());
    let result = registry.execute(
        &ctx,
        ToolRequest::ReadFile {
            path: "big.bin".to_string(),
            offset: None,
            limit: None,
        },
    );

    assert!(matches!(
        result,
        Err(ToolError::TooLarge {
            max_bytes: MAX_READ_FILE_BYTES,
            ..
        })
    ));
}

#[test]
fn read_file_non_utf8_returns_non_utf8() {
    let dir = TempDir::new();
    let invalid_utf8: &[u8] = &[0xFF, 0xFE, 0xFF];
    std::fs::write(dir.path().join("bad.bin"), invalid_utf8).expect("write non-utf8 file");

    let registry = ToolRegistry::new_readonly();
    let ctx = make_context(dir.path());
    let result = registry.execute(
        &ctx,
        ToolRequest::ReadFile {
            path: "bad.bin".to_string(),
            offset: None,
            limit: None,
        },
    );

    assert!(matches!(result, Err(ToolError::NonUtf8 { .. })));
}

#[test]
fn tool_risk_read_only_as_str() {
    assert_eq!(ToolRisk::ReadOnly.as_str(), "read_only");
}

#[test]
fn search_text_returns_search_results() {
    let dir = TempDir::new();
    std::fs::write(dir.path().join("hello.txt"), "hello world\n").expect("write file");

    let registry = ToolRegistry::new_readonly();
    let ctx = make_context(dir.path());
    let result = registry.execute(
        &ctx,
        ToolRequest::SearchText {
            query: "hello".to_string(),
        },
    );

    assert!(
        matches!(result, Ok(ToolOutput::SearchResults { .. })),
        "expected SearchResults, got {:?}",
        result
    );
}

#[test]
fn search_text_no_matches_returns_empty_search_results() {
    let dir = TempDir::new();
    std::fs::write(dir.path().join("file.txt"), "no match here\n").expect("write file");

    let registry = ToolRegistry::new_readonly();
    let ctx = make_context(dir.path());
    let result = registry.execute(
        &ctx,
        ToolRequest::SearchText {
            query: "ZZZNOMATCH".to_string(),
        },
    );

    match result {
        Ok(ToolOutput::SearchResults {
            matches, truncated, ..
        }) => {
            assert!(matches.is_empty(), "expected empty matches");
            assert!(!truncated, "expected truncated=false");
        }
        other => panic!("expected SearchResults, got {:?}", other),
    }
}

// --- GlobFiles registry tests ---

#[test]
fn glob_files_simple_match_returns_file_matches() {
    let dir = TempDir::new();
    std::fs::write(dir.path().join("main.rs"), "fn main() {}").expect("write file");
    std::fs::write(dir.path().join("readme.txt"), "readme").expect("write file");

    let registry = ToolRegistry::new_readonly();
    let ctx = make_context(dir.path());
    let result = registry.execute(
        &ctx,
        ToolRequest::GlobFiles {
            pattern: "*.rs".to_string(),
        },
    );

    match result {
        Ok(ToolOutput::FileMatches {
            pattern,
            paths,
            truncated,
        }) => {
            assert_eq!(pattern, "*.rs");
            assert_eq!(paths, vec!["main.rs"]);
            assert!(!truncated);
        }
        other => panic!("expected FileMatches, got {:?}", other),
    }
}

#[test]
fn glob_files_nested_match_returns_file_matches() {
    let dir = TempDir::new();
    std::fs::create_dir(dir.path().join("src")).expect("create dir");
    std::fs::write(dir.path().join("src").join("lib.rs"), "").expect("write file");
    std::fs::write(dir.path().join("src").join("main.rs"), "").expect("write file");
    std::fs::write(dir.path().join("readme.md"), "").expect("write file");

    let registry = ToolRegistry::new_readonly();
    let ctx = make_context(dir.path());
    let result = registry.execute(
        &ctx,
        ToolRequest::GlobFiles {
            pattern: "**/*.rs".to_string(),
        },
    );

    match result {
        Ok(ToolOutput::FileMatches {
            paths, truncated, ..
        }) => {
            assert_eq!(paths, vec!["src/lib.rs", "src/main.rs"]);
            assert!(!truncated);
        }
        other => panic!("expected FileMatches, got {:?}", other),
    }
}

#[test]
fn glob_files_no_match_returns_empty_file_matches() {
    let dir = TempDir::new();
    std::fs::write(dir.path().join("file.txt"), "content").expect("write file");

    let registry = ToolRegistry::new_readonly();
    let ctx = make_context(dir.path());
    let result = registry.execute(
        &ctx,
        ToolRequest::GlobFiles {
            pattern: "*.rs".to_string(),
        },
    );

    match result {
        Ok(ToolOutput::FileMatches {
            paths, truncated, ..
        }) => {
            assert!(paths.is_empty(), "expected empty paths");
            assert!(!truncated);
        }
        other => panic!("expected FileMatches, got {:?}", other),
    }
}

#[test]
fn glob_files_truncation_sets_truncated_true() {
    use super::glob::GLOB_MAX_MATCHES;

    let dir = TempDir::new();
    for i in 0..=GLOB_MAX_MATCHES {
        std::fs::write(dir.path().join(format!("f{:04}.rs", i)), "").expect("write file");
    }

    let registry = ToolRegistry::new_readonly();
    let ctx = make_context(dir.path());
    let result = registry.execute(
        &ctx,
        ToolRequest::GlobFiles {
            pattern: "*.rs".to_string(),
        },
    );

    match result {
        Ok(ToolOutput::FileMatches {
            paths, truncated, ..
        }) => {
            assert_eq!(paths.len(), GLOB_MAX_MATCHES);
            assert!(truncated, "expected truncated=true");
        }
        other => panic!("expected FileMatches, got {:?}", other),
    }
}

#[test]
fn glob_files_invalid_pattern_returns_invalid_pattern_error() {
    let dir = TempDir::new();
    let registry = ToolRegistry::new_readonly();
    let ctx = make_context(dir.path());
    let result = registry.execute(
        &ctx,
        ToolRequest::GlobFiles {
            pattern: "".to_string(),
        },
    );

    assert!(
        matches!(result, Err(ToolError::InvalidPattern { .. })),
        "expected InvalidPattern, got {:?}",
        result
    );
}

#[test]
fn glob_files_dotdot_pattern_returns_workspace_violation() {
    let dir = TempDir::new();
    let registry = ToolRegistry::new_readonly();
    let ctx = make_context(dir.path());
    let result = registry.execute(
        &ctx,
        ToolRequest::GlobFiles {
            pattern: "../escape".to_string(),
        },
    );

    assert!(
        matches!(result, Err(ToolError::WorkspaceViolation { .. })),
        "expected WorkspaceViolation, got {:?}",
        result
    );
}

// ─── Range-read unit tests ────────────────────────────────────────────────────

/// Helper: write a multi-line file in the temp dir.
fn write_lines(dir: &TempDir, name: &str, lines: &[&str]) {
    let content = lines.join("\n");
    std::fs::write(dir.path().join(name), content).expect("write file");
}

#[test]
fn range_read_basic_returns_requested_lines() {
    let dir = TempDir::new();
    write_lines(
        &dir,
        "lines.txt",
        &["line1", "line2", "line3", "line4", "line5"],
    );

    let registry = ToolRegistry::new_readonly();
    let ctx = make_context(dir.path());
    let result = registry.execute(
        &ctx,
        ToolRequest::ReadFile {
            path: "lines.txt".to_string(),
            offset: Some(2),
            limit: Some(2),
        },
    );

    match result.expect("should succeed") {
        ToolOutput::FileContent {
            content,
            start_line,
            line_count,
            truncated,
            ..
        } => {
            assert_eq!(start_line, Some(2));
            assert_eq!(line_count, Some(2));
            assert!(!truncated);
            assert_eq!(content, "line2\nline3");
        }
        other => panic!("expected FileContent, got {:?}", other),
    }
}

#[test]
fn range_read_eof_before_offset_returns_empty() {
    let dir = TempDir::new();
    write_lines(&dir, "short.txt", &["only one line"]);

    let registry = ToolRegistry::new_readonly();
    let ctx = make_context(dir.path());
    let result = registry.execute(
        &ctx,
        ToolRequest::ReadFile {
            path: "short.txt".to_string(),
            offset: Some(5),
            limit: Some(10),
        },
    );

    match result.expect("should succeed") {
        ToolOutput::FileContent {
            content,
            start_line,
            line_count,
            truncated,
            ..
        } => {
            assert_eq!(start_line, Some(5));
            assert_eq!(line_count, Some(0));
            assert!(!truncated);
            assert_eq!(content, "");
        }
        other => panic!("expected FileContent, got {:?}", other),
    }
}

#[test]
fn range_read_offset_only_defaults_limit_to_default() {
    let dir = TempDir::new();
    // Write DEFAULT_READ_RANGE_LIMIT_LINES + 5 more lines than the default.
    let all_lines: Vec<String> = (1..=DEFAULT_READ_RANGE_LIMIT_LINES + 5)
        .map(|i| format!("line{}", i))
        .collect();
    let refs: Vec<&str> = all_lines.iter().map(|s| s.as_str()).collect();
    write_lines(&dir, "many.txt", &refs);

    let registry = ToolRegistry::new_readonly();
    let ctx = make_context(dir.path());
    let result = registry.execute(
        &ctx,
        ToolRequest::ReadFile {
            path: "many.txt".to_string(),
            offset: Some(1),
            limit: None, // should default to DEFAULT_READ_RANGE_LIMIT_LINES
        },
    );

    match result.expect("should succeed") {
        ToolOutput::FileContent {
            line_count,
            start_line,
            ..
        } => {
            assert_eq!(start_line, Some(1));
            // Should return exactly DEFAULT_READ_RANGE_LIMIT_LINES lines.
            assert_eq!(
                line_count,
                Some(DEFAULT_READ_RANGE_LIMIT_LINES),
                "expected default limit of {} lines",
                DEFAULT_READ_RANGE_LIMIT_LINES
            );
        }
        other => panic!("expected FileContent, got {:?}", other),
    }
}

#[test]
fn range_read_limit_only_reads_from_line_1() {
    let dir = TempDir::new();
    write_lines(&dir, "ab.txt", &["alpha", "beta", "gamma"]);

    let registry = ToolRegistry::new_readonly();
    let ctx = make_context(dir.path());
    let result = registry.execute(
        &ctx,
        ToolRequest::ReadFile {
            path: "ab.txt".to_string(),
            offset: None,
            limit: Some(2),
        },
    );

    match result.expect("should succeed") {
        ToolOutput::FileContent {
            content,
            start_line,
            line_count,
            ..
        } => {
            assert_eq!(start_line, Some(1));
            assert_eq!(line_count, Some(2));
            assert_eq!(content, "alpha\nbeta");
        }
        other => panic!("expected FileContent, got {:?}", other),
    }
}

#[test]
fn range_read_on_large_file_succeeds_when_output_bounded() {
    // File is larger than MAX_READ_FILE_BYTES but range read succeeds
    // because we only read a small slice line-by-line.
    let dir = TempDir::new();

    // Write 2 KiB of 'A' + newline in the first line,
    // followed by enough 'x' lines to push total > MAX_READ_FILE_BYTES.
    let big_first_line = "A".repeat(2048);
    let filler_line = "x".repeat(64); // 64 bytes
    // MAX_READ_FILE_BYTES = 64 * 1024 = 65536 bytes
    // We need > 65536 bytes total. 2048 + N*65 > 65536 → N > 983
    let mut lines: Vec<String> = vec![big_first_line];
    for i in 0..1100usize {
        lines.push(format!("{}{}", filler_line, i));
    }
    let content = lines.join("\n");
    assert!(
        content.len() > MAX_READ_FILE_BYTES as usize,
        "test file must be larger than MAX_READ_FILE_BYTES"
    );
    std::fs::write(dir.path().join("large.txt"), &content).expect("write large file");

    let registry = ToolRegistry::new_readonly();
    let ctx = make_context(dir.path());

    // Range read: only line 2, limit 1 — well within the byte cap.
    let result = registry.execute(
        &ctx,
        ToolRequest::ReadFile {
            path: "large.txt".to_string(),
            offset: Some(2),
            limit: Some(1),
        },
    );

    match result.expect("range read on large file should succeed") {
        ToolOutput::FileContent {
            start_line,
            line_count,
            truncated,
            ..
        } => {
            assert_eq!(start_line, Some(2));
            assert_eq!(line_count, Some(1));
            assert!(!truncated);
        }
        other => panic!("expected FileContent, got {:?}", other),
    }
}

#[test]
fn range_read_non_utf8_returns_non_utf8() {
    let dir = TempDir::new();
    // Write a file with valid UTF-8 on line 1 then invalid bytes on line 2.
    let mut content = b"valid line\n".to_vec();
    content.extend_from_slice(&[0xFF, 0xFE, 0xFF]);
    content.push(b'\n');
    std::fs::write(dir.path().join("mixed.bin"), &content).expect("write file");

    let registry = ToolRegistry::new_readonly();
    let ctx = make_context(dir.path());
    let result = registry.execute(
        &ctx,
        ToolRequest::ReadFile {
            path: "mixed.bin".to_string(),
            offset: Some(1),
            limit: Some(5),
        },
    );

    assert!(
        matches!(result, Err(ToolError::NonUtf8 { .. })),
        "expected NonUtf8 for file with invalid UTF-8, got {:?}",
        result
    );
}

#[test]
fn range_read_workspace_escape_rejected() {
    let dir = TempDir::new();
    let registry = ToolRegistry::new_readonly();
    let ctx = make_context(dir.path());
    let result = registry.execute(
        &ctx,
        ToolRequest::ReadFile {
            path: "../escape.txt".to_string(),
            offset: Some(1),
            limit: Some(5),
        },
    );

    assert!(
        matches!(result, Err(ToolError::WorkspaceViolation { .. })),
        "expected WorkspaceViolation, got {:?}",
        result
    );
}

#[test]
fn range_read_byte_cap_truncation_sets_truncated_true() {
    let dir = TempDir::new();
    // Write many lines each large enough that after a few lines the byte cap hits.
    // MAX_READ_RANGE_OUTPUT_BYTES = 64 * 1024 = 65536.
    // Write 200 lines of 1000 'x' chars each → 200 * 1001 = 200200 bytes > 65536.
    let big_line = "x".repeat(1000);
    let lines: Vec<&str> = (0..200).map(|_| big_line.as_str()).collect();
    write_lines(&dir, "biglines.txt", &lines);

    let registry = ToolRegistry::new_readonly();
    let ctx = make_context(dir.path());
    let result = registry.execute(
        &ctx,
        ToolRequest::ReadFile {
            path: "biglines.txt".to_string(),
            offset: Some(1),
            limit: Some(200),
        },
    );

    match result.expect("should succeed with truncation") {
        ToolOutput::FileContent {
            start_line,
            truncated,
            content,
            ..
        } => {
            assert_eq!(start_line, Some(1));
            assert!(truncated, "expected truncated=true when byte cap is hit");
            assert!(
                content.len() <= MAX_READ_RANGE_OUTPUT_BYTES,
                "content length {} must be <= MAX_READ_RANGE_OUTPUT_BYTES {}",
                content.len(),
                MAX_READ_RANGE_OUTPUT_BYTES
            );
        }
        other => panic!("expected FileContent, got {:?}", other),
    }
}

#[test]
fn range_read_limit_does_not_read_past_requested_window() {
    let dir = TempDir::new();
    // Lines 1-2 are valid UTF-8; line 3 is invalid. A read of the first two
    // lines must succeed without ever decoding line 3 (regression: the loop
    // used to read one line past the requested count before breaking).
    let mut content = b"line one\nline two\n".to_vec();
    content.extend_from_slice(&[0xFF, 0xFE, 0xFF]);
    content.push(b'\n');
    std::fs::write(dir.path().join("guard.bin"), &content).expect("write file");

    let registry = ToolRegistry::new_readonly();
    let ctx = make_context(dir.path());
    let result = registry.execute(
        &ctx,
        ToolRequest::ReadFile {
            path: "guard.bin".to_string(),
            offset: Some(1),
            limit: Some(2),
        },
    );

    match result.expect("limit=2 read must not touch the invalid third line") {
        ToolOutput::FileContent {
            content,
            start_line,
            line_count,
            truncated,
            ..
        } => {
            assert_eq!(start_line, Some(1));
            assert_eq!(line_count, Some(2));
            assert!(!truncated);
            assert_eq!(content, "line one\nline two");
        }
        other => panic!("expected FileContent, got {:?}", other),
    }
}

#[test]
fn range_read_single_line_exactly_at_byte_cap_is_not_truncated() {
    let dir = TempDir::new();
    // A single line of exactly MAX_READ_RANGE_OUTPUT_BYTES bytes must be
    // returned whole with truncated=false (regression: the per-line `+1`
    // separator accounting used to flag this exact-fit case as truncated).
    let exact = "a".repeat(MAX_READ_RANGE_OUTPUT_BYTES);
    std::fs::write(dir.path().join("exact.txt"), &exact).expect("write file");

    let registry = ToolRegistry::new_readonly();
    let ctx = make_context(dir.path());
    let result = registry.execute(
        &ctx,
        ToolRequest::ReadFile {
            path: "exact.txt".to_string(),
            offset: Some(1),
            limit: Some(1),
        },
    );

    match result.expect("should succeed") {
        ToolOutput::FileContent {
            content,
            line_count,
            truncated,
            ..
        } => {
            assert_eq!(line_count, Some(1));
            assert!(
                !truncated,
                "a line exactly at the byte cap must not be marked truncated"
            );
            assert_eq!(content.len(), MAX_READ_RANGE_OUTPUT_BYTES);
        }
        other => panic!("expected FileContent, got {:?}", other),
    }
}

#[test]
fn range_read_single_line_far_larger_than_cap_truncates_bounded() {
    let dir = TempDir::new();
    // A single line many times larger than the output cap must come back
    // truncated and bounded — the range reader must NOT load the whole line
    // into memory (regression: a line-at-a-time reader allocated the full line).
    let huge = "a".repeat(MAX_READ_RANGE_OUTPUT_BYTES * 4);
    std::fs::write(dir.path().join("huge_line.txt"), &huge).expect("write file");

    let registry = ToolRegistry::new_readonly();
    let ctx = make_context(dir.path());
    let result = registry.execute(
        &ctx,
        ToolRequest::ReadFile {
            path: "huge_line.txt".to_string(),
            offset: Some(1),
            limit: Some(1),
        },
    );

    match result.expect("should succeed") {
        ToolOutput::FileContent {
            content,
            start_line,
            truncated,
            ..
        } => {
            assert_eq!(start_line, Some(1));
            assert!(truncated, "a line larger than the cap must be truncated");
            assert!(
                content.len() <= MAX_READ_RANGE_OUTPUT_BYTES,
                "content {} must stay within the byte cap {}",
                content.len(),
                MAX_READ_RANGE_OUTPUT_BYTES
            );
        }
        other => panic!("expected FileContent, got {:?}", other),
    }
}

#[test]
fn range_read_offset_past_eof_on_huge_single_line_returns_empty() {
    let dir = TempDir::new();
    // One huge line (no newline). An offset past the only line must report an
    // empty range without loading the line into memory.
    let huge = "z".repeat(MAX_READ_RANGE_OUTPUT_BYTES * 4);
    std::fs::write(dir.path().join("huge_one_line.txt"), &huge).expect("write file");

    let registry = ToolRegistry::new_readonly();
    let ctx = make_context(dir.path());
    let result = registry.execute(
        &ctx,
        ToolRequest::ReadFile {
            path: "huge_one_line.txt".to_string(),
            offset: Some(5),
            limit: Some(1),
        },
    );

    match result.expect("should succeed") {
        ToolOutput::FileContent {
            content,
            start_line,
            line_count,
            truncated,
            ..
        } => {
            assert_eq!(start_line, Some(5));
            assert_eq!(line_count, Some(0));
            assert!(!truncated);
            assert_eq!(content, "");
        }
        other => panic!("expected FileContent, got {:?}", other),
    }
}

#[test]
fn range_read_multibyte_truncation_keeps_valid_utf8() {
    let dir = TempDir::new();
    // A single line of multi-byte UTF-8 chars exceeding the cap must truncate on
    // a char boundary so the returned content is still valid UTF-8.
    // 'é' is 2 bytes; repeat past the cap.
    let multibyte = "é".repeat(MAX_READ_RANGE_OUTPUT_BYTES);
    std::fs::write(dir.path().join("multibyte.txt"), &multibyte).expect("write file");

    let registry = ToolRegistry::new_readonly();
    let ctx = make_context(dir.path());
    let result = registry.execute(
        &ctx,
        ToolRequest::ReadFile {
            path: "multibyte.txt".to_string(),
            offset: Some(1),
            limit: Some(1),
        },
    );

    match result.expect("should succeed") {
        ToolOutput::FileContent {
            content, truncated, ..
        } => {
            assert!(truncated, "multibyte line over cap must be truncated");
            assert!(
                content.len() <= MAX_READ_RANGE_OUTPUT_BYTES,
                "truncated content must stay within the byte cap"
            );
            // Content is a valid Rust String, so it is already valid UTF-8; assert
            // it is a whole number of 'é' chars (no split multi-byte sequence).
            assert!(
                content.chars().all(|c| c == 'é'),
                "truncation must not split a multi-byte char"
            );
        }
        other => panic!("expected FileContent, got {:?}", other),
    }
}

#[test]
fn range_read_on_directory_returns_not_a_file() {
    let dir = TempDir::new();
    std::fs::create_dir_all(dir.path().join("subdir")).expect("create subdir");

    let registry = ToolRegistry::new_readonly();
    let ctx = make_context(dir.path());
    let result = registry.execute(
        &ctx,
        ToolRequest::ReadFile {
            path: "subdir".to_string(),
            offset: Some(1),
            limit: Some(5),
        },
    );

    assert!(
        matches!(result, Err(ToolError::NotAFile { .. })),
        "expected NotAFile for directory target with range read, got {:?}",
        result
    );
}
