pub struct ModelOutput {
    pub response: String,
    pub tokens: Vec<String>,
}

pub struct ModelRequest {
    pub prompt: String,
    pub user_message: String,
}

pub trait ModelAdapter {
    fn complete(&self, request: &ModelRequest) -> ModelResult<ModelOutput>;
}

pub struct MockModelAdapter;

impl ModelAdapter for MockModelAdapter {
    fn complete(&self, request: &ModelRequest) -> ModelResult<ModelOutput> {
        let response = format!("Mock response for: {}", request.user_message);
        let tokens = response.split_whitespace().map(str::to_string).collect();
        Ok(ModelOutput { response, tokens })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelError {
    UnsupportedAdapter { adapter: String },
    AdapterFailure { message: String },
}

impl ModelError {
    pub fn kind(&self) -> &'static str {
        match self {
            ModelError::UnsupportedAdapter { .. } => "unsupported_adapter",
            ModelError::AdapterFailure { .. } => "adapter_failure",
        }
    }

    pub fn message(&self) -> String {
        match self {
            ModelError::UnsupportedAdapter { adapter } => {
                format!("unsupported adapter: {adapter}")
            }
            ModelError::AdapterFailure { message } => message.clone(),
        }
    }
}

impl std::fmt::Display for ModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "kind={} message=\"{}\"", self.kind(), self.message())
    }
}

pub type ModelResult<T> = Result<T, ModelError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_adapter_response_single_word() {
        let request = ModelRequest {
            prompt: "any prompt".into(),
            user_message: "hello".into(),
        };
        assert_eq!(
            MockModelAdapter.complete(&request).unwrap().response,
            "Mock response for: hello"
        );
    }

    #[test]
    fn mock_adapter_response_and_tokens_multi_word() {
        let request = ModelRequest {
            prompt: "any prompt".into(),
            user_message: "hello caravan".into(),
        };
        let output = MockModelAdapter.complete(&request).unwrap();
        assert_eq!(output.response, "Mock response for: hello caravan");
        assert_eq!(
            output.tokens,
            vec!["Mock", "response", "for:", "hello", "caravan"]
        );
    }

    #[test]
    fn mock_adapter_token_count_matches_response_whitespace_split() {
        let request = ModelRequest {
            prompt: "any prompt".into(),
            user_message: "hello".into(),
        };
        let output = MockModelAdapter.complete(&request).unwrap();
        assert_eq!(
            output.tokens.len(),
            output.response.split_whitespace().count()
        );
    }

    #[test]
    fn model_error_display_adapter_failure() {
        let err = ModelError::AdapterFailure {
            message: "test failure".into(),
        };
        assert_eq!(err.kind(), "adapter_failure");
        assert_eq!(
            err.to_string(),
            "kind=adapter_failure message=\"test failure\""
        );
    }

    #[test]
    fn model_error_display_unsupported_adapter() {
        let err = ModelError::UnsupportedAdapter {
            adapter: "gpt-99".into(),
        };
        assert_eq!(err.kind(), "unsupported_adapter");
        assert_eq!(
            err.to_string(),
            "kind=unsupported_adapter message=\"unsupported adapter: gpt-99\""
        );
    }
}
