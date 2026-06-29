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

        // Early-return: Search runs a workspace text search and auto-stages a context.
        if let ToolCommand::Search { query } = tc {
            let request = ToolRequest::SearchText {
                query: query.clone(),
            };
            match ToolEventRunner::new_readonly().run(&mut self.event_log, &ctx, request) {
                Ok(ToolOutput::SearchResults {
                    ref matches,
                    truncated,
                    ..
                }) => {
                    let tool_ctx = ManualToolContext::from_search_text(&query, matches, truncated);
                    self.last_tool_output_candidate = Some(tool_ctx.clone());
                    self.pending_manual_tool_context = Some(tool_ctx);
                    self.push_tool_search_output(&query, matches, truncated);
                    self.log.push(
                        "This tool output will be used as workspace context for your next message."
                            .to_string(),
                    );
                }
                Ok(_) => unreachable!("SearchText only produces SearchResults output"),
                Err(error) => {
                    self.push_tool_error_output(error);
                }
            }
            return;
        }

        // Early-return: Glob runs a workspace file-glob and auto-stages a context.
        if let ToolCommand::Glob { pattern } = tc {
            let request = ToolRequest::GlobFiles {
                pattern: pattern.clone(),
            };
            match ToolEventRunner::new_readonly().run(&mut self.event_log, &ctx, request) {
                Ok(ToolOutput::FileMatches {
                    ref paths,
                    truncated,
                    ..
                }) => {
                    let tool_ctx = ManualToolContext::from_glob_files(&pattern, paths, truncated);
                    self.last_tool_output_candidate = Some(tool_ctx.clone());
                    self.pending_manual_tool_context = Some(tool_ctx);
                    self.push_tool_glob_output(&pattern, paths, truncated);
                    self.log.push(
                        "This tool output will be used as workspace context for your next message."
                            .to_string(),
                    );
                }
                Ok(_) => unreachable!("GlobFiles only produces FileMatches output"),
                Err(error) => {
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
                (
                    ToolRequest::ReadFile {
                        path,
                        offset: None,
                        limit: None,
                    },
                    dp,
                )
            }
            ToolCommand::PlanWrite { .. } => unreachable!("handled above"),
            ToolCommand::PreviewWrite { .. } => unreachable!("handled above"),
            ToolCommand::ProposeWrite { .. } => unreachable!("handled above"),
            ToolCommand::Search { .. } => unreachable!("handled above"),
            ToolCommand::Glob { .. } => unreachable!("handled above"),
        };
        match ToolEventRunner::new_readonly().run(&mut self.event_log, &ctx, request) {
            Ok(ToolOutput::FileList { entries, .. }) => {
                let ctx = ManualToolContext::from_list_files(&display_path, &entries);
                self.last_tool_output_candidate = Some(ctx.clone());
                self.pending_manual_tool_context = Some(ctx);
                self.push_tool_list_output(&display_path, entries);
                self.log.push(
                    "This tool output will be used as workspace context for your next message."
                        .to_string(),
                );
            }
            Ok(ToolOutput::FileContent { content, .. }) => {
                let ctx = ManualToolContext::from_read_file(&display_path, &content);
                self.last_tool_output_candidate = Some(ctx.clone());
                self.pending_manual_tool_context = Some(ctx);
                self.push_tool_read_output(&display_path, &content);
                self.log.push(
                    "This tool output will be used as workspace context for your next message."
                        .to_string(),
                );
            }
            Ok(ToolOutput::WritePreview { .. }) => {
                unreachable!("handled in PreviewWrite early-return")
            }
            Ok(ToolOutput::SearchResults { .. }) => {
                unreachable!("List/Read never produces SearchResults")
            }
            Ok(ToolOutput::FileMatches { .. }) => {
                unreachable!("Glob handled in early-return")
            }
            Err(error) => {
                self.push_tool_error_output(error);
            }
        }
    }
}
