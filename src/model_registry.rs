use crate::model::{MockModelAdapter, ModelAdapter, ModelError, ModelOutput, ModelRequest};
use crate::model_config::ModelProfile;
use crate::model_openai_compatible::OpenAICompatibleAdapter;
use crate::model_types::ModelAdapterKind;

pub struct ModelAdapterRegistry {
    mock: MockModelAdapter,
    openai_compatible: OpenAICompatibleAdapter,
}

impl Default for ModelAdapterRegistry {
    fn default() -> Self {
        ModelAdapterRegistry {
            mock: MockModelAdapter,
            openai_compatible: OpenAICompatibleAdapter::default(),
        }
    }
}

impl ModelAdapterRegistry {
    pub fn complete(
        &self,
        profile: &ModelProfile,
        request: &ModelRequest,
    ) -> Result<ModelOutput, ModelError> {
        match profile.adapter {
            ModelAdapterKind::MockModelAdapter => self.mock.complete(request),
            ModelAdapterKind::OpenAICompatibleAdapter => self.openai_compatible.complete(request),
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
                "OpenAI-compatible adapter is a skeleton in this POC"
            );
        }
    }
}
