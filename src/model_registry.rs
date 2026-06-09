use crate::model::{MockModelAdapter, ModelAdapter, ModelOutput};
use crate::model_config::ModelProfile;
use crate::model_gateway::ModelRequest;
use crate::model_types::ModelAdapterKind;

pub struct ModelAdapterRegistry {
    mock: MockModelAdapter,
}

impl Default for ModelAdapterRegistry {
    fn default() -> Self {
        ModelAdapterRegistry {
            mock: MockModelAdapter,
        }
    }
}

impl ModelAdapterRegistry {
    // Mock-only invariant: always delegates to MockModelAdapter.
    // Adapter selection is by ModelAdapterKind; switching, fallback, and error handling are out of scope.
    pub fn complete(&self, profile: &ModelProfile, request: &ModelRequest) -> ModelOutput {
        match profile.adapter {
            ModelAdapterKind::MockModelAdapter => {
                self.mock.complete(&request.prompt, &request.user_message)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_config::ModelConfig;

    #[test]
    fn complete_passes_mock_output_through_unchanged() {
        let registry = ModelAdapterRegistry::default();
        let profile = ModelConfig::default().active_profile;
        let request = ModelRequest {
            prompt: "any".into(),
            user_message: "hello caravan".into(),
        };
        let output = registry.complete(&profile, &request);
        assert_eq!(output.response, "Mock response for: hello caravan");
        assert_eq!(
            output.tokens,
            vec!["Mock", "response", "for:", "hello", "caravan"]
        );
    }
}
