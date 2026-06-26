use crate::model::config::ModelProfile;
use crate::model::openai::compatible::OpenAICompatibleAdapter;
use crate::model::openai::config::OpenAICompatibleConfig;
use crate::model::openai::http::{
    BlockingOpenAIHttpClient, OpenAIHttpClient, OpenAIHttpClientKind, StubOpenAIHttpClient,
};
use crate::model::tool_use::{ModelStepOutput, ModelStepRequest};
use crate::model::types::ModelAdapterKind;
use crate::model::{
    MockModelAdapter, ModelAdapter, ModelAdapterContext, ModelError, ModelOutput, ModelRequest,
};

pub struct ModelAdapterRegistry {
    mock: MockModelAdapter,
    openai_compatible: OpenAICompatibleAdapter,
}

impl Default for ModelAdapterRegistry {
    fn default() -> Self {
        Self::new(OpenAICompatibleConfig::default())
    }
}

impl ModelAdapterRegistry {
    pub fn new(openai_config: OpenAICompatibleConfig) -> Self {
        Self::with_openai_http_client(openai_config, Box::new(StubOpenAIHttpClient::default()))
    }

    pub fn with_openai_http_client(
        openai_config: OpenAICompatibleConfig,
        http_client: Box<dyn OpenAIHttpClient>,
    ) -> Self {
        Self {
            mock: MockModelAdapter,
            openai_compatible: OpenAICompatibleAdapter::with_http_client(
                openai_config,
                http_client,
            ),
        }
    }

    pub fn from_openai_runtime(
        openai_config: OpenAICompatibleConfig,
        client_kind: OpenAIHttpClientKind,
    ) -> Self {
        match client_kind {
            OpenAIHttpClientKind::Stub => Self::new(openai_config),
            OpenAIHttpClientKind::Blocking => Self::with_openai_http_client(
                openai_config,
                Box::new(BlockingOpenAIHttpClient::default()),
            ),
        }
    }

    #[cfg(test)]
    pub fn openai_config_for_test(&self) -> &OpenAICompatibleConfig {
        self.openai_compatible.config()
    }

    pub fn complete(
        &self,
        profile: &ModelProfile,
        request: &ModelRequest,
    ) -> Result<ModelOutput, ModelError> {
        let context = ModelAdapterContext {
            provider: profile.provider,
            model: profile.model.clone(),
            adapter: profile.adapter,
        };
        match profile.adapter {
            ModelAdapterKind::MockModelAdapter => self.mock.complete(&context, request),
            ModelAdapterKind::OpenAICompatibleAdapter => {
                self.openai_compatible.complete(&context, request)
            }
        }
    }

