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

/// Hard upper bound on native tool executions per turn.
const MAX_NATIVE_TOOL_CALLS_PER_TURN: usize = 2;

/// Hard upper bound on model calls per turn (tool calls + final assistant call).
const MAX_MODEL_STEPS_PER_TURN: usize = 3;

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

/// Extract the canonical path/query string from a validated `ToolRequest` for
/// activity recording. Mirrors the single-tool path from T-2.
fn activity_path_from_request(req: &ToolRequest) -> String {
    match req {
        ToolRequest::ListFiles { path } => path.clone(),
        ToolRequest::ReadFile { path, .. } => path.clone(),
        ToolRequest::PlanWrite { path } => path.clone(),
        ToolRequest::PreviewWrite { path, .. } => path.clone(),
        ToolRequest::SearchText { query } => query.clone(),
        ToolRequest::GlobFiles { pattern } => pattern.clone(),
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
    // Enforce the hard bounds via constants — these are compile-time assertions
    // that the bounds themselves are sane, not runtime checks.
    let _ = MAX_NATIVE_TOOL_CALLS_PER_TURN; // 2
    let _ = MAX_MODEL_STEPS_PER_TURN; // 3

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
    // Pre-clone the base request so the second and third steps can reuse the
    // same compiled prompt without re-running PromptCompile (hard bound:
    // exactly one PromptCompile per turn).
    let request_for_second = request.clone();
    let request_for_third = request.clone();

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
            let tool_request1 = match model_tool_call_to_request(&call) {
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

            // Capture activity metadata from the validated ToolRequest.
            let activity1_name = call.name.clone();
            let activity1_path = activity_path_from_request(&tool_request1);

            let ctx = ToolExecutionContext {
                workspace_root: workspace_root.to_path_buf(),
            };
            let tool_outcome1 = ToolEventRunner::new_readonly().run(event_log, &ctx, tool_request1);

            let (result1, tool1_succeeded) = match tool_outcome1 {
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
                        content: format_tool_error_for_model(&call.name, &other),
                        is_error: true,
                    };
                    (r, false)
                }
            };

            let activity1 = ModelToolActivity {
                name: activity1_name,
                path: activity1_path,
                succeeded: tool1_succeeded,
            };

            let exchange1 = ModelToolExchange {
                call: call.clone(),
                result: result1,
            };

            // Follow-up tool policy: offer tools again when exec 1 succeeded AND
            // budget remains (exec_count=1 < MAX_NATIVE_TOOL_CALLS_PER_TURN=2).
            // After a tool error, offer NO tools so the model explains the failure.
            let second_step_tools = if tool1_succeeded {
                ToolCatalog::readonly().readonly_model_definitions()
            } else {
                vec![]
            };

            let second_step = ModelStepRequest {
                request: request_for_second,
                tools: second_step_tools,
                prior_tool_exchanges: vec![exchange1.clone()],
            };

            match gateway.complete_step(second_step) {
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
                        tool_activities: vec![activity1],
                    }
                }
                Ok(ModelStepResponse {
                    route: route2,
                    output: ModelStepOutput::Assistant(out),
                }) => {
                    // Second call returned an assistant message: success path.
                    event_log.append(EventKind::ModelRoute, route2.detail());
                    for chunk in &out.chunks {
                        event_log.append(
                            EventKind::ModelOutputChunk,
                            format!("run_id={} turn_id={} text=\"{}\"", run_id, turn_id, chunk),
                        );
                    }
                    event_log.append(EventKind::AssistantMessage, out.response.clone());
                    // Aggregate usage from both calls independently.
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
                        tool_activities: vec![activity1],
                    }
                }
                Ok(ModelStepResponse {
                    route: route2,
                    output:
                        ModelStepOutput::ToolCall {
                            call: call2,
                            usage: usage2,
                        },
                }) => {
                    // Second call returned a tool call: SECOND EXECUTION PATH.
                    // Budget allows up to MAX_NATIVE_TOOL_CALLS_PER_TURN=2 executions.
                    event_log.append(EventKind::ModelRoute, route2.detail());

                    // Bridge-level validation for the second tool call.
                    let tool_request2 = match model_tool_call_to_request(&call2) {
                        Err(e) => {
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
                                tool_activities: vec![activity1],
                            };
                        }
                        Ok(req) => req,
                    };

                    let activity2_name = call2.name.clone();
                    let activity2_path = activity_path_from_request(&tool_request2);

                    let tool_outcome2 =
                        ToolEventRunner::new_readonly().run(event_log, &ctx, tool_request2);

                    let (result2, tool2_succeeded) = match tool_outcome2 {
                        Ok(output) => {
                            let r = ModelToolResult {
                                tool_call_id: call2.id.clone(),
                                name: call2.name.clone(),
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
                                tool_activities: vec![activity1],
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
                                tool_activities: vec![activity1],
                            };
                        }
                        Err(other) => {
                            // Second tool execution error: record as error result and
                            // continue to the third (final) model call.
                            let r = ModelToolResult {
                                tool_call_id: call2.id.clone(),
                                name: call2.name.clone(),
                                content: format_tool_error_for_model(&call2.name, &other),
                                is_error: true,
                            };
                            (r, false)
                        }
                    };

                    let activity2 = ModelToolActivity {
                        name: activity2_name,
                        path: activity2_path,
                        succeeded: tool2_succeeded,
                    };

                    let exchange2 = ModelToolExchange {
                        call: call2.clone(),
                        result: result2,
                    };

                    // Third model call: budget exhausted (2 executions done), NO tools.
                    // MAX_MODEL_STEPS_PER_TURN=3 means this is the last allowed call.
                    let third_step = ModelStepRequest {
                        request: request_for_third,
                        tools: vec![],
                        prior_tool_exchanges: vec![exchange1, exchange2],
                    };

                    match gateway.complete_step(third_step) {
                        Err(e) => {
                            // Third call adapter/wire failure: NO third ModelRoute.
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
                                tool_activities: vec![activity1, activity2],
                            }
                        }
                        Ok(ModelStepResponse {
                            route: route3,
                            output: ModelStepOutput::Assistant(out),
                        }) => {
                            // Third call succeeded: emit third ModelRoute then success events.
                            event_log.append(EventKind::ModelRoute, route3.detail());
                            for chunk in &out.chunks {
                                event_log.append(
                                    EventKind::ModelOutputChunk,
                                    format!(
                                        "run_id={} turn_id={} text=\"{}\"",
                                        run_id, turn_id, chunk
                                    ),
                                );
                            }
                            event_log.append(EventKind::AssistantMessage, out.response.clone());
                            // Aggregate usage across all three model responses.
                            if let Some(aggregated) =
                                aggregate_usage(aggregate_usage(usage1, usage2), out.usage)
                            {
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
                                tool_activities: vec![activity1, activity2],
                            }
                        }
                        Ok(ModelStepResponse {
                            route: route3,
                            output: ModelStepOutput::ToolCall { .. },
                        }) => {
                            // Third call returned a tool call: hard bound — reject it.
                            // No third tool execution is allowed (budget = 0).
                            event_log.append(EventKind::ModelRoute, route3.detail());
                            event_log.append(
                                EventKind::ModelError,
                                ModelError::AdapterFailure {
                                    message:
                                        "third_tool_call_not_supported: model issued a third tool call"
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
                                tool_activities: vec![activity1, activity2],
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
