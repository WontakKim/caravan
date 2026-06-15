//! Read-only tool harness type definitions for Caravan.
//!
//! This module defines the type system for the tool harness. Execution logic
//! (`execute`) lands in T-2; `ReadFile`/`FileContent` variants land in T-3.

use std::fs;
use std::path::{Component, Path};

/// Identifies a tool by name.
#[derive(Debug, PartialEq)]
pub enum ToolName {
    ListFiles,
    ReadFile,
}

/// Risk classification for a tool.
#[derive(Debug, PartialEq)]
pub enum ToolRisk {
    ReadOnly,
}

impl ToolRisk {
    /// Returns the canonical snake_case string for this risk level.
    pub fn as_str(&self) -> &'static str {
        match self {
            ToolRisk::ReadOnly => "read_only",
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
    ListFiles { path: String },
    ReadFile { path: String },
}

/// Outputs produced by the tool harness.
#[derive(Debug, PartialEq)]
pub enum ToolOutput {
    FileList { path: String, entries: Vec<String> },
    FileContent { path: String, content: String },
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
}

/// Maximum bytes allowed when reading a file.
pub const MAX_READ_FILE_BYTES: u64 = 64 * 1024;

/// Stateless registry that vends tool executors.
///
/// `workspace_root` is intentionally absent here; it lives on
/// [`ToolExecutionContext`] and is supplied per call.
#[derive(Debug, PartialEq)]
pub struct ToolRegistry;

/// Resolves `requested` (a relative path) to an absolute, canonicalized path
/// that is guaranteed to remain inside `ctx.workspace_root`.
///
/// # Security
///
/// - Absolute components (`/`, drive prefix) and `..` are rejected **lexically**
///   before any filesystem operation or join, so outside-existence is never
///   probed.
/// - After joining and canonicalizing (which follows symlinks), a `starts_with`
///   check provides defense-in-depth against symlink escapes.
///
/// # Errors
///
/// - [`ToolError::WorkspaceViolation`] for absolute or `..`-containing paths,
///   or if the canonical path escapes the canonical root.
/// - [`ToolError::NotFound`] if the path does not exist.
/// - [`ToolError::Io`] for any other I/O error.
fn resolve_in_workspace(
    ctx: &ToolExecutionContext,
    requested: &str,
) -> Result<std::path::PathBuf, ToolError> {
    // Lexical validation: reject absolute paths and any `..` before any
    // filesystem operation or join (existence-independent).
    for component in Path::new(requested).components() {
        match component {
            Component::RootDir | Component::Prefix(_) => {
                return Err(ToolError::WorkspaceViolation {
                    path: requested.to_string(),
                });
            }
            Component::ParentDir => {
                return Err(ToolError::WorkspaceViolation {
                    path: requested.to_string(),
                });
            }
            Component::Normal(_) | Component::CurDir => {}
        }
    }

    // Canonicalize the workspace root (handles macOS /var -> /private/var).
    let canonical_root = fs::canonicalize(&ctx.workspace_root).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ToolError::NotFound {
                path: ctx.workspace_root.display().to_string(),
            }
        } else {
            ToolError::Io {
                message: e.to_string(),
            }
        }
    })?;

    // Join and canonicalize the candidate (follows symlinks).
    let candidate = canonical_root.join(requested);
    let canonical = fs::canonicalize(&candidate).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ToolError::NotFound {
                path: requested.to_string(),
            }
        } else {
            ToolError::Io {
                message: e.to_string(),
            }
        }
    })?;

    // Defense-in-depth: the canonicalized path must still be under the root
    // (catches symlinks whose canonical target escapes the workspace).
    if !canonical.starts_with(&canonical_root) {
        return Err(ToolError::WorkspaceViolation {
            path: requested.to_string(),
        });
    }

    Ok(canonical)
}

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
            ToolRequest::ReadFile { path } => self.read_file(context, &path),
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

    /// Reads a file's UTF-8 contents, capping at [`MAX_READ_FILE_BYTES`].
    ///
    /// The size cap is checked from file metadata **before** reading any bytes
    /// into memory. UTF-8 is validated strictly with `String::from_utf8`; lossy
    /// conversion is never used.
    fn read_file(
        &self,
        context: &ToolExecutionContext,
        requested: &str,
    ) -> Result<ToolOutput, ToolError> {
        let canonical = resolve_in_workspace(context, requested)?;

        // Use Path::is_file() (not fs::metadata) for the not-a-file guard so
        // the only fs::metadata call in this module is the size check below.
        if !canonical.is_file() {
            return Err(ToolError::NotAFile {
                path: requested.to_string(),
            });
        }

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
        })
    }
}

#[cfg(test)]
mod tests {
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
}
