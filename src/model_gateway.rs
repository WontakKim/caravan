use crate::model::{MockModelAdapter, ModelAdapter};

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

pub struct ModelGateway;

impl ModelGateway {
    pub fn new() -> Self {
        ModelGateway
    }

    pub fn complete(&self, request: ModelRequest) -> ModelResponse {
        let adapter = MockModelAdapter;
        let output = adapter.complete(&request.prompt, &request.user_message);
        ModelResponse {
            route: ModelRoute {
                provider: "mock".into(),
                model: "mock-model".into(),
                adapter: "MockModelAdapter".into(),
            },
            assistant_response: output.response,
            tokens: output.tokens,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_returns_expected_response_and_tokens() {
        let response = ModelGateway::new().complete(ModelRequest {
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
        let response = ModelGateway::new().complete(ModelRequest {
            prompt: "any".into(),
            user_message: "hello caravan".into(),
        });
        assert_eq!(response.route.provider, "mock");
        assert_eq!(response.route.model, "mock-model");
        assert_eq!(response.route.adapter, "MockModelAdapter");
    }

    #[test]
    fn route_detail_formats_mock_route() {
        let response = ModelGateway::new().complete(ModelRequest {
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
}
