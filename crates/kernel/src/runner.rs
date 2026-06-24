use crate::events::{EventKind, EventLog, RunId, TurnId};
use crate::manual_context::ManualToolContext;
use crate::model::ModelRequest;
use crate::model_gateway::ModelGateway;

pub struct MockRunOutput {
    pub user_message: String,
    pub assistant_response: String,
    pub run_id: String,
    pub turn_id: String,
    pub detected_model_tool_request: Option<crate::model_tool_request::ModelToolRequest>,
}

pub fn run_mock_turn(
    event_log: &mut EventLog,
    message: &str,
    gateway: &ModelGateway,
    manual_tool_context: Option<&ManualToolContext>,
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
    let prompt =
        crate::prompt::compile_prompt_with_context(message, history, manual_tool_context, None);
    event_log.append(EventKind::PromptCompile, prompt.clone());
    let request = ModelRequest {
        prompt,
        user_message: message.to_string(),
    };
    match gateway.complete(request) {
        Ok(response) => {
            event_log.append(EventKind::ModelRoute, response.route.detail());
            for chunk in &response.chunks {
                event_log.append(
                    EventKind::ModelOutputChunk,
                    format!("run_id={} turn_id={} text=\"{}\"", run_id, turn_id, chunk),
                );
            }
            event_log.append(
                EventKind::AssistantMessage,
                response.assistant_response.clone(),
            );
            let detected_model_tool_request =
                crate::model_tool_request::parse_first_model_tool_request(
                    &response.assistant_response,
                );
            if let Some(req) = &detected_model_tool_request {
                event_log.append(EventKind::ModelToolRequest, req.detail());
            }
            if let Some(usage) = response.usage {
                event_log.append(
                    EventKind::ModelUsage,
                    format!(
                        "prompt_tokens={} completion_tokens={} total_tokens={}",
                        usage.prompt_tokens, usage.completion_tokens, usage.total_tokens
                    ),
                );
            }
            event_log.append(
                EventKind::RunComplete,
                format!("run_id={} outcome=ok", run_id),
            );
            MockRunOutput {
                user_message: message.to_string(),
                assistant_response: response.assistant_response,
                run_id: run_id.to_string(),
                turn_id: turn_id.to_string(),
                detected_model_tool_request,
            }
        }
        Err(err) => {
            event_log.append(EventKind::ModelError, err.to_string());
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
            }
        }
    }
}

#[cfg(test)]
mod tests;
