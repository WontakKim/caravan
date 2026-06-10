use crate::model::{ModelAdapter, ModelError, ModelOutput, ModelRequest, ModelResult};

pub struct OpenAICompatibleAdapter;

impl ModelAdapter for OpenAICompatibleAdapter {
    fn complete(&self, _request: &ModelRequest) -> ModelResult<ModelOutput> {
        Err(ModelError::AdapterFailure {
            message: "OpenAI-compatible adapter is a skeleton in this POC".to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_returns_err() {
        let request = ModelRequest {
            prompt: "any prompt".into(),
            user_message: "hello".into(),
        };
        assert!(OpenAICompatibleAdapter.complete(&request).is_err());
    }

    #[test]
    fn complete_returns_adapter_failure_variant() {
        let request = ModelRequest {
            prompt: "any prompt".into(),
            user_message: "hello".into(),
        };
        let result = OpenAICompatibleAdapter.complete(&request);
        assert!(matches!(result, Err(ModelError::AdapterFailure { .. })));
    }

    #[test]
    fn complete_returns_exact_d1_message() {
        let request = ModelRequest {
            prompt: "any prompt".into(),
            user_message: "hello".into(),
        };
        if let Err(ModelError::AdapterFailure { message }) =
            OpenAICompatibleAdapter.complete(&request)
        {
            assert_eq!(
                message,
                "OpenAI-compatible adapter is a skeleton in this POC"
            );
            assert!(!message.is_empty());
        } else {
            panic!("expected AdapterFailure");
        }
    }
}
