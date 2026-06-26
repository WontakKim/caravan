use super::config::OpenAICompatibleConfig;
use super::http::{OpenAIHttpClient, StubOpenAIHttpClient};
use super::request::OpenAIRequestBuilder;
use crate::model::{
    ModelAdapter, ModelAdapterContext, ModelError, ModelOutput, ModelRequest, ModelResult,
};

pub struct OpenAICompatibleAdapter {
    config: OpenAICompatibleConfig,
    http_client: Box<dyn OpenAIHttpClient>,
}

impl OpenAICompatibleAdapter {
    pub fn new(config: OpenAICompatibleConfig) -> Self {
        Self::with_http_client(config, Box::new(StubOpenAIHttpClient::default()))
    }

    pub fn with_http_client(
        config: OpenAICompatibleConfig,
        http_client: Box<dyn OpenAIHttpClient>,
    ) -> Self {
        Self {
            config,
            http_client,
        }
    }

    pub fn config(&self) -> &OpenAICompatibleConfig {
        &self.config
    }
}

impl Default for OpenAICompatibleAdapter {
    fn default() -> Self {
        Self::new(OpenAICompatibleConfig::default())
    }
}

impl ModelAdapter for OpenAICompatibleAdapter {
    fn complete(
        &self,
        context: &ModelAdapterContext,
        request: &ModelRequest,
    ) -> ModelResult<ModelOutput> {
        let plan = OpenAIRequestBuilder::build(&self.config, &context.model, request);
        match self.http_client.send_chat_completion(&plan) {
            Ok(response) => response.to_model_output(),
            Err(err) => Err(ModelError::AdapterFailure {
                message: err.message().to_string(),
            }),
        }
    }

