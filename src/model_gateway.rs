use crate::model::{ModelError, ModelRequest};
use crate::model_config::ModelConfig;
use crate::model_registry::ModelAdapterRegistry;
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
}
