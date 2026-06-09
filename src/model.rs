pub struct ModelOutput {
    pub response: String,
    pub tokens: Vec<String>,
}

pub struct ModelRequest {
    pub prompt: String,
    pub user_message: String,
}

pub trait ModelAdapter {
    fn complete(&self, request: &ModelRequest) -> ModelOutput;
}

pub struct MockModelAdapter;

impl ModelAdapter for MockModelAdapter {
    fn complete(&self, request: &ModelRequest) -> ModelOutput {
        let response = format!("Mock response for: {}", request.user_message);
        let tokens = response.split_whitespace().map(str::to_string).collect();
        ModelOutput { response, tokens }
    }
}

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
            MockModelAdapter.complete(&request).response,
            "Mock response for: hello"
        );
    }

    #[test]
    fn mock_adapter_response_and_tokens_multi_word() {
        let request = ModelRequest {
            prompt: "any prompt".into(),
            user_message: "hello caravan".into(),
        };
        let output = MockModelAdapter.complete(&request);
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
        let output = MockModelAdapter.complete(&request);
        assert_eq!(
            output.tokens.len(),
            output.response.split_whitespace().count()
        );
    }
}