    fn complete_step(
        &self,
        context: &ModelAdapterContext,
        request: &crate::model::tool_use::ModelStepRequest,
    ) -> ModelResult<crate::model::tool_use::ModelStepOutput> {
        let plan = OpenAIRequestBuilder::build_step(&self.config, &context.model, request);
        match self.http_client.send_chat_completion(&plan) {
            Ok(response) => response.to_model_step_output(),
            Err(err) => Err(ModelError::AdapterFailure {
                message: err.message(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::openai::http::OpenAIHttpResult;
    use crate::model::openai::request::OpenAIRequestPlan;
    use crate::model::openai::types::{
        OpenAIChatChoice, OpenAIChatMessage, OpenAIChatResponse, OpenAIToolCall,
        OpenAIToolCallFunction,
    };
    use crate::model::tool_use::{ModelStepRequest, ModelToolDefinition};
    use crate::model::types::{ModelAdapterKind, ModelProvider};

    struct FakeSuccessClient;

    impl OpenAIHttpClient for FakeSuccessClient {
        fn send_chat_completion(
            &self,
            _plan: &OpenAIRequestPlan,
        ) -> OpenAIHttpResult<OpenAIChatResponse> {
            Ok(OpenAIChatResponse {
                choices: vec![OpenAIChatChoice {
                    message: OpenAIChatMessage {
                        role: "assistant".to_string(),
                        content: Some("Hello from fake OpenAI".to_string()),
                        tool_calls: None,
                        tool_call_id: None,
                    },
                }],
                usage: None,
            })
        }
    }

    #[test]
    fn complete_returns_err() {
        let request = ModelRequest {
            prompt: "any prompt".into(),
            user_message: "hello".into(),
        };
        assert!(
            OpenAICompatibleAdapter::default()
                .complete(
                    &ModelAdapterContext {
                        provider: ModelProvider::OpenAI,
                        model: "gpt-4o".into(),
                        adapter: ModelAdapterKind::OpenAICompatibleAdapter,
                    },
                    &request,
                )
                .is_err()
        );
    }

    #[test]
    fn complete_returns_adapter_failure_variant() {
        let request = ModelRequest {
            prompt: "any prompt".into(),
            user_message: "hello".into(),
        };
        let result = OpenAICompatibleAdapter::default().complete(
            &ModelAdapterContext {
                provider: ModelProvider::OpenAI,
                model: "gpt-4o".into(),
                adapter: ModelAdapterKind::OpenAICompatibleAdapter,
            },
            &request,
        );
        assert!(matches!(result, Err(ModelError::AdapterFailure { .. })));
    }

    #[test]
    fn complete_returns_exact_http_client_skeleton_message() {
        let request = ModelRequest {
            prompt: "any prompt".into(),
            user_message: "hello".into(),
        };
        if let Err(ModelError::AdapterFailure { message }) = OpenAICompatibleAdapter::default()
            .complete(
                &ModelAdapterContext {
                    provider: ModelProvider::OpenAI,
                    model: "gpt-4o".into(),
                    adapter: ModelAdapterKind::OpenAICompatibleAdapter,
                },
                &request,
            )
        {
            assert_eq!(
                message,
                "OpenAI-compatible HTTP client is a skeleton in this POC"
            );
            assert!(!message.is_empty());
        } else {
            panic!("expected AdapterFailure");
        }
    }

    #[test]
    fn default_adapter_uses_default_config() {
        assert!(*OpenAICompatibleAdapter::default().config() == OpenAICompatibleConfig::default());
    }

    #[test]
    fn new_adapter_stores_custom_config() {
        let custom = OpenAICompatibleConfig {
            base_url: "https://example.com/v1".to_string(),
            api_key_env: "CUSTOM_KEY".to_string(),
            timeout_secs: 5,
        };
        let adapter = OpenAICompatibleAdapter::new(custom.clone());
        assert_eq!(*adapter.config(), custom);
    }

    fn fake_success_context() -> ModelAdapterContext {
        ModelAdapterContext {
            provider: ModelProvider::OpenAI,
            model: "gpt-4o".into(),
            adapter: ModelAdapterKind::OpenAICompatibleAdapter,
        }
    }

    fn fake_success_request() -> ModelRequest {
        ModelRequest {
            prompt: "any prompt".into(),
            user_message: "hello".into(),
        }
    }

    #[test]
    fn complete_with_fake_client_returns_ok() {
        let adapter = OpenAICompatibleAdapter::with_http_client(
            OpenAICompatibleConfig::default(),
            Box::new(FakeSuccessClient),
        );
        let result = adapter.complete(&fake_success_context(), &fake_success_request());
        assert!(result.is_ok());
    }

    #[test]
    fn complete_with_fake_client_response_equals_hello_from_fake_openai() {
        let adapter = OpenAICompatibleAdapter::with_http_client(
            OpenAICompatibleConfig::default(),
            Box::new(FakeSuccessClient),
        );
        let output = adapter
            .complete(&fake_success_context(), &fake_success_request())
            .unwrap();
        assert_eq!(output.response, "Hello from fake OpenAI");
    }

    #[test]
    fn complete_with_fake_client_chunks_equal_split_whitespace() {
        let adapter = OpenAICompatibleAdapter::with_http_client(
            OpenAICompatibleConfig::default(),
            Box::new(FakeSuccessClient),
        );
        let output = adapter
            .complete(&fake_success_context(), &fake_success_request())
            .unwrap();
        assert_eq!(output.chunks, vec!["Hello", "from", "fake", "OpenAI"]);
    }

    // --- complete_step fake clients ---

    struct FakeStepAssistantClient;

    impl OpenAIHttpClient for FakeStepAssistantClient {
        fn send_chat_completion(
            &self,
            _plan: &OpenAIRequestPlan,
        ) -> OpenAIHttpResult<OpenAIChatResponse> {
            Ok(OpenAIChatResponse {
                choices: vec![OpenAIChatChoice {
                    message: OpenAIChatMessage {
                        role: "assistant".to_string(),
                        content: Some("step assistant response".to_string()),
                        tool_calls: None,
                        tool_call_id: None,
                    },
                }],
                usage: None,
            })
        }
    }

    struct FakeStepToolCallClient;

    impl OpenAIHttpClient for FakeStepToolCallClient {
        fn send_chat_completion(
            &self,
            _plan: &OpenAIRequestPlan,
        ) -> OpenAIHttpResult<OpenAIChatResponse> {
            Ok(OpenAIChatResponse {
                choices: vec![OpenAIChatChoice {
                    message: OpenAIChatMessage {
                        role: "assistant".to_string(),
                        content: None,
                        tool_calls: Some(vec![OpenAIToolCall {
                            id: "call-1".to_string(),
                            kind: "function".to_string(),
                            function: OpenAIToolCallFunction {
                                name: "search".to_string(),
                                arguments: r#"{"q":"rust"}"#.to_string(),
                            },
                        }]),
                        tool_call_id: None,
                    },
                }],
                usage: None,
            })
        }
    }

    fn make_step_request() -> ModelStepRequest {
        ModelStepRequest {
            request: ModelRequest {
                prompt: "what is rust?".into(),
                user_message: "what is rust?".into(),
            },
            tools: vec![ModelToolDefinition {
                name: "search".into(),
                description: "Search the web".into(),
                input_schema: serde_json::json!({"type": "object"}),
            }],
            prior_tool_exchanges: vec![],
        }
    }

    // --- complete_step tests ---

    #[test]
    fn complete_step_with_fake_client_returning_content_returns_assistant_output() {
        let adapter = OpenAICompatibleAdapter::with_http_client(
            OpenAICompatibleConfig::default(),
            Box::new(FakeStepAssistantClient),
        );
        let step = make_step_request();
        let result = adapter.complete_step(&fake_success_context(), &step);
        match result.unwrap() {
            crate::model::tool_use::ModelStepOutput::Assistant(o) => {
                assert_eq!(o.response, "step assistant response");
            }
            _ => panic!("expected ModelStepOutput::Assistant"),
        }
    }

    #[test]
    fn complete_step_with_fake_client_returning_tool_call_returns_tool_call_output() {
        let adapter = OpenAICompatibleAdapter::with_http_client(
            OpenAICompatibleConfig::default(),
            Box::new(FakeStepToolCallClient),
        );
        let step = make_step_request();
        let result = adapter.complete_step(&fake_success_context(), &step);
        match result.unwrap() {
            crate::model::tool_use::ModelStepOutput::ToolCall { call, .. } => {
                assert_eq!(call.id, "call-1");
                assert_eq!(call.name, "search");
                assert!(call.arguments.is_object());
            }
            _ => panic!("expected ModelStepOutput::ToolCall"),
        }
    }
}
