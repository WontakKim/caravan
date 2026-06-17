use kernel::commands::ToolCommand;
use kernel::manual_context::ManualToolContext;
use kernel::{ToolEventRunner, ToolExecutionContext, ToolOutput, ToolRequest};

impl super::App {
    pub(super) fn handle_tool_command(&mut self, tc: ToolCommand) {
        let ctx = ToolExecutionContext {
            workspace_root: self.workspace_root.clone(),
        };
        let (request, display_path) = match tc {
            ToolCommand::List { path } => {
                let dp = path.clone();
                (ToolRequest::ListFiles { path }, dp)
            }
            ToolCommand::Read { path } => {
                let dp = path.clone();
                (ToolRequest::ReadFile { path }, dp)
            }
        };
        match ToolEventRunner::new_readonly().run(&mut self.event_log, &ctx, request) {
            Ok(ToolOutput::FileList { entries, .. }) => {
                self.last_tool_output_candidate =
                    Some(ManualToolContext::from_list_files(&display_path, &entries));
                self.push_tool_list_output(&display_path, entries);
            }
            Ok(ToolOutput::FileContent { content, .. }) => {
                self.last_tool_output_candidate =
                    Some(ManualToolContext::from_read_file(&display_path, &content));
                self.push_tool_read_output(&display_path, &content);
            }
            Err(error) => {
                self.push_tool_error_output(error);
            }
        }
    }
}
