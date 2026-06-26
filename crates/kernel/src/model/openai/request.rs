use super::config::OpenAICompatibleConfig;
use super::types::{
    OpenAIChatMessage, OpenAIChatRequest, OpenAIFunctionDefinition, OpenAIToolCall,
    OpenAIToolCallFunction, OpenAIToolDefinition,
};
use crate::model::ModelRequest;
use crate::model::tool_use::ModelStepRequest;

/// Describes what would be sent to an OpenAI-compatible endpoint.
///
/// This is a coordinator type — it is never transmitted over the network.
/// `api_key_env` holds only the environment variable NAME (e.g. `"OPENAI_API_KEY"`),
/// never a resolved secret value.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenAIRequestPlan {
    pub url: String,
    pub api_key_env: String,
    pub timeout_secs: u64,
    pub body: OpenAIChatRequest,
}

/// Assembles an [`OpenAIRequestPlan`] from config, model name, and request.
///
/// Pure assembly — no I/O, no validation, no network access.
pub struct OpenAIRequestBuilder;

impl OpenAIRequestBuilder {
    /// Build a request plan from the given config, model name, and model request.
    ///
    /// The `api_key_env` field on the returned plan holds only the variable name
    /// copied from `config` — the value is never resolved here.
    pub fn build(
        config: &OpenAICompatibleConfig,
        model: &str,
        request: &ModelRequest,
    ) -> OpenAIRequestPlan {
        OpenAIRequestPlan {
            url: config.chat_completions_url(),
            api_key_env: config.api_key_env.clone(),
            timeout_secs: config.timeout_secs,
            body: OpenAIChatRequest::from_model_request(model, request),
        }
    }

