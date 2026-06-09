#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelProvider {
    Mock,
}

impl ModelProvider {
    pub fn as_str(&self) -> &'static str {
        match self {
            ModelProvider::Mock => "mock",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelAdapterKind {
    MockModelAdapter,
}

impl ModelAdapterKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ModelAdapterKind::MockModelAdapter => "MockModelAdapter",
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
}
