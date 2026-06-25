use kernel::commands::ToolCommand;
use kernel::manual_context::ManualToolContext;
use kernel::{ToolError, ToolEventRunner, ToolExecutionContext, ToolOutput, ToolRequest};

impl super::App {
    pub(super) fn handle_tool_command(&mut self, tc: ToolCommand) {
        let ctx = ToolExecutionContext {
            workspace_root: self.workspace_root.clone(),
        };

        // Early-return: PlanWrite records the write intent and emits approval-required
        // guidance. It must NOT set last_tool_output_candidate, pending_manual_tool_context,
        // or pending_model_tool_request, and must NOT write any file.
        if let ToolCommand::PlanWrite { path } = tc {
            let request = ToolRequest::PlanWrite { path };
            match ToolEventRunner::new_readonly().run(&mut self.event_log, &ctx, request) {
                Ok(_) => unreachable!("PlanWrite gate always returns ApprovalRequired"),
                Err(ToolError::ApprovalRequired { .. }) => {
                    self.log.push("Write plan requires approval.".to_string());
                    self.log
                        .push("Use /approval status to inspect pending approvals.".to_string());
                    self.log.push(
                        "Use /approval approve <seq> or /approval reject <seq> to resolve."
                            .to_string(),
                    );
                }
                Err(error) => {
                    self.push_tool_error_output(error);
                }
            }
            return;
        }

        // Early-return: PreviewWrite runs the dry-run diff preview using
        // last_tool_output_candidate as proposed content. It performs NO file write,
        // creates NO ApprovalRequest, and does NOT update last_tool_output_candidate,
        // pending_manual_tool_context, or pending_model_tool_request.
        if let ToolCommand::PreviewWrite { path } = tc {
            let Some(candidate) = self.last_tool_output_candidate.as_ref() else {
                self.log.push(
                    "No latest tool output to preview. Run /tool read <path> or /tool list [path] first.".to_string(),
                );
                return;
            };
            let content = candidate.content.clone();
            let request = ToolRequest::PreviewWrite {
                path: path.clone(),
                content,
            };
            match ToolEventRunner::new_readonly().run(&mut self.event_log, &ctx, request) {
                Ok(ToolOutput::WritePreview { preview, .. }) => {
                    self.push_tool_write_preview_output(&path, &preview);
                }
                Ok(_) => unreachable!("PreviewWrite only produces WritePreview output"),
                Err(error) => {
                    self.push_tool_error_output(error);
                }
            }
            return;
        }

        // Early-return: ProposeWrite runs the dry-run diff preview using
        // last_tool_output_candidate as proposed content, then emits a workspace_write
        // ToolPolicy + ApprovalRequest with the preview summary. It performs NO file
        // write and does NOT update last_tool_output_candidate, pending_manual_tool_context,
        // or pending_model_tool_request.
        if let ToolCommand::ProposeWrite { path } = tc {
            let Some(candidate) = self.last_tool_output_candidate.as_ref() else {
                self.log.push(
                    "No latest tool output to propose. Run /tool read <path> or /tool list [path] first.".to_string(),
                );
                return;
            };
            let content = candidate.content.clone();
            let request = ToolRequest::PreviewWrite {
                path: path.clone(),
                content,
            };
            let runner = ToolEventRunner::new_readonly();
            match runner.run(&mut self.event_log, &ctx, request) {
                Ok(ToolOutput::WritePreview { preview, .. }) => {
                    runner.append_write_approval(&mut self.event_log, &path, &preview);
                    self.push_tool_write_proposal_output(&path, &preview);
                }
                Ok(_) => unreachable!("PreviewWrite only produces WritePreview output"),
                Err(error) => {
                    self.log.push("Write proposal preview failed:".to_string());
                    self.push_tool_error_output(error);
                }
            }
            return;
        }

        let (request, display_path) = match tc {
            ToolCommand::List { path } => {
                let dp = path.clone();
                (ToolRequest::ListFiles { path }, dp)
            }
            ToolCommand::Read { path } => {
                let dp = path.clone();
                (ToolRequest::ReadFile { path }, dp)
            }
            ToolCommand::PlanWrite { .. } => unreachable!("handled above"),
            ToolCommand::PreviewWrite { .. } => unreachable!("handled above"),
            ToolCommand::ProposeWrite { .. } => unreachable!("handled above"),
        };
        match ToolEventRunner::new_readonly().run(&mut self.event_log, &ctx, request) {
            Ok(ToolOutput::FileList { entries, .. }) => {
                let ctx = ManualToolContext::from_list_files(&display_path, &entries);
                self.last_tool_output_candidate = Some(ctx.clone());
                self.pending_manual_tool_context = Some(ctx);
                self.push_tool_list_output(&display_path, entries);
                self.log.push(
                    "This tool output will be used as context for your next message.".to_string(),
                );
            }
            Ok(ToolOutput::FileContent { content, .. }) => {
                let ctx = ManualToolContext::from_read_file(&display_path, &content);
                self.last_tool_output_candidate = Some(ctx.clone());
                self.pending_manual_tool_context = Some(ctx);
                self.push_tool_read_output(&display_path, &content);
                self.log.push(
                    "This tool output will be used as context for your next message.".to_string(),
                );
            }
            Ok(ToolOutput::WritePreview { .. }) => {
                unreachable!("handled in PreviewWrite early-return")
            }
            Err(error) => {
                self.push_tool_error_output(error);
            }
        }
    }
}
