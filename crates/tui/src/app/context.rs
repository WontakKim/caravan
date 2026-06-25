use kernel::commands::ContextCommand;
use kernel::events::EventKind;

impl super::App {
    pub(super) fn handle_context_command(&mut self, cc: ContextCommand) {
        match cc {
            ContextCommand::AttachLastTool => {
                if let Some(candidate) = self.last_tool_output_candidate.clone() {
                    let summary = candidate.attach_summary();
                    self.pending_manual_tool_context = Some(candidate);
                    self.event_log
                        .append(EventKind::ToolContextAttach, &summary);
                    self.log
                        .push(format!("Workspace context attached: {summary}"));
                } else {
                    self.log.push(super::NO_TOOL_OUTPUT_NOTICE.to_string());
                }
            }
            ContextCommand::Clear => {
                self.pending_manual_tool_context = None;
                self.event_log
                    .append(EventKind::ToolContextClear, "Tool context cleared");
                self.log.push("Workspace context cleared.".to_string());
            }
            ContextCommand::Status => {
                let pending_summary = self
                    .pending_manual_tool_context
                    .as_ref()
                    .map(|ctx| ctx.attach_summary())
                    .unwrap_or_else(|| "none".to_string());
                let candidate_summary = self
                    .last_tool_output_candidate
                    .as_ref()
                    .map(|ctx| ctx.attach_summary())
                    .unwrap_or_else(|| "none".to_string());
                self.log.push("Workspace context status:".to_string());
                self.log.push(format!("- pending: {}", pending_summary));
                self.log
                    .push(format!("- last tool output: {}", candidate_summary));
            }
        }
    }
}