    /// Build a step request plan for a tool-enabled conversation turn.
    ///
    /// Three cases are handled:
    /// - `prior_tool_exchange` is `None` and `tools` is non-empty: initial request with
    ///   `tools` and `tool_choice="auto"`.
    /// - `prior_tool_exchange` is `None` and `tools` is empty: plain single-user-message
    ///   request; no `tools` or `tool_choice` fields are serialized.
    /// - `prior_tool_exchange` is `Some(exchange)`: follow-up with three messages
    ///   (user, assistant with tool_calls, tool result); no `tools` or `tool_choice`.
    pub fn build_step(
        config: &OpenAICompatibleConfig,
        model: &str,
        step: &ModelStepRequest,
    ) -> OpenAIRequestPlan {
        let (messages, tools, tool_choice) = match &step.prior_tool_exchange {
            None if !step.tools.is_empty() => {
                let messages = vec![OpenAIChatMessage {
                    role: "user".to_string(),
                    content: Some(step.request.prompt.clone()),
                    tool_calls: None,
                    tool_call_id: None,
                }];
                let tool_defs = step
                    .tools
                    .iter()
                    .map(|t| OpenAIToolDefinition {
                        kind: "function".to_string(),
                        function: OpenAIFunctionDefinition {
                            name: t.name.clone(),
                            description: t.description.clone(),
                            parameters: t.input_schema.clone(),
                        },
                    })
                    .collect();
                (messages, Some(tool_defs), Some("auto".to_string()))
            }
            None => {
                let messages = vec![OpenAIChatMessage {
                    role: "user".to_string(),
                    content: Some(step.request.prompt.clone()),
                    tool_calls: None,
                    tool_call_id: None,
                }];
                (messages, None, None)
            }
            Some(exchange) => {
                let messages = vec![
                    OpenAIChatMessage {
                        role: "user".to_string(),
                        content: Some(step.request.prompt.clone()),
                        tool_calls: None,
                        tool_call_id: None,
                    },
                    OpenAIChatMessage {
                        role: "assistant".to_string(),
                        content: None, // serializes as "content":null (no skip_serializing_if)
                        tool_calls: Some(vec![OpenAIToolCall {
                            id: exchange.call.id.clone(),
                            kind: "function".to_string(),
                            function: OpenAIToolCallFunction {
                                name: exchange.call.name.clone(),
                                arguments: serde_json::to_string(&exchange.call.arguments)
                                    .unwrap_or_else(|_| "{}".to_string()),
                            },
                        }]),
                        tool_call_id: None,
                    },
                    OpenAIChatMessage {
                        role: "tool".to_string(),
                        content: Some(exchange.result.content.clone()),
                        tool_calls: None,
                        tool_call_id: Some(exchange.result.tool_call_id.clone()),
                    },
                ];
                (messages, None, None)
            }
        };

        OpenAIRequestPlan {
            url: config.chat_completions_url(),
            api_key_env: config.api_key_env.clone(),
            timeout_secs: config.timeout_secs,
            body: OpenAIChatRequest {
                model: model.to_string(),
                messages,
                stream: false,
                tools,
                tool_choice,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tool_use::{
        ModelStepRequest, ModelToolCall, ModelToolDefinition, ModelToolExchange, ModelToolResult,
    };

    fn default_request() -> ModelRequest {
        ModelRequest {
            prompt: "any prompt".to_string(),
            user_message: "any message".to_string(),
        }
    }

    #[test]
    fn build_uses_default_chat_completions_url() {
        let config = OpenAICompatibleConfig::default();
        let plan = OpenAIRequestBuilder::build(&config, "gpt-4o", &default_request());
        assert_eq!(plan.url, "https://api.openai.com/v1/chat/completions");
    }

    #[test]
    fn build_does_not_duplicate_trailing_slash() {
        let config = OpenAICompatibleConfig {
            base_url: "https://example.com/v1/".to_string(),
            ..Default::default()
        };
        let plan = OpenAIRequestBuilder::build(&config, "gpt-4o", &default_request());
        assert_eq!(plan.url, "https://example.com/v1/chat/completions");
    }

    #[test]
    fn build_preserves_api_key_env() {
        let config = OpenAICompatibleConfig {
            api_key_env: "OPENAI_API_KEY".to_string(),
            ..Default::default()
        };
        let plan = OpenAIRequestBuilder::build(&config, "gpt-4o", &default_request());
        assert_eq!(plan.api_key_env, "OPENAI_API_KEY");
    }

    #[test]
    fn build_preserves_timeout_secs() {
        let config = OpenAICompatibleConfig {
            timeout_secs: 60,
            ..Default::default()
        };
        let plan = OpenAIRequestBuilder::build(&config, "gpt-4o", &default_request());
        assert_eq!(plan.timeout_secs, 60);
    }

    #[test]
    fn build_sets_body_model() {
        let config = OpenAICompatibleConfig::default();
        let plan = OpenAIRequestBuilder::build(&config, "gpt-4o-mini", &default_request());
        assert_eq!(plan.body.model, "gpt-4o-mini");
    }

    #[test]
    fn build_sets_user_role_message() {
        let config = OpenAICompatibleConfig::default();
        let plan = OpenAIRequestBuilder::build(&config, "gpt-4o", &default_request());
        assert_eq!(plan.body.messages.len(), 1);
        assert_eq!(plan.body.messages[0].role, "user");
    }

    #[test]
    fn build_maps_prompt_to_message_content() {
        let request = ModelRequest {
            prompt: "SYSTEM: be helpful\nUSER: explain recursion".to_string(),
            user_message: "explain recursion".to_string(),
        };
        let config = OpenAICompatibleConfig::default();
        let plan = OpenAIRequestBuilder::build(&config, "gpt-4o", &request);
        assert_eq!(
            plan.body.messages[0].content,
            Some("SYSTEM: be helpful\nUSER: explain recursion".to_string())
        );
    }

    #[test]
    fn build_sets_stream_false() {
        let config = OpenAICompatibleConfig::default();
        let plan = OpenAIRequestBuilder::build(&config, "gpt-4o", &default_request());
        assert!(!plan.body.stream);
    }

    #[test]
    fn plan_body_serializes_to_json() {
        let config = OpenAICompatibleConfig::default();
        let plan = OpenAIRequestBuilder::build(&config, "gpt-4o", &default_request());
        let json = serde_json::to_string(&plan.body);
        assert!(json.is_ok());
        let json_str = json.unwrap();
        assert!(json_str.contains("\"model\""));
        assert!(json_str.contains("\"messages\""));
        assert!(json_str.contains("\"stream\""));
    }

    #[test]
    fn plan_has_exactly_four_fields() {
        let config = OpenAICompatibleConfig::default();
        let plan = OpenAIRequestBuilder::build(&config, "gpt-4o", &default_request());
        let OpenAIRequestPlan {
            url,
            api_key_env,
            timeout_secs,
            body,
        } = plan;
        assert_eq!(url, "https://api.openai.com/v1/chat/completions");
        assert_eq!(api_key_env, "OPENAI_API_KEY");
        assert_eq!(timeout_secs, 30);
        assert_eq!(body.model, "gpt-4o");
    }

    // --- build_step helpers ---

    fn sample_tool_def() -> ModelToolDefinition {
        ModelToolDefinition {
            name: "search".to_string(),
            description: "Search the web".to_string(),
            input_schema: serde_json::json!({"type": "object", "properties": {"q": {"type": "string"}}}),
        }
    }

    fn step_with_tools() -> ModelStepRequest {
        ModelStepRequest {
            request: ModelRequest {
                prompt: "what is rust?".to_string(),
                user_message: "what is rust?".to_string(),
            },
            tools: vec![sample_tool_def()],
            prior_tool_exchange: None,
        }
    }

    fn step_empty_tools() -> ModelStepRequest {
        ModelStepRequest {
            request: ModelRequest {
                prompt: "hello".to_string(),
                user_message: "hello".to_string(),
            },
            tools: vec![],
            prior_tool_exchange: None,
        }
    }

    fn sample_exchange() -> ModelToolExchange {
        ModelToolExchange {
            call: ModelToolCall {
                id: "call-1".to_string(),
                name: "search".to_string(),
                arguments: serde_json::json!({"q": "rust"}),
            },
            result: ModelToolResult {
                tool_call_id: "call-1".to_string(),
                name: "search".to_string(),
                content: "Rust is a systems language.".to_string(),
                is_error: false,
            },
        }
    }

    // --- build_step: initial request with tools ---

    #[test]
    fn build_step_initial_serialized_body_contains_tools_array() {
        let config = OpenAICompatibleConfig::default();
        let step = step_with_tools();
        let plan = OpenAIRequestBuilder::build_step(&config, "gpt-4o", &step);
        let json = serde_json::to_string(&plan.body).unwrap();
        assert!(
            json.contains("\"tools\""),
            "expected \"tools\" key in json: {json}"
        );
    }

    #[test]
    fn build_step_initial_serialized_body_contains_tool_choice_auto() {
        let config = OpenAICompatibleConfig::default();
        let step = step_with_tools();
        let plan = OpenAIRequestBuilder::build_step(&config, "gpt-4o", &step);
        let json = serde_json::to_string(&plan.body).unwrap();
        assert!(
            json.contains("\"tool_choice\":\"auto\""),
            "expected tool_choice:auto in json: {json}"
        );
    }

    #[test]
    fn build_step_initial_has_single_user_message_with_prompt() {
        let config = OpenAICompatibleConfig::default();
        let step = step_with_tools();
        let plan = OpenAIRequestBuilder::build_step(&config, "gpt-4o", &step);
        assert_eq!(plan.body.messages.len(), 1);
        assert_eq!(plan.body.messages[0].role, "user");
        assert_eq!(
            plan.body.messages[0].content,
            Some("what is rust?".to_string())
        );
    }

    #[test]
    fn build_step_initial_maps_tool_definition_fields() {
        let config = OpenAICompatibleConfig::default();
        let step = step_with_tools();
        let plan = OpenAIRequestBuilder::build_step(&config, "gpt-4o", &step);
        let tools = plan.body.tools.as_ref().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].kind, "function");
        assert_eq!(tools[0].function.name, "search");
        assert_eq!(tools[0].function.description, "Search the web");
    }

    #[test]
    fn build_step_initial_stream_is_false() {
        let config = OpenAICompatibleConfig::default();
        let step = step_with_tools();
        let plan = OpenAIRequestBuilder::build_step(&config, "gpt-4o", &step);
        assert!(!plan.body.stream);
    }

    // --- build_step: empty-tools case ---

    #[test]
    fn build_step_empty_tools_emits_no_tools_field() {
        let config = OpenAICompatibleConfig::default();
        let step = step_empty_tools();
        let plan = OpenAIRequestBuilder::build_step(&config, "gpt-4o", &step);
        let json = serde_json::to_string(&plan.body).unwrap();
        assert!(
            !json.contains("\"tools\""),
            "should not contain \"tools\" when tools is empty: {json}"
        );
    }

    #[test]
    fn build_step_empty_tools_emits_no_tool_choice_field() {
        let config = OpenAICompatibleConfig::default();
        let step = step_empty_tools();
        let plan = OpenAIRequestBuilder::build_step(&config, "gpt-4o", &step);
        let json = serde_json::to_string(&plan.body).unwrap();
        assert!(
            !json.contains("\"tool_choice\""),
            "should not contain \"tool_choice\" when tools is empty: {json}"
        );
    }

    #[test]
    fn build_step_empty_tools_has_single_user_message() {
        let config = OpenAICompatibleConfig::default();
        let step = step_empty_tools();
        let plan = OpenAIRequestBuilder::build_step(&config, "gpt-4o", &step);
        assert_eq!(plan.body.messages.len(), 1);
        assert_eq!(plan.body.messages[0].role, "user");
    }

    // --- build_step: follow-up with prior_tool_exchange ---

    #[test]
    fn build_step_follow_up_produces_exactly_three_messages() {
        let config = OpenAICompatibleConfig::default();
        let mut step = step_with_tools();
        step.prior_tool_exchange = Some(sample_exchange());
        let plan = OpenAIRequestBuilder::build_step(&config, "gpt-4o", &step);
        assert_eq!(
            plan.body.messages.len(),
            3,
            "expected exactly 3 messages for follow-up"
        );
    }

    #[test]
    fn build_step_follow_up_message_roles_are_user_assistant_tool() {
        let config = OpenAICompatibleConfig::default();
        let mut step = step_with_tools();
        step.prior_tool_exchange = Some(sample_exchange());
        let plan = OpenAIRequestBuilder::build_step(&config, "gpt-4o", &step);
        assert_eq!(plan.body.messages[0].role, "user");
        assert_eq!(plan.body.messages[1].role, "assistant");
        assert_eq!(plan.body.messages[2].role, "tool");
    }

    #[test]
    fn build_step_follow_up_assistant_has_explicit_content_null() {
        let config = OpenAICompatibleConfig::default();
        let mut step = step_with_tools();
        step.prior_tool_exchange = Some(sample_exchange());
        let plan = OpenAIRequestBuilder::build_step(&config, "gpt-4o", &step);
        let json = serde_json::to_string(&plan.body).unwrap();
        assert!(
            json.contains("\"content\":null"),
            "expected explicit \"content\":null for assistant message: {json}"
        );
        assert_eq!(plan.body.messages[1].content, None);
    }

    #[test]
    fn build_step_follow_up_assistant_has_tool_calls_with_call_id() {
        let config = OpenAICompatibleConfig::default();
        let mut step = step_with_tools();
        step.prior_tool_exchange = Some(sample_exchange());
        let plan = OpenAIRequestBuilder::build_step(&config, "gpt-4o", &step);
        let assistant = &plan.body.messages[1];
        let tool_calls = assistant
            .tool_calls
            .as_ref()
            .expect("assistant must have tool_calls");
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, "call-1");
        assert_eq!(tool_calls[0].kind, "function");
        assert_eq!(tool_calls[0].function.name, "search");
    }

    #[test]
    fn build_step_follow_up_tool_message_has_tool_call_id() {
        let config = OpenAICompatibleConfig::default();
        let mut step = step_with_tools();
        step.prior_tool_exchange = Some(sample_exchange());
        let plan = OpenAIRequestBuilder::build_step(&config, "gpt-4o", &step);
        let tool_msg = &plan.body.messages[2];
        assert_eq!(tool_msg.tool_call_id, Some("call-1".to_string()));
        let json = serde_json::to_string(&plan.body).unwrap();
        assert!(
            json.contains("\"tool_call_id\":\"call-1\""),
            "expected tool_call_id in json: {json}"
        );
    }

    #[test]
    fn build_step_follow_up_omits_tools_field_in_serialized_body() {
        let config = OpenAICompatibleConfig::default();
        let mut step = step_with_tools();
        step.prior_tool_exchange = Some(sample_exchange());
        let plan = OpenAIRequestBuilder::build_step(&config, "gpt-4o", &step);
        let json = serde_json::to_string(&plan.body).unwrap();
        assert!(
            !json.contains("\"tools\""),
            "follow-up should not contain \"tools\" key: {json}"
        );
    }
}
