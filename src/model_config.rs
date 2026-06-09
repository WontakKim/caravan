pub struct ModelProfile {
    pub provider: String,
    pub model: String,
    pub adapter: String,
}

pub struct ModelConfig {
    pub active_profile: ModelProfile,
}

impl Default for ModelConfig {
    fn default() -> Self {
        ModelConfig {
            active_profile: ModelProfile {
                provider: "mock".into(),
                model: "mock-model".into(),
                adapter: "MockModelAdapter".into(),
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
        assert_eq!(config.active_profile.provider, "mock");
    }

    #[test]
    fn default_profile_has_mock_model() {
        let config = ModelConfig::default();
        assert_eq!(config.active_profile.model, "mock-model");
    }

    #[test]
    fn default_profile_has_mock_adapter() {
        let config = ModelConfig::default();
        assert_eq!(config.active_profile.adapter, "MockModelAdapter");
    }
}
