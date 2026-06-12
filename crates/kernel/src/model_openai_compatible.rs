use crate::model::{
    ModelAdapter, ModelAdapterContext, ModelError, ModelOutput, ModelRequest, ModelResult,
};
use crate::model_openai_config::OpenAICompatibleConfig;

pub struct OpenAICompatibleAdapter {
    config: OpenAICompatibleConfig,
}

impl OpenAICompatibleAdapter {
    pub fn new(config: OpenAICompatibleConfig) -> Self {
        Self { config }
    }

    #[allow(dead_code)]
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
        _context: &ModelAdapterContext,
        _request: &ModelRequest,
    ) -> ModelResult<ModelOutput> {
        Err(ModelError::AdapterFailure {
            message: "OpenAI-compatible adapter is a skeleton in this POC".to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_types::{ModelAdapterKind, ModelProvider};

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
                        provider: ModelProvider::OpenAICompatible,
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
                provider: ModelProvider::OpenAICompatible,
                model: "gpt-4o".into(),
                adapter: ModelAdapterKind::OpenAICompatibleAdapter,
            },
            &request,
        );
        assert!(matches!(result, Err(ModelError::AdapterFailure { .. })));
    }

    #[test]
    fn complete_returns_exact_d1_message() {
        let request = ModelRequest {
            prompt: "any prompt".into(),
            user_message: "hello".into(),
        };
        if let Err(ModelError::AdapterFailure { message }) = OpenAICompatibleAdapter::default()
            .complete(
                &ModelAdapterContext {
                    provider: ModelProvider::OpenAICompatible,
                    model: "gpt-4o".into(),
                    adapter: ModelAdapterKind::OpenAICompatibleAdapter,
                },
                &request,
            )
        {
            assert_eq!(
                message,
                "OpenAI-compatible adapter is a skeleton in this POC"
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
}
