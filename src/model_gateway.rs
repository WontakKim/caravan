use crate::model::{MockModelAdapter, ModelAdapter};
use crate::model_config::ModelConfig;

pub struct ModelRequest {
    pub prompt: String,
    pub user_message: String,
}

pub struct ModelRoute {
    pub provider: String,
    pub model: String,
    pub adapter: String,
}

impl ModelRoute {
    pub fn detail(&self) -> String {
        format!(
            "provider={} model={} adapter={}",
            self.provider, self.model, self.adapter
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
}

impl ModelGateway {
    pub fn new(config: ModelConfig) -> Self {
        ModelGateway { config }
    }

    pub fn complete(&self, request: ModelRequest) -> ModelResponse {
        // Mock-only invariant: active_profile.adapter is expected to equal
        // "MockModelAdapter" — the concrete adapter constructed here.
        // Adapter dispatch / switching / fallback are out of scope.
        let adapter = MockModelAdapter;
        let output = adapter.complete(&request.prompt, &request.user_message);
        ModelResponse {
            route: ModelRoute {
                provider: self.config.active_profile.provider.clone(),
                model: self.config.active_profile.model.clone(),
                adapter: self.config.active_profile.adapter.clone(),
            },
            assistant_response: output.response,
            tokens: output.tokens,
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
        let response = ModelGateway::default().complete(ModelRequest {
            prompt: "any".into(),
            user_message: "hello caravan".into(),
        });
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
        let response = ModelGateway::default().complete(ModelRequest {
            prompt: "any".into(),
            user_message: "hello caravan".into(),
        });
        assert_eq!(response.route.provider, "mock");
        assert_eq!(response.route.model, "mock-model");
        assert_eq!(response.route.adapter, "MockModelAdapter");
    }

    #[test]
    fn route_detail_formats_mock_route() {
        let response = ModelGateway::default().complete(ModelRequest {
            prompt: "any".into(),
            user_message: "hello caravan".into(),
        });
        assert_eq!(
            response.route.detail(),
            "provider=mock model=mock-model adapter=MockModelAdapter"
        );
    }

    #[test]
    fn route_detail_formats_arbitrary_fields() {
        let route = ModelRoute {
            provider: "p".into(),
            model: "m".into(),
            adapter: "a".into(),
        };
        assert_eq!(route.detail(), "provider=p model=m adapter=a");
    }

    #[test]
    fn default_config_route_detail_and_response_match_mock_adapter() {
        let response = ModelGateway::default().complete(ModelRequest {
            prompt: String::new(),
            user_message: String::new(),
        });
        assert_eq!(
            response.route.detail(),
            "provider=mock model=mock-model adapter=MockModelAdapter"
        );
        assert_eq!(response.assistant_response, "Mock response for: ");
    }
}
