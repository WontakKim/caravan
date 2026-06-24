//! Read-only loader for the workspace-root `CLAUDE.md` project memory file.

use std::io::ErrorKind;

pub const PROJECT_MEMORY_MAX_BYTES: usize = 32 * 1024;

/// Where the loaded project memory came from.
#[derive(Debug, PartialEq)]
pub enum ProjectMemorySource {
    /// No `CLAUDE.md` file was found at the workspace root.
    Missing,
    /// Content was loaded from the given file path.
    File { path: String },
    /// An I/O or encoding error prevented loading.
    Error { message: String },
}

/// A snapshot of the workspace-root `CLAUDE.md`, loaded read-only at
/// session start for inclusion in the prompt's project-memory section.
#[derive(Debug)]
pub struct ProjectMemory {
    pub source: ProjectMemorySource,
    pub content: String,
    pub truncated: bool,
}

impl ProjectMemory {
    /// Returns the canonical missing-file fallback.
    ///
    /// Callers and tests use this instead of constructing the variant
    /// directly so the fallback string stays in one place.
    pub fn missing() -> Self {
        Self {
            source: ProjectMemorySource::Missing,
            content: "No CLAUDE.md project memory found.".to_string(),
            truncated: false,
        }
    }
}

/// Loads `<workspace_root>/CLAUDE.md` read-only.
///
/// - **Missing file** → [`ProjectMemory::missing()`] fallback.
/// - **Present file** → reads at most [`PROJECT_MEMORY_MAX_BYTES`] bytes from
///   the start, sets `truncated = true` only when the file is larger than the
///   limit. Requires valid UTF-8; any encoding or I/O error falls through to
///   the error branch.
/// - **I/O or UTF-8 error** → returns `source: Error { message }` with a
///   short readable summary and an empty content string.
pub fn load_project_memory(workspace_root: &std::path::Path) -> ProjectMemory {
    let path = workspace_root.join("CLAUDE.md");

    let raw = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == ErrorKind::NotFound => return ProjectMemory::missing(),
        Err(err) => {
            return ProjectMemory {
                source: ProjectMemorySource::Error {
                    message: format!("failed to read CLAUDE.md: {err}"),
                },
                content: String::new(),
                truncated: false,
            };
        }
    };

    let file_size = raw.len();
    let truncated = file_size > PROJECT_MEMORY_MAX_BYTES;
    let slice = if truncated {
        &raw[..PROJECT_MEMORY_MAX_BYTES]
    } else {
        &raw[..]
    };

    let content = match std::str::from_utf8(slice) {
        Ok(s) => s.to_string(),
        Err(err) => {
            return ProjectMemory {
                source: ProjectMemorySource::Error {
                    message: format!("CLAUDE.md contains invalid UTF-8: {err}"),
                },
                content: String::new(),
                truncated: false,
            };
        }
    };

    ProjectMemory {
        source: ProjectMemorySource::File {
            path: path.to_string_lossy().into_owned(),
        },
        content,
        truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write as _;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// Creates a unique temporary directory under `std::env::temp_dir()` and
    /// returns its path. The caller is responsible for removing it afterwards.
    fn make_temp_dir() -> std::path::PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("kernel_pm_test_{}_{}", std::process::id(), id));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_file(dir: &std::path::Path, name: &str, bytes: &[u8]) {
        let mut f = fs::File::create(dir.join(name)).unwrap();
        f.write_all(bytes).unwrap();
    }

    // (a) Missing CLAUDE.md yields the fallback string and Missing source.
    #[test]
    fn missing_file_returns_fallback() {
        let dir = make_temp_dir();
        let pm = load_project_memory(&dir);
        assert_eq!(pm.source, ProjectMemorySource::Missing);
        assert_eq!(pm.content, "No CLAUDE.md project memory found.");
        assert!(!pm.truncated);
        let _ = fs::remove_dir_all(&dir);
    }

    // (b) Present UTF-8 CLAUDE.md loads its content with File source and truncated == false.
    #[test]
    fn present_file_loads_content_untruncated() {
        let dir = make_temp_dir();
        write_file(&dir, "CLAUDE.md", b"# Project\nHello world.");
        let pm = load_project_memory(&dir);
        let expected_path = dir.join("CLAUDE.md").to_string_lossy().into_owned();
        assert_eq!(
            pm.source,
            ProjectMemorySource::File {
                path: expected_path
            }
        );
        assert_eq!(pm.content, "# Project\nHello world.");
        assert!(!pm.truncated);
        let _ = fs::remove_dir_all(&dir);
    }

    // (c) A file larger than PROJECT_MEMORY_MAX_BYTES is bounded to the limit with truncated == true.
    #[test]
    fn oversized_file_is_truncated() {
        let dir = make_temp_dir();
        let big = vec![b'a'; PROJECT_MEMORY_MAX_BYTES + 1];
        write_file(&dir, "CLAUDE.md", &big);
        let pm = load_project_memory(&dir);
        assert!(pm.truncated);
        assert_eq!(pm.content.len(), PROJECT_MEMORY_MAX_BYTES);
        assert!(matches!(pm.source, ProjectMemorySource::File { .. }));
        let _ = fs::remove_dir_all(&dir);
    }

    // (d) Non-UTF-8 bytes yield an Error source rather than a panic.
    #[test]
    fn non_utf8_bytes_yield_error_not_panic() {
        let dir = make_temp_dir();
        // 0xFF is never valid in UTF-8.
        write_file(&dir, "CLAUDE.md", &[0xFF, 0xFE, 0xFD]);
        let pm = load_project_memory(&dir);
        assert!(
            matches!(pm.source, ProjectMemorySource::Error { .. }),
            "expected Error source, got: {:?}",
            pm.source
        );
        // Content must be empty and the loader must not panic.
        assert!(pm.content.is_empty());
        assert!(!pm.truncated);
        let _ = fs::remove_dir_all(&dir);
    }

    // (e) ProjectMemory::missing() constructor matches the Missing fallback.
    #[test]
    fn missing_constructor_matches_fallback() {
        let pm = ProjectMemory::missing();
        assert_eq!(pm.source, ProjectMemorySource::Missing);
        assert_eq!(pm.content, "No CLAUDE.md project memory found.");
        assert!(!pm.truncated);
    }
}
