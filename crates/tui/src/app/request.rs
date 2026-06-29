use kernel::commands::RequestCommand;
use kernel::manual_context::ManualToolContext;
use kernel::{ToolEventRunner, ToolExecutionContext, ToolOutput};

impl super::App {
    pub(super) fn handle_request_command(&mut self, rc: RequestCommand) {
        match rc {
            RequestCommand::Status => {
                self.log.push("Model tool request status:".to_string());
                if let Some(req) = &self.pending_model_tool_request {
                    self.log.push(format!("- pending: {}", req.detail()));
                    self.log
                        .push(format!("- suggested command: {}", req.suggested_command()));
                    self.log.push(
                        "- next: run /context attach-last-tool after the tool succeeds".to_string(),
                    );
                } else {
                    self.log.push("- pending: none".to_string());
                }
            }
            RequestCommand::Clear => {
                self.pending_model_tool_request = None;
                self.log
                    .push("Cleared pending model tool request.".to_string());
            }
            RequestCommand::Run => {
                if let Some(req) = self.pending_model_tool_request.clone() {
                    let ctx = ToolExecutionContext {
                        workspace_root: self.workspace_root.clone(),
                    };
                    let display_path = req.path.clone();
                    let tool_request = req.to_tool_request();
                    match ToolEventRunner::new_readonly().run(
                        &mut self.event_log,
                        &ctx,
                        tool_request,
                    ) {
                        Ok(ToolOutput::FileList { entries, .. }) => {
                            self.last_tool_output_candidate =
                                Some(ManualToolContext::from_list_files(&display_path, &entries));
                            self.push_tool_list_output(&display_path, entries);
                            self.pending_model_tool_request = None;
                            self.log.push(
                                "Run /context attach-last-tool to include this tool output in the next prompt.".to_string(),
                            );
                        }
                        Ok(ToolOutput::FileContent { content, .. }) => {
                            self.last_tool_output_candidate =
                                Some(ManualToolContext::from_read_file(&display_path, &content));
                            self.push_tool_read_output(&display_path, &content);
                            self.pending_model_tool_request = None;
                            self.log.push(
                                "Run /context attach-last-tool to include this tool output in the next prompt.".to_string(),
                            );
                        }
                        Ok(ToolOutput::WritePreview { .. }) => unreachable!(
                            "preview-write is operator-only; never produced by /request run"
                        ),
                        Ok(ToolOutput::SearchResults {
                            ref query,
                            ref matches,
                            truncated,
                        }) => {
                            // SearchResults is not expected from /request run today
                            // (only list_files/read_file are reconstructed), but we degrade
                            // gracefully rather than panicking on a public ToolOutput variant.
                            // Stage the candidate so the attach hint is accurate, matching
                            // the FileList/FileContent arms above.
                            self.last_tool_output_candidate = Some(
                                ManualToolContext::from_search_text(query, matches, truncated),
                            );
                            self.push_tool_search_output(query, matches, truncated);
                            self.pending_model_tool_request = None;
                            self.log.push(
                                "Run /context attach-last-tool to include this tool output in the next prompt.".to_string(),
                            );
                        }
                        Ok(ToolOutput::FileMatches { .. }) => {
                            unreachable!("Glob handled in early-return")
                        }
                        Err(error) => {
                            self.push_tool_error_output(error);
                            // Keep pending_model_tool_request unchanged on failure.
                        }
                    }
                } else {
                    self.log.push("No pending model tool request.".to_string());
                }
            }
        }
    }
}
