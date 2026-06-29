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
        },
    );

    let output = result.expect("should succeed");
    assert!(matches!(
        output,
        ToolOutput::FileContent {
            ref path,
            ref content
        } if path == "hello.txt" && content == "hello, world!"
    ));
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
