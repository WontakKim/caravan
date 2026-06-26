use crate::model::{ModelOutput, ModelRequest, ModelUsage};

/// A tool definition passed to the model so it knows what tools are available.
#[derive(Debug, Clone)]
pub struct ModelToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// A tool call the model decided to make.
#[derive(Debug, Clone)]
pub struct ModelToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// The result of executing a tool call, ready to feed back to the model.
#[derive(Debug, Clone)]
pub struct ModelToolResult {
    pub tool_call_id: String,
    pub name: String,
    pub content: String,
    pub is_error: bool,
}

/// A paired tool call and its result from a prior turn.
#[derive(Debug, Clone)]
pub struct ModelToolExchange {
    pub call: ModelToolCall,
    pub result: ModelToolResult,
}

/// A single step request: a base model request plus available tools and any
/// prior tool exchange to include in the conversation history.
pub struct ModelStepRequest {
    pub request: ModelRequest,
    pub tools: Vec<ModelToolDefinition>,
    pub prior_tool_exchange: Option<ModelToolExchange>,
}

/// The output of a single model step: either a plain assistant reply or a
/// tool call the model wants to make.
pub enum ModelStepOutput {
    Assistant(ModelOutput),
    ToolCall {
        call: ModelToolCall,
        usage: Option<ModelUsage>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::types::{ModelAdapterKind, ModelProvider};
    use crate::model::{MockModelAdapter, ModelAdapter, ModelAdapterContext, ModelRequest};

    fn mock_context() -> ModelAdapterContext {
        ModelAdapterContext {
            provider: ModelProvider::Mock,
            model: "mock-model".into(),
            adapter: ModelAdapterKind::MockModelAdapter,
        }
    }

    #[test]
    fn tool_definition_construction_and_clone() {
        let def = ModelToolDefinition {
            name: "search".into(),
            description: "Search the web".into(),
            input_schema: serde_json::json!({"type": "object"}),
        };
        let cloned = def.clone();
        assert_eq!(cloned.name, "search");
        assert_eq!(cloned.description, "Search the web");
    }

    #[test]
    fn tool_call_construction_and_clone() {
        let call = ModelToolCall {
            id: "call-1".into(),
            name: "search".into(),
            arguments: serde_json::json!({"query": "rust"}),
        };
        let cloned = call.clone();
        assert_eq!(cloned.id, "call-1");
        assert_eq!(cloned.name, "search");
    }

    #[test]
    fn tool_result_construction_and_clone() {
        let result = ModelToolResult {
            tool_call_id: "call-1".into(),
            name: "search".into(),
            content: "found results".into(),
            is_error: false,
        };
        let cloned = result.clone();
        assert_eq!(cloned.tool_call_id, "call-1");
        assert!(!cloned.is_error);
    }

    #[test]
    fn tool_exchange_construction_and_clone() {
        let call = ModelToolCall {
            id: "call-2".into(),
            name: "run".into(),
            arguments: serde_json::json!({}),
        };
        let result = ModelToolResult {
            tool_call_id: "call-2".into(),
            name: "run".into(),
            content: "ok".into(),
            is_error: false,
        };
        let exchange = ModelToolExchange {
            call: call.clone(),
            result: result.clone(),
        };
        let cloned = exchange.clone();
        assert_eq!(cloned.call.id, "call-2");
        assert_eq!(cloned.result.content, "ok");
    }

    #[test]
    fn model_request_clone_equality() {
        let req = ModelRequest {
            prompt: "system prompt".into(),
            user_message: "hello".into(),
        };
        let cloned = req.clone();
        assert_eq!(cloned.prompt, req.prompt);
        assert_eq!(cloned.user_message, req.user_message);
    }

    #[test]
    fn step_output_assistant_variant() {
        let output = ModelOutput {
            response: "hi".into(),
            chunks: vec!["hi".into()],
            usage: None,
        };
        let step_output = ModelStepOutput::Assistant(output);
        match step_output {
            ModelStepOutput::Assistant(o) => assert_eq!(o.response, "hi"),
            ModelStepOutput::ToolCall { .. } => panic!("expected Assistant variant"),
        }
    }

    #[test]
    fn step_output_tool_call_variant() {
        let call = ModelToolCall {
            id: "c1".into(),
            name: "tool".into(),
            arguments: serde_json::json!({}),
        };
        let step_output = ModelStepOutput::ToolCall { call, usage: None };
        match step_output {
            ModelStepOutput::ToolCall { call, usage } => {
                assert_eq!(call.id, "c1");
                assert!(usage.is_none());
            }
            ModelStepOutput::Assistant(_) => panic!("expected ToolCall variant"),
        }
    }

    #[test]
    fn mock_adapter_complete_step_returns_assistant() {
        let request = ModelRequest {
            prompt: "sys".into(),
            user_message: "greet".into(),
        };
        let step_request = ModelStepRequest {
            request,
            tools: vec![],
            prior_tool_exchange: None,
        };
        let result = MockModelAdapter.complete_step(&mock_context(), &step_request);
        let output = result.unwrap();
        match output {
            ModelStepOutput::Assistant(o) => {
                assert_eq!(o.response, "Mock response for: greet");
            }
            ModelStepOutput::ToolCall { .. } => panic!("expected Assistant variant"),
        }
    }
}
