use crate::model::{ModelError, ModelRequest};
use crate::model_config::ModelConfig;
#[cfg(test)]
use crate::model_openai_config::OpenAICompatibleConfig;
use crate::model_registry::ModelAdapterRegistry;
use crate::model_runtime_config::ModelRuntimeConfig;
use crate::model_types::{ModelAdapterKind, ModelProvider};

pub struct ModelRoute {
    pub provider: ModelProvider,
    pub model: String,
    pub adapter: ModelAdapterKind,
}

impl ModelRoute {
    pub fn detail(&self) -> String {
        format!(
            "provider={} model={} adapter={}",
            self.provider.as_str(),
            self.model,
            self.adapter.as_str()
        )
    }
}

pub struct ModelResponse {
    pub route: ModelRoute,
    pub assistant_response: String,
    pub tokens: Vec<String>,
}

pub struct ModelGateway {
    config: ModelConfig,
    registry: ModelAdapterRegistry,
    #[cfg(test)]
    forced_error: Option<ModelError>,
}

impl ModelGateway {
    pub fn new(config: ModelConfig) -> Self {
        ModelGateway {
            config,
            registry: ModelAdapterRegistry::default(),
            #[cfg(test)]
            forced_error: None,
        }
    }

    pub fn from_runtime_config(runtime_config: ModelRuntimeConfig) -> Self {
        Self {
            config: runtime_config.model_config,
            registry: ModelAdapterRegistry::new(runtime_config.openai_config),
            #[cfg(test)]
            forced_error: None,
        }
    }

    #[cfg(test)]
    pub fn openai_config_for_test(&self) -> &OpenAICompatibleConfig {
        self.registry.openai_config_for_test()
    }

    #[cfg(test)]
    pub fn failing_for_test(error: ModelError) -> Self {
        ModelGateway {
            config: ModelConfig::default(),
            registry: ModelAdapterRegistry::default(),
            forced_error: Some(error),
        }
    }

    pub fn complete(&self, request: ModelRequest) -> Result<ModelResponse, ModelError> {
        #[cfg(test)]
        if let Some(ref err) = self.forced_error {
            return Err(err.clone());
        }

        let profile = &self.config.active_profile;
        match self.registry.complete(profile, &request) {
            Ok(output) => Ok(ModelResponse {
                route: ModelRoute {
                    provider: profile.provider,
                    model: profile.model.clone(),
                    adapter: profile.adapter,
                },
                assistant_response: output.response,
                tokens: output.tokens,
            }),
            Err(e) => Err(e),
        }
    }
}

