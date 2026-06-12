use crate::model::{
    MockModelAdapter, ModelAdapter, ModelAdapterContext, ModelError, ModelOutput, ModelRequest,
};
use crate::model_config::ModelProfile;
use crate::model_openai_compatible::OpenAICompatibleAdapter;
use crate::model_openai_config::OpenAICompatibleConfig;
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
        Self {
            mock: MockModelAdapter,
            openai_compatible: OpenAICompatibleAdapter::new(openai_config),
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
    use crate::model_types::ModelProvider;

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
}
