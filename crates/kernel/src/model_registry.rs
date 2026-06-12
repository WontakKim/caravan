use crate::model::{
    MockModelAdapter, ModelAdapter, ModelAdapterContext, ModelError, ModelOutput, ModelRequest,
};
use crate::model_config::ModelProfile;
use crate::model_openai_compatible::OpenAICompatibleAdapter;
use crate::model_openai_config::OpenAICompatibleConfig;
use crate::model_openai_http::{
    BlockingOpenAIHttpClient, OpenAIHttpClient, OpenAIHttpClientKind, StubOpenAIHttpClient,
};
use crate::model_types::ModelAdapterKind;

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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_config::ModelConfig;
    use crate::model_openai_http::OpenAIHttpResult;
    use crate::model_openai_request::OpenAIRequestPlan;
    use crate::model_openai_types::{OpenAIChatChoice, OpenAIChatMessage, OpenAIChatResponse};
    use crate::model_types::ModelProvider;

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
                        content: "Hello from fake OpenAI".to_string(),
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
            output.tokens,
            vec!["Mock", "response", "for:", "hello", "caravan"]
        );
    }

    #[test]
    fn complete_openai_compatible_returns_adapter_failure() {
        let registry = ModelAdapterRegistry::default();
        let profile = ModelProfile {
            provider: ModelProvider::OpenAICompatible,
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
            provider: ModelProvider::OpenAICompatible,
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
        use crate::model_openai_http::OpenAIHttpClientKind;

        let registry = ModelAdapterRegistry::from_openai_runtime(
            OpenAICompatibleConfig::default(),
            OpenAIHttpClientKind::Stub,
        );
        let profile = ModelProfile {
            provider: ModelProvider::OpenAICompatible,
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
        use crate::model_openai_http::OpenAIHttpClientKind;

        let config = OpenAICompatibleConfig {
            base_url: "https://api.openai.com/v1".into(),
            api_key_env: "CARAVAN_TEST_MISSING_OPENAI_KEY_SHOULD_NOT_EXIST".into(),
            timeout_secs: 30,
        };
        let registry =
            ModelAdapterRegistry::from_openai_runtime(config, OpenAIHttpClientKind::Blocking);
        let profile = ModelProfile {
            provider: ModelProvider::OpenAICompatible,
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
}