impl Default for ModelGateway {
    fn default() -> Self {
        Self::new(ModelConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_config::ModelProfile;

    #[test]
    fn complete_openai_compatible_profile_returns_adapter_failure() {
        let config = ModelConfig {
            active_profile: ModelProfile {
                provider: ModelProvider::OpenAICompatible,
                model: "gpt-4o".into(),
                adapter: ModelAdapterKind::OpenAICompatibleAdapter,
            },
        };
        let result = ModelGateway::new(config).complete(ModelRequest {
            prompt: "any".into(),
            user_message: "hello".into(),
        });
        match result {
            Err(ModelError::AdapterFailure { message }) => {
                assert_eq!(
                    message,
                    "OpenAI-compatible HTTP client is a skeleton in this POC"
                );
            }
            _ => panic!("expected Err(AdapterFailure)"),
        }
    }

    #[test]
    fn complete_returns_expected_response_and_tokens() {
        let response = ModelGateway::default()
            .complete(ModelRequest {
                prompt: "any".into(),
                user_message: "hello caravan".into(),
            })
            .unwrap();
        assert_eq!(
            response.assistant_response,
            "Mock response for: hello caravan"
        );
        assert_eq!(
            response.tokens,
            vec!["Mock", "response", "for:", "hello", "caravan"]
        );
    }

    #[test]
    fn complete_returns_expected_route() {
        let response = ModelGateway::default()
            .complete(ModelRequest {
                prompt: "any".into(),
                user_message: "hello caravan".into(),
            })
            .unwrap();
        assert_eq!(response.route.provider, ModelProvider::Mock);
        assert_eq!(response.route.model, "mock-model");
        assert_eq!(response.route.adapter, ModelAdapterKind::MockModelAdapter);
    }

    #[test]
    fn route_detail_formats_mock_route() {
        let response = ModelGateway::default()
            .complete(ModelRequest {
                prompt: "any".into(),
                user_message: "hello caravan".into(),
            })
            .unwrap();
        assert_eq!(
            response.route.detail(),
            "provider=mock model=mock-model adapter=MockModelAdapter"
        );
    }

    #[test]
    fn route_detail_formats_arbitrary_fields() {
        let route = ModelRoute {
            provider: ModelProvider::Mock,
            model: "m".into(),
            adapter: ModelAdapterKind::MockModelAdapter,
        };
        assert_eq!(
            route.detail(),
            "provider=mock model=m adapter=MockModelAdapter"
        );
    }

    #[test]
    fn default_config_route_detail_and_response_match_mock_adapter() {
        let response = ModelGateway::default()
            .complete(ModelRequest {
                prompt: String::new(),
                user_message: String::new(),
            })
            .unwrap();
        assert_eq!(
            response.route.detail(),
            "provider=mock model=mock-model adapter=MockModelAdapter"
        );
        assert_eq!(response.assistant_response, "Mock response for: ");
    }

    #[test]
    fn from_runtime_config_default_returns_mock_response() {
        let gateway = ModelGateway::from_runtime_config(ModelRuntimeConfig::default());
        let response = gateway
            .complete(ModelRequest {
                prompt: "any".into(),
                user_message: "hello caravan".into(),
            })
            .unwrap();
        assert_eq!(
            response.assistant_response,
            "Mock response for: hello caravan"
        );
        assert_eq!(
            response.tokens,
            vec!["Mock", "response", "for:", "hello", "caravan"]
        );
    }

    #[test]
    fn from_runtime_config_default_returns_mock_route() {
        let gateway = ModelGateway::from_runtime_config(ModelRuntimeConfig::default());
        let response = gateway
            .complete(ModelRequest {
                prompt: "any".into(),
                user_message: "hello caravan".into(),
            })
            .unwrap();
        assert_eq!(
            response.route.detail(),
            "provider=mock model=mock-model adapter=MockModelAdapter"
        );
    }

    #[test]
    fn from_runtime_config_openai_profile_returns_adapter_failure() {
        let runtime_config = ModelRuntimeConfig {
            model_config: ModelConfig {
                active_profile: ModelProfile {
                    provider: ModelProvider::OpenAICompatible,
                    model: "gpt-4o".into(),
                    adapter: ModelAdapterKind::OpenAICompatibleAdapter,
                },
            },
            openai_config: OpenAICompatibleConfig::default(),
        };
        let result = ModelGateway::from_runtime_config(runtime_config).complete(ModelRequest {
            prompt: "any".into(),
            user_message: "hello caravan".into(),
        });
        match result {
            Err(ModelError::AdapterFailure { message }) => {
                assert_eq!(
                    message,
                    "OpenAI-compatible HTTP client is a skeleton in this POC"
                );
            }
            _ => panic!("expected Err(AdapterFailure)"),
        }
    }

    #[test]
    fn from_runtime_config_passes_custom_openai_config_to_adapter() {
        let runtime_config = ModelRuntimeConfig {
            model_config: ModelConfig::default(),
            openai_config: OpenAICompatibleConfig {
                base_url: "https://example.test/v1".into(),
                api_key_env: "CUSTOM_KEY_ENV".into(),
                timeout_secs: 99,
            },
        };
        let gateway = ModelGateway::from_runtime_config(runtime_config);
        assert_eq!(
            gateway.openai_config_for_test().base_url,
            "https://example.test/v1"
        );
        assert_eq!(
            gateway.openai_config_for_test().api_key_env,
            "CUSTOM_KEY_ENV"
        );
        assert_eq!(gateway.openai_config_for_test().timeout_secs, 99);
    }
}
