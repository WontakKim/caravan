use crate::model_types::{ModelAdapterKind, ModelProvider};

pub struct ModelProfile {
    pub provider: ModelProvider,
    pub model: String,
    pub adapter: ModelAdapterKind,
}

pub struct ModelConfig {
    pub active_profile: ModelProfile,
}

impl Default for ModelConfig {
    fn default() -> Self {
        ModelConfig {
            active_profile: ModelProfile {
                provider: ModelProvider::Mock,
                model: "mock-model".into(),
                adapter: ModelAdapterKind::MockModelAdapter,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_profile_has_mock_provider() {
        let config = ModelConfig::default();
        assert_eq!(config.active_profile.provider, ModelProvider::Mock);
    }

    #[test]
    fn default_profile_has_mock_model() {
        let config = ModelConfig::default();
        assert_eq!(config.active_profile.model, "mock-model");
    }

    #[test]
    fn default_profile_has_mock_adapter() {
        let config = ModelConfig::default();
        assert_eq!(
            config.active_profile.adapter,
            ModelAdapterKind::MockModelAdapter
        );
    }
}
