use std::path::Path;

use crate::events::{EventKind, EventLog, RunId, TurnId};
use crate::manual_context::ManualToolContext;
use crate::model::tool_use::{
    ModelStepOutput, ModelStepRequest, ModelToolExchange, ModelToolResult,
    format_tool_error_for_model, format_tool_output_for_model, model_tool_call_to_request,
};
use crate::model::{ModelError, ModelRequest, ModelUsage};
use crate::model_gateway::{ModelGateway, ModelStepResponse};
use crate::project_memory::ProjectMemory;
use crate::tool::events::ToolEventRunner;
use crate::tool::registry::{ToolError, ToolExecutionContext, ToolRequest};
use crate::tool::schema::ToolCatalog;

#[derive(Debug, Clone)]
pub struct ModelToolActivity {
    pub name: String,
    pub path: String,
    pub succeeded: bool,
}

pub struct MockRunOutput {
    pub user_message: String,
    pub assistant_response: String,
    pub run_id: String,
    pub turn_id: String,
    pub detected_model_tool_request: Option<crate::model_tool_request::ModelToolRequest>,
    pub tool_activities: Vec<ModelToolActivity>,
}

fn emit_usage(event_log: &mut EventLog, usage: &ModelUsage) {
    event_log.append(
        EventKind::ModelUsage,
        format!(
            "prompt_tokens={} completion_tokens={} total_tokens={}",
            usage.prompt_tokens, usage.completion_tokens, usage.total_tokens
        ),
    );
}

fn aggregate_usage(u1: Option<ModelUsage>, u2: Option<ModelUsage>) -> Option<ModelUsage> {
    match (u1, u2) {
        (None, None) => None,
        (Some(u), None) | (None, Some(u)) => Some(u),
        (Some(a), Some(b)) => Some(ModelUsage {
            prompt_tokens: a.prompt_tokens + b.prompt_tokens,
            completion_tokens: a.completion_tokens + b.completion_tokens,
            total_tokens: a.total_tokens + b.total_tokens,
        }),
    }
}