    pub fn complete_step(
        &self,
        profile: &ModelProfile,
        request: &ModelStepRequest,
    ) -> Result<ModelStepOutput, ModelError> {
        let context = ModelAdapterContext {
            provider: profile.provider,
            model: profile.model.clone(),
            adapter: profile.adapter,
        };
        match profile.adapter {
            ModelAdapterKind::MockModelAdapter => self.mock.complete_step(&context, request),
            ModelAdapterKind::OpenAICompatibleAdapter => {
                self.openai_compatible.complete_step(&context, request)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::config::ModelConfig;
    use crate::model::openai::http::OpenAIHttpResult;
    use crate::model::openai::request::OpenAIRequestPlan;
    use crate::model::openai::types::{
        OpenAIChatChoice, OpenAIChatMessage, OpenAIChatResponse, OpenAIToolCall,
        OpenAIToolCallFunction,
    };
    use crate::model::tool_use::{ModelStepRequest, ModelToolDefinition};
    use crate::model::types::ModelProvider;

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

    struct FakeToolCallClient;

    impl OpenAIHttpClient for FakeToolCallClient {
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
                            id: "call-99".to_string(),
                            kind: "function".to_string(),
                            function: OpenAIToolCallFunction {
                                name: "list_files".to_string(),
                                arguments: r#"{"path":"."}"#.to_string(),
                            },
                        }]),
                        tool_call_id: None,
                    },
                }],
                usage: None,
            })
        }
    }

    #[test]
    fn complete_passes_mock_output_through_unchanged() {
        let registry = ModelAdapterRegistry::default();
        let profile = ModelConfig::default().active_profile;
        let request = ModelRequest {
            prompt: "any".into(),
            user_message: "hello caravan".into(),
        };
        let output = registry.complete(&profile, &request).unwrap();
        assert_eq!(output.response, "Mock response for: hello caravan");
        assert_eq!(
            output.chunks,
            vec!["Mock", "response", "for:", "hello", "caravan"]
        );
    }

    #[test]
    fn complete_openai_returns_adapter_failure() {
        let registry = ModelAdapterRegistry::default();
        let profile = ModelProfile {
            provider: ModelProvider::OpenAI,
            model: "gpt-4o".into(),
            adapter: ModelAdapterKind::OpenAICompatibleAdapter,
        };
        let request = ModelRequest {
            prompt: "any".into(),
            user_message: "hello".into(),
        };
        let result = registry.complete(&profile, &request);
        assert!(matches!(result, Err(ModelError::AdapterFailure { .. })));
        if let Err(ModelError::AdapterFailure { message }) = result {
            assert_eq!(
                message,
                "OpenAI-compatible HTTP client is a skeleton in this POC"
            );
        }
    }

    #[test]
    fn new_passes_custom_openai_config_to_adapter() {
        let custom = OpenAICompatibleConfig {
            base_url: "https://example.test/v1".into(),
            api_key_env: "CUSTOM_KEY_ENV".into(),
            timeout_secs: 99,
        };
        let registry = ModelAdapterRegistry::new(custom);
        assert_eq!(
            registry.openai_config_for_test().base_url,
            "https://example.test/v1"
        );
        assert_eq!(
            registry.openai_config_for_test().api_key_env,
            "CUSTOM_KEY_ENV"
        );
        assert_eq!(registry.openai_config_for_test().timeout_secs, 99);
    }

    #[test]
    fn default_uses_default_openai_config() {
        let registry = ModelAdapterRegistry::default();
        assert_eq!(
            registry.openai_config_for_test(),
            &OpenAICompatibleConfig::default()
        );
    }

    #[test]
    fn registry_completes_with_injected_fake_success_client() {
        let registry = ModelAdapterRegistry::with_openai_http_client(
            OpenAICompatibleConfig::default(),
            Box::new(FakeSuccessClient),
        );
        let profile = ModelProfile {
            provider: ModelProvider::OpenAI,
            model: "gpt-4o".into(),
            adapter: ModelAdapterKind::OpenAICompatibleAdapter,
        };
        let request = ModelRequest {
            prompt: "any".into(),
            user_message: "hello".into(),
        };
        let result = registry.complete(&profile, &request);
        assert!(result.is_ok());
        let output = result.unwrap();
        assert_eq!(output.response, "Hello from fake OpenAI");
    }

    #[test]
    fn from_openai_runtime_stub_returns_skeleton_error() {
        use crate::model::openai::http::OpenAIHttpClientKind;

        let registry = ModelAdapterRegistry::from_openai_runtime(
            OpenAICompatibleConfig::default(),
            OpenAIHttpClientKind::Stub,
        );
        let profile = ModelProfile {
            provider: ModelProvider::OpenAI,
            model: "gpt-4o".into(),
            adapter: ModelAdapterKind::OpenAICompatibleAdapter,
        };
        let request = ModelRequest {
            prompt: "any".into(),
            user_message: "hello".into(),
        };
        let result = registry.complete(&profile, &request);
        assert!(matches!(result, Err(ModelError::AdapterFailure { .. })));
        if let Err(ModelError::AdapterFailure { message }) = result {
            assert_eq!(
                message,
                "OpenAI-compatible HTTP client is a skeleton in this POC"
            );
        }
    }

    #[test]
    fn from_openai_runtime_blocking_missing_key_returns_missing_api_key() {
        use crate::model::openai::http::OpenAIHttpClientKind;

        let config = OpenAICompatibleConfig {
            base_url: "https://api.openai.com/v1".into(),
            api_key_env: "CARAVAN_TEST_MISSING_OPENAI_KEY_SHOULD_NOT_EXIST".into(),
            timeout_secs: 30,
        };
        let registry =
            ModelAdapterRegistry::from_openai_runtime(config, OpenAIHttpClientKind::Blocking);
        let profile = ModelProfile {
            provider: ModelProvider::OpenAI,
            model: "gpt-4o".into(),
            adapter: ModelAdapterKind::OpenAICompatibleAdapter,
        };
        let request = ModelRequest {
            prompt: "any".into(),
            user_message: "hello".into(),
        };
        let result = registry.complete(&profile, &request);
        assert!(matches!(result, Err(ModelError::AdapterFailure { .. })));
        if let Err(ModelError::AdapterFailure { message }) = result {
            assert!(
                message.contains(
                    "missing or empty API key env var: CARAVAN_TEST_MISSING_OPENAI_KEY_SHOULD_NOT_EXIST"
                ),
                "unexpected message: {message}"
            );
        }
    }

    fn make_step_request(user_message: &str) -> ModelStepRequest {
        ModelStepRequest {
            request: ModelRequest {
                prompt: "any".into(),
                user_message: user_message.into(),
            },
            tools: vec![ModelToolDefinition {
                name: "list_files".into(),
                description: "List files in a directory".into(),
                input_schema: serde_json::json!({"type": "object"}),
            }],
            prior_tool_exchange: None,
        }
    }

    #[test]
    fn complete_step_mock_profile_returns_assistant_output() {
        let registry = ModelAdapterRegistry::default();
        let profile = ModelConfig::default().active_profile;
        let step = make_step_request("hello caravan");
        let output = registry.complete_step(&profile, &step).unwrap();
        match output {
            ModelStepOutput::Assistant(o) => {
                assert_eq!(o.response, "Mock response for: hello caravan");
            }
            ModelStepOutput::ToolCall { .. } => panic!("expected Assistant variant"),
        }
    }

    #[test]
    fn complete_step_with_fake_tool_call_client_returns_tool_call_output() {
        let registry = ModelAdapterRegistry::with_openai_http_client(
            OpenAICompatibleConfig::default(),
            Box::new(FakeToolCallClient),
        );
        let profile = ModelProfile {
            provider: ModelProvider::OpenAI,
            model: "gpt-4o".into(),
            adapter: ModelAdapterKind::OpenAICompatibleAdapter,
        };
        let step = make_step_request("list my files");
        let output = registry.complete_step(&profile, &step).unwrap();
        match output {
            ModelStepOutput::ToolCall { call, .. } => {
                assert_eq!(call.id, "call-99");
                assert_eq!(call.name, "list_files");
            }
            ModelStepOutput::Assistant(_) => panic!("expected ToolCall variant"),
        }
    }
}
