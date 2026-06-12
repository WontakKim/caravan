use crate::model::{
    ModelAdapter, ModelAdapterContext, ModelError, ModelOutput, ModelRequest, ModelResult,
};
use crate::model_openai_config::OpenAICompatibleConfig;
use crate::model_openai_http::{OpenAIHttpClient, StubOpenAIHttpClient};
use crate::model_openai_request::OpenAIRequestBuilder;

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
    fn complete_returns_exact_http_client_skeleton_message() {
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
}
