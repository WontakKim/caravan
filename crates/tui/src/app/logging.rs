use kernel::{SearchMatch, WRITE_DIFF_PREVIEW_LINES, WritePreview, WritePreviewKind};

impl super::App {
    /// Pushes a sorted directory listing to the screen log, capped at
    /// [`super::TOOL_LIST_PREVIEW_ENTRIES`] lines plus an overflow trailer.
    pub(super) fn push_tool_list_output(&mut self, display_path: &str, mut entries: Vec<String>) {
        entries.sort();
        self.log.push(format!("Tool list {}:", display_path));
        let total = entries.len();
        for entry in entries.iter().take(super::TOOL_LIST_PREVIEW_ENTRIES) {
            self.log.push(format!("- {}", entry));
        }
        if total > super::TOOL_LIST_PREVIEW_ENTRIES {
            self.log.push(format!(
                "... and {} more",
                total - super::TOOL_LIST_PREVIEW_ENTRIES
            ));
        }
    }

    /// Pushes a UTF-8 content preview to the screen log, truncated to at most
    /// [`super::TOOL_READ_PREVIEW_BYTES`] bytes on a valid char boundary using a
    /// backward scan with [`str::is_char_boundary`].
    pub(super) fn push_tool_read_output(&mut self, display_path: &str, content: &str) {
        self.log.push(format!("Tool read {}:", display_path));
        let mut limit = super::TOOL_READ_PREVIEW_BYTES.min(content.len());
        while limit > 0 && !content.is_char_boundary(limit) {
            limit -= 1;
        }
        let preview = &content[..limit];
        self.log.push(preview.to_string());
        if content.len() > limit {
            self.log.push("... [truncated]".to_string());
        }
    }

    /// Pushes a bounded diff preview of a proposed write to the screen log.
    ///
    /// For [`WritePreviewKind::NoChange`], emits the single literal line `"No changes."`
    /// and does NOT iterate `preview.diff.preview` (which already contains the sentinel
    /// to avoid double-printing). For other kinds, emits each bounded preview line
    /// (already capped at [`WRITE_DIFF_PREVIEW_LINES`]) followed by `"... [truncated]"`
    /// when the diff was truncated.
    pub(super) fn push_tool_write_preview_output(&mut self, path: &str, preview: &WritePreview) {
        self.log.push(format!("Write preview for {}:", path));
        self.log.push(preview.detail());
        match preview.kind {
            WritePreviewKind::NoChange => {
                self.log.push("No changes.".to_string());
            }
            _ => {
                self.log.push("Diff preview:".to_string());
                for line in preview.diff.preview.iter().take(WRITE_DIFF_PREVIEW_LINES) {
                    self.log.push(line.clone());
                }
                if preview.diff.truncated {
                    self.log.push("... [truncated]".to_string());
                }
            }
        }
    }

    /// Pushes a bounded diff preview of a proposed write to the screen log, followed by
    /// approval-request guidance lines.
    ///
    /// Mirrors [`push_tool_write_preview_output`] for the preview block but adds a blank
    /// separator line and three approval-guidance lines so the user knows what to do next.
    pub(super) fn push_tool_write_proposal_output(&mut self, path: &str, preview: &WritePreview) {
        self.log
            .push(format!("Write proposal preview for {}:", path));
        self.log.push(preview.detail());
        match preview.kind {
            WritePreviewKind::NoChange => {
                self.log.push("No changes.".to_string());
            }
            _ => {
                self.log.push("Diff preview:".to_string());
                for line in preview.diff.preview.iter().take(WRITE_DIFF_PREVIEW_LINES) {
                    self.log.push(line.clone());
                }
                if preview.diff.truncated {
                    self.log.push("... [truncated]".to_string());
                }
            }
        }
        self.log.push(String::new());
        self.log
            .push("Approval requested for proposed write.".to_string());
        self.log
            .push("Use /approval status to inspect pending approvals.".to_string());
        self.log
            .push("Use /approval approve <seq> or /approval reject <seq> to resolve.".to_string());
    }

    /// Pushes search results to the screen log.
    ///
    /// On success with matches, emits a header followed by `"<path>:<line>: <text>"` lines
    /// capped at [`super::TOOL_READ_PREVIEW_BYTES`], then `"... [truncated]"` when the
    /// output is truncated. On no matches, emits the header followed by `"No matches."`.
    pub(super) fn push_tool_search_output(
        &mut self,
        query: &str,
        matches: &[SearchMatch],
        truncated: bool,
    ) {
        self.log.push(format!("Search results for \"{}\":", query));
        if matches.is_empty() {
            self.log.push("No matches.".to_string());
            return;
        }
        let mut bytes_used: usize = 0;
        for m in matches {
            let line = format!("{}:{}: {}", m.path, m.line, m.text);
            if bytes_used + line.len() > super::TOOL_READ_PREVIEW_BYTES {
                self.log.push("... [truncated]".to_string());
                return;
            }
            bytes_used += line.len();
            self.log.push(line);
        }
        if truncated {
            self.log.push("... [truncated]".to_string());
        }
    }

    /// Pushes a single human-readable error line derived from a [`kernel::ToolError`].
    pub(super) fn push_tool_error_output(&mut self, error: kernel::ToolError) {
        let msg = match error {
            kernel::ToolError::WorkspaceViolation { path } => {
                format!("Tool error: path '{}' is outside the workspace", path)
            }
            kernel::ToolError::NotFound { path } => {
                format!("Tool error: '{}' not found", path)
            }
            kernel::ToolError::NotAFile { path } => {
                format!("Tool error: '{}' is not a file", path)
            }
            kernel::ToolError::NotADirectory { path } => {
                format!("Tool error: '{}' is not a directory", path)
            }
            kernel::ToolError::NonUtf8 { path } => {
                format!("Tool error: '{}' is not valid UTF-8", path)
            }
            kernel::ToolError::TooLarge { path, max_bytes } => {
                format!("Tool error: '{}' exceeds {} byte limit", path, max_bytes)
            }
            kernel::ToolError::Io { message } => {
                format!("Tool error: I/O error: {}", message)
            }
            kernel::ToolError::PolicyDenied { reason } => {
                format!("Tool error: policy denied ({})", reason)
            }
            // List/Read path only: new_readonly() yields ApprovalRequirement::None for
            // ReadOnly risk, so this arm is unreachable for List/Read operations.
            // WorkspaceWrite (PlanWrite) yields Manual approval and is handled by the
            // PlanWrite early-return branch in tools.rs before reaching this path.
            kernel::ToolError::ApprovalRequired { reason } => {
                format!("Tool error: approval required ({})", reason)
            }
        };
        self.log.push(msg);
    }
}
