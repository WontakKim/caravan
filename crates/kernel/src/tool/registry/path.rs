use std::fs;
use std::path::{Component, Path};

use super::{ToolError, ToolExecutionContext};

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
pub(super) fn resolve_in_workspace(
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
