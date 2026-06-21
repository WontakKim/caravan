//! ToolEventRunner: traces read-only tool execution as EventLog entries.

mod detail;
use detail::{
    format_approval_request_detail, format_tool_call_detail, format_tool_error_detail,
    format_tool_result_detail,
};

use crate::approval::ApprovalGate;
use crate::events::{EventKind, EventLog};
use crate::tool::policy::{ToolPolicyDecision, ToolPolicyEngine, format_tool_policy_detail};
use crate::tool::registry::{
    ToolError, ToolExecutionContext, ToolOutput, ToolRegistry, ToolRequest,
};

/// Runs read-only tool calls and records them in an [`EventLog`].
pub struct ToolEventRunner {
    registry: ToolRegistry,
    policy: ToolPolicyEngine,
}

impl ToolEventRunner {
    /// Creates a new runner backed by a read-only [`ToolRegistry`].
    pub fn new_readonly() -> Self {
        ToolEventRunner {
            registry: ToolRegistry::new_readonly(),
            policy: ToolPolicyEngine::read_only(),
        }
    }

    /// Creates a new runner with an injected [`ToolPolicyEngine`], backed by a
    /// read-only [`ToolRegistry`]. Used in tests to exercise the deny path.
    #[cfg(test)]
    pub fn with_policy(policy: ToolPolicyEngine) -> Self {
        ToolEventRunner {
            registry: ToolRegistry::new_readonly(),
            policy,
        }
    }

    /// Executes a tool request, recording a [`ToolPolicy`] event immediately
    /// before the [`ToolCall`] event, then delegating to the registry and
    /// appending either a [`ToolResult`] or [`ToolError`] event on completion.
    ///
    /// When the policy returns [`ToolPolicyDecision::Deny`], the runner records
    /// the `ToolPolicy` event and returns `Err(ToolError::PolicyDenied { .. })`
    /// without appending `ToolCall`, `ToolResult`, or `ToolError` events.
    ///
    /// The `path` is captured from `&request` BEFORE the move into
    /// [`ToolRegistry::execute`] so the event always reflects the caller-supplied
    /// path, never workspace-internal paths that may appear in error variant fields.
    pub fn run(
        &self,
        event_log: &mut EventLog,
        context: &ToolExecutionContext,
        request: ToolRequest,
    ) -> Result<ToolOutput, ToolError> {
        let (tool_name, tool_path) = match &request {
            ToolRequest::ListFiles { path } => ("list_files", path.clone()),
            ToolRequest::ReadFile { path } => ("read_file", path.clone()),
            ToolRequest::PlanWrite { path } => ("write_file", path.clone()),
        };

        let outcome = self.policy.evaluate(&request);
        event_log.append(
            EventKind::ToolPolicy,
            format_tool_policy_detail(tool_name, &tool_path, &outcome),
        );

        match outcome.decision {
            ToolPolicyDecision::Allow => {
                let gate = ApprovalGate::new();
                if let Some(request) = gate.evaluate(
                    tool_name,
                    &tool_path,
                    outcome.risk.as_str(),
                    &outcome.approval_requirement,
                ) {
                    event_log.append(
                        EventKind::ApprovalRequest,
                        format_approval_request_detail(&request),
                    );
                    return Err(ToolError::ApprovalRequired {
                        reason: request.reason,
                    });
                }

                event_log.append(
                    EventKind::ToolCall,
                    format_tool_call_detail(tool_name, &tool_path),
                );

                match self.registry.execute(context, request) {
                    Ok(output) => {
                        event_log.append(
                            EventKind::ToolResult,
                            format_tool_result_detail(tool_name, &tool_path, &output),
                        );
                        Ok(output)
                    }
                    Err(error) => {
                        event_log.append(
                            EventKind::ToolError,
                            format_tool_error_detail(tool_name, &tool_path, &error),
                        );
                        Err(error)
                    }
                }
            }
            ToolPolicyDecision::Deny => {
                // Policy denial happens before tool execution: the ToolPolicy event already
                // records decision=deny. Do not append ToolCall or ToolError because no tool
                // was invoked.
                Err(ToolError::PolicyDenied {
                    reason: outcome.reason.clone(),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests;
