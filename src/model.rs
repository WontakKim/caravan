pub struct ModelOutput {
    pub response: String,
    pub tokens: Vec<String>,
}

pub trait ModelAdapter {
    fn complete(&self, prompt: &str, user_message: &str) -> ModelOutput;
}

pub struct MockModelAdapter;

impl ModelAdapter for MockModelAdapter {
    fn complete(&self, _prompt: &str, user_message: &str) -> ModelOutput {
        let response = format!("Mock response for: {}", user_message);
        let tokens = response.split_whitespace().map(str::to_string).collect();
        ModelOutput { response, tokens }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_adapter_response_single_word() {
        assert_eq!(
            MockModelAdapter.complete("any prompt", "hello").response,
            "Mock response for: hello"
        );
    }

    #[test]
    fn mock_adapter_response_and_tokens_multi_word() {
        let output = MockModelAdapter.complete("any prompt", "hello caravan");
        assert_eq!(output.response, "Mock response for: hello caravan");
        assert_eq!(
            output.tokens,
            vec!["Mock", "response", "for:", "hello", "caravan"]
        );
    }

    #[test]
    fn mock_adapter_token_count_matches_response_whitespace_split() {
        let output = MockModelAdapter.complete("any prompt", "hello");
        assert_eq!(
            output.tokens.len(),
            output.response.split_whitespace().count()
        );
    }
}