pub fn run_mock_turn(
    event_log: &mut EventLog,
    message: &str,
    gateway: &ModelGateway,
    workspace_root: &Path,
    manual_tool_context: Option<&ManualToolContext>,
    project_memory: Option<&ProjectMemory>,
) -> MockRunOutput {
    let run_id = RunId(format!("run-{}", event_log.next_seq_value()));
    event_log.append(
        EventKind::RunCreate,
        format!("run_id={} input=\"{}\"", run_id, message),
    );
    event_log.append(EventKind::RunStart, format!("run_id={}", run_id));
    let turn_id = TurnId(format!("turn-{}", event_log.next_seq_value()));
    event_log.append(
        EventKind::TurnStart,
        format!("run_id={} turn_id={}", run_id, turn_id),
    );
    // Project the recent conversation history and drop the current (trailing)
    // user message, which the app appended before this runner ran. The
    // projection clones into an owned transcript, so no borrow of `event_log`
    // survives the subsequent PromptCompile append.
    let transcript = crate::transcript::ConversationTranscript::from_event_log(event_log);
    let history = transcript.without_trailing_user_message();
    let prompt = crate::prompt::compile_prompt_with_context(
        message,
        history,
        manual_tool_context,
        project_memory,
    );
    event_log.append(EventKind::PromptCompile, prompt.clone());
    let request = ModelRequest {
        prompt,
        user_message: message.to_string(),
    };
    // Clone the request so the follow-up step can reuse the same base prompt
    // without recompiling (T-8 hard bound: at most 1 PromptCompile per turn).
    let request_clone = request.clone();

    let first_step = ModelStepRequest {
        request,
        tools: ToolCatalog::readonly().readonly_model_definitions(),
        prior_tool_exchanges: vec![],
    };

    match gateway.complete_step(first_step) {
        Err(e) => {
            // Adapter/wire failure on first call: NO ModelRoute.
            event_log.append(EventKind::ModelError, e.to_string());
            event_log.append(
                EventKind::RunFail,
                format!("run_id={} outcome=error", run_id),
            );
            MockRunOutput {
                user_message: message.to_string(),
                assistant_response: String::new(),
                run_id: run_id.to_string(),
                turn_id: turn_id.to_string(),
                detected_model_tool_request: None,
                tool_activities: vec![],
            }
        }
        Ok(ModelStepResponse {
            route,
            output: ModelStepOutput::Assistant(out),
        }) => {
            // Direct assistant response (no tool call): success path.
            event_log.append(EventKind::ModelRoute, route.detail());
            for chunk in &out.chunks {
                event_log.append(
                    EventKind::ModelOutputChunk,
                    format!("run_id={} turn_id={} text=\"{}\"", run_id, turn_id, chunk),
                );
            }
            event_log.append(EventKind::AssistantMessage, out.response.clone());
            if let Some(usage) = out.usage {
                emit_usage(event_log, &usage);
            }
            event_log.append(
                EventKind::RunComplete,
                format!("run_id={} outcome=ok", run_id),
            );
            MockRunOutput {
                user_message: message.to_string(),
                assistant_response: out.response,
                run_id: run_id.to_string(),
                turn_id: turn_id.to_string(),
                detected_model_tool_request: None,
                tool_activities: vec![],
            }
        }
        Ok(ModelStepResponse {
            route,
            output:
                ModelStepOutput::ToolCall {
                    call,
                    usage: usage1,
                },
        }) => {
            // First call requested a tool: emit the first ModelRoute.
            event_log.append(EventKind::ModelRoute, route.detail());

            // Validate at the bridge level (unsupported name, missing/empty path, etc.).
            let tool_request = match model_tool_call_to_request(&call) {
                Err(e) => {
                    // Bridge-level error: one ModelRoute already appended, no tool events.
                    event_log.append(EventKind::ModelError, e.to_string());
                    event_log.append(
                        EventKind::RunFail,
                        format!("run_id={} outcome=error", run_id),
                    );
                    return MockRunOutput {
                        user_message: message.to_string(),
                        assistant_response: String::new(),
                        run_id: run_id.to_string(),
                        turn_id: turn_id.to_string(),
                        detected_model_tool_request: None,
                        tool_activities: vec![],
                    };
                }
                Ok(req) => req,
            };

            // Capture activity metadata from the validated ToolRequest variant so the
            // list_files default "." path is recorded, not the raw call arguments.
            let activity_name = call.name.clone();
            let activity_path = match &tool_request {
                ToolRequest::ListFiles { path } => path.clone(),
                ToolRequest::ReadFile { path } => path.clone(),
                ToolRequest::PlanWrite { path } => path.clone(),
                ToolRequest::PreviewWrite { path, .. } => path.clone(),
                ToolRequest::SearchText { query } => query.clone(),
            };

            let ctx = ToolExecutionContext {
                workspace_root: workspace_root.to_path_buf(),
            };
            let tool_outcome = ToolEventRunner::new_readonly().run(event_log, &ctx, tool_request);

            let (result, tool_succeeded) = match tool_outcome {
                Ok(output) => {
                    let r = ModelToolResult {
                        tool_call_id: call.id.clone(),
                        name: call.name.clone(),
                        content: format_tool_output_for_model(&output),
                        is_error: false,
                    };
                    (r, true)
                }
                Err(ToolError::PolicyDenied { reason }) => {
                    // Unreachable on the read-only path — internal safety mismatch.
                    event_log.append(
                        EventKind::ModelError,
                        ModelError::AdapterFailure {
                            message: format!("policy_denied: {reason}"),
                        }
                        .to_string(),
                    );
                    event_log.append(
                        EventKind::RunFail,
                        format!("run_id={} outcome=error", run_id),
                    );
                    return MockRunOutput {
                        user_message: message.to_string(),
                        assistant_response: String::new(),
                        run_id: run_id.to_string(),
                        turn_id: turn_id.to_string(),
                        detected_model_tool_request: None,
                        tool_activities: vec![],
                    };
                }
                Err(ToolError::ApprovalRequired { reason }) => {
                    // Unreachable on the read-only path — internal safety mismatch.
                    event_log.append(
                        EventKind::ModelError,
                        ModelError::AdapterFailure {
                            message: format!("approval_required: {reason}"),
                        }
                        .to_string(),
                    );
                    event_log.append(
                        EventKind::RunFail,
                        format!("run_id={} outcome=error", run_id),
                    );
                    return MockRunOutput {
                        user_message: message.to_string(),
                        assistant_response: String::new(),
                        run_id: run_id.to_string(),
                        turn_id: turn_id.to_string(),
                        detected_model_tool_request: None,
                        tool_activities: vec![],
                    };
                }
                Err(other) => {
                    // Tool execution error (e.g. NotFound): record as error result and
                    // continue to the follow-up model call.
                    let r = ModelToolResult {
                        tool_call_id: call.id.clone(),
                        name: call.name.clone(),
                        content: format_tool_error_for_model(&other),
                        is_error: true,
                    };
                    (r, false)
                }
            };

            let activity = ModelToolActivity {
                name: activity_name,
                path: activity_path,
                succeeded: tool_succeeded,
            };

            // Build the follow-up request: reuse the cloned base request, pass NO tools,
            // and include the prior exchange so the model sees the tool result.
            let exchange = ModelToolExchange {
                call: call.clone(),
                result,
            };
            let follow_up = ModelStepRequest {
                request: request_clone,
                tools: vec![],
                prior_tool_exchanges: vec![exchange],
            };

            match gateway.complete_step(follow_up) {
                Err(e) => {
                    // Second call adapter/wire failure: NO second ModelRoute.
                    event_log.append(EventKind::ModelError, e.to_string());
                    event_log.append(
                        EventKind::RunFail,
                        format!("run_id={} outcome=error", run_id),
                    );
                    MockRunOutput {
                        user_message: message.to_string(),
                        assistant_response: String::new(),
                        run_id: run_id.to_string(),
                        turn_id: turn_id.to_string(),
                        detected_model_tool_request: None,
                        tool_activities: vec![activity],
                    }
                }
                Ok(ModelStepResponse {
                    route: route2,
                    output: ModelStepOutput::ToolCall { .. },
                }) => {
                    // Second call returned another tool call: hard bound — reject it.
                    event_log.append(EventKind::ModelRoute, route2.detail());
                    event_log.append(
                        EventKind::ModelError,
                        ModelError::AdapterFailure {
                            message:
                                "second_tool_call_not_supported: model issued a second tool call"
                                    .to_string(),
                        }
                        .to_string(),
                    );
                    event_log.append(
                        EventKind::RunFail,
                        format!("run_id={} outcome=error", run_id),
                    );
                    MockRunOutput {
                        user_message: message.to_string(),
                        assistant_response: String::new(),
                        run_id: run_id.to_string(),
                        turn_id: turn_id.to_string(),
                        detected_model_tool_request: None,
                        tool_activities: vec![activity],
                    }
                }
                Ok(ModelStepResponse {
                    route: route2,
                    output: ModelStepOutput::Assistant(out),
                }) => {
                    // Second call succeeded: emit second ModelRoute then success events.
                    event_log.append(EventKind::ModelRoute, route2.detail());
                    for chunk in &out.chunks {
                        event_log.append(
                            EventKind::ModelOutputChunk,
                            format!("run_id={} turn_id={} text=\"{}\"", run_id, turn_id, chunk),
                        );
                    }
                    event_log.append(EventKind::AssistantMessage, out.response.clone());
                    // Aggregate usage from both calls independently (do NOT recompute total
                    // as prompt + completion — each field is summed separately).
                    if let Some(aggregated) = aggregate_usage(usage1, out.usage) {
                        emit_usage(event_log, &aggregated);
                    }
                    event_log.append(
                        EventKind::RunComplete,
                        format!("run_id={} outcome=ok", run_id),
                    );
                    MockRunOutput {
                        user_message: message.to_string(),
                        assistant_response: out.response,
                        run_id: run_id.to_string(),
                        turn_id: turn_id.to_string(),
                        detected_model_tool_request: None,
                        tool_activities: vec![activity],
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
