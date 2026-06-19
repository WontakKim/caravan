#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelProvider {
    Mock,
    OpenAI,
}

impl ModelProvider {
    pub fn as_str(&self) -> &'static str {
        match self {
            ModelProvider::Mock => "mock",
            ModelProvider::OpenAI => "openai",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelAdapterKind {
    MockModelAdapter,
    OpenAICompatibleAdapter,
}

impl ModelAdapterKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ModelAdapterKind::MockModelAdapter => "MockModelAdapter",
            ModelAdapterKind::OpenAICompatibleAdapter => "OpenAICompatibleAdapter",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_provider_mock_as_str() {
        assert_eq!(ModelProvider::Mock.as_str(), "mock");
    }

    #[test]
    fn model_adapter_kind_mock_as_str() {
        assert_eq!(
            ModelAdapterKind::MockModelAdapter.as_str(),
            "MockModelAdapter"
        );
    }

    #[test]
    fn model_provider_openai_as_str() {
        assert_eq!(ModelProvider::OpenAI.as_str(), "openai");
    }

    #[test]
    fn model_adapter_kind_openai_compatible_as_str() {
        assert_eq!(
            ModelAdapterKind::OpenAICompatibleAdapter.as_str(),
            "OpenAICompatibleAdapter"
        );
    }
}
