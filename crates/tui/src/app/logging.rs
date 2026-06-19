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
            // production-unreachable: new_readonly() always yields ApprovalRequirement::None; kept for exhaustive match
            kernel::ToolError::ApprovalRequired { reason } => {
                format!("Tool error: approval required ({})", reason)
            }
        };
        self.log.push(msg);
    }
}
