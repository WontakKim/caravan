use kernel::events::{EventKind, EventSeq};
use kernel::manual_context::ManualToolContext;
use kernel::{ApprovalCommand, ApprovalDecision, ApprovalDecisionRecord, ApprovalQueue};
use kernel::{ToolEventRunner, ToolExecutionContext, ToolOutput};

impl super::App {
    pub(super) fn handle_approval_command(&mut self, ac: ApprovalCommand) {
        match ac {
            ApprovalCommand::Status => {
                let queue = ApprovalQueue::from_event_log(&self.event_log);
                self.log.extend(queue.render_status_lines());
                let plans = queue.resume_plans();
                self.log
                    .push(format!("- approved resume plans: {}", plans.len()));
                for plan in &plans {
                    self.log.push(format!(
                        "- seq={} {}",
                        plan.request_seq, plan.request_detail
                    ));
                    if let Some(cmd) = plan.suggested_command() {
                        self.log.push(format!("- suggested: {cmd}"));
                    }
                }
            }
            ApprovalCommand::Approve { seq } => {
                let is_pending = {
                    let queue = ApprovalQueue::from_event_log(&self.event_log);
                    queue.pending.iter().any(|p| p.seq == EventSeq(seq))
                };
                if is_pending {
                    let record = ApprovalDecisionRecord {
                        request_seq: EventSeq(seq),
                        decision: ApprovalDecision::Approved,
                        reason: "operator_approved".to_string(),
                    };
                    self.event_log
                        .append(EventKind::ApprovalDecision, &record.detail());
                    self.log
                        .push(format!("Approved approval request seq={seq}"));
                } else {
                    self.log.push(format!("No pending approval for seq={seq}"));
                }
            }
            ApprovalCommand::Reject { seq } => {
                let is_pending = {
                    let queue = ApprovalQueue::from_event_log(&self.event_log);
                    queue.pending.iter().any(|p| p.seq == EventSeq(seq))
                };
                if is_pending {
                    let record = ApprovalDecisionRecord {
                        request_seq: EventSeq(seq),
                        decision: ApprovalDecision::Rejected,
                        reason: "operator_rejected".to_string(),
                    };
                    self.event_log
                        .append(EventKind::ApprovalDecision, &record.detail());
                    self.log
                        .push(format!("Rejected approval request seq={seq}"));
                } else {
                    self.log.push(format!("No pending approval for seq={seq}"));
                }
            }
            ApprovalCommand::Resume { seq } => {
                let queue = ApprovalQueue::from_event_log(&self.event_log);
                let plan = queue
                    .resume_plans()
                    .into_iter()
                    .find(|p| p.request_seq == EventSeq(seq));
                match plan {
                    None => {
                        self.log
                            .push(format!("No approved resume plan for seq={seq}"));
                    }
                    Some(plan) => {
                        let Some(tool_request) = plan.to_tool_request() else {
                            self.log.push(format!(
                                "Unsupported tool in approved resume plan for seq={seq}"
                            ));
                            return;
                        };
                        let display_path = plan.request.path.clone();
                        self.event_log
                            .append(EventKind::ApprovalResume, &plan.resume_detail());
                        let ctx = ToolExecutionContext {
                            workspace_root: self.workspace_root.clone(),
                        };
                        match ToolEventRunner::new_readonly().run(
                            &mut self.event_log,
                            &ctx,
                            tool_request,
                        ) {
                            Ok(ToolOutput::FileList { entries, .. }) => {
                                self.last_tool_output_candidate = Some(
                                    ManualToolContext::from_list_files(&display_path, &entries),
                                );
                                self.push_tool_list_output(&display_path, entries);
                                self.log.push(
                                    "Run /context attach-last-tool to include this tool output in the next prompt.".to_string(),
                                );
                            }
                            Ok(ToolOutput::FileContent { content, .. }) => {
                                self.last_tool_output_candidate = Some(
                                    ManualToolContext::from_read_file(&display_path, &content),
                                );
                                self.push_tool_read_output(&display_path, &content);
                                self.log.push(
                                    "Run /context attach-last-tool to include this tool output in the next prompt.".to_string(),
                                );
                            }
                            Ok(ToolOutput::WritePreview { .. }) => unreachable!(
                                "preview-write is operator-only; never produced by /approval resume"
                            ),
                            Ok(ToolOutput::SearchResults {
                                ref query,
                                ref matches,
                                truncated,
                            }) => {
                                // SearchResults is not expected from /approval resume today
                                // (only list_files/read_file are reconstructed), but we degrade
                                // gracefully rather than panicking on a public ToolOutput variant.
                                self.push_tool_search_output(query, matches, truncated);
                                self.log.push(
                                    "Run /context attach-last-tool to include this tool output in the next prompt.".to_string(),
                                );
                            }
                            Err(error) => {
                                self.push_tool_error_output(error);
                            }
                        }
                    }
                }
            }
        }
    }
}
