use crate::model::{ModelError, ModelOutput, ModelRequest, ModelResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenAIChatRequest {
    pub model: String,
    pub messages: Vec<OpenAIChatMessage>,
    pub stream: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenAIChatMessage {
    pub role: String,
    pub content: String,
}

impl OpenAIChatRequest {
    pub fn from_model_request(model: impl Into<String>, request: &ModelRequest) -> Self {
        Self {
            model: model.into(),
            messages: vec![OpenAIChatMessage {
                role: "user".to_string(),
                content: request.prompt.clone(),
            }],
            stream: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenAIChatResponse {
    pub choices: Vec<OpenAIChatChoice>,
    pub usage: Option<OpenAIUsage>,
}

impl OpenAIChatResponse {
    pub fn first_assistant_content(&self) -> Option<&str> {
        self.choices
            .first()
            .map(|choice| choice.message.content.as_str())
    }

    pub fn to_model_output(&self) -> ModelResult<ModelOutput> {
        let text = self
            .first_assistant_content()
            .ok_or_else(|| ModelError::AdapterFailure {
                message: "OpenAI-compatible response did not contain assistant content".to_string(),
            })?;

        Ok(ModelOutput {
            response: text.to_string(),
            tokens: text.split_whitespace().map(str::to_string).collect(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenAIChatChoice {
    pub message: OpenAIChatMessage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenAIUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_request_serializes_to_expected_json() {
        let request = OpenAIChatRequest {
            model: "mock-model".to_string(),
            messages: vec![OpenAIChatMessage {
                role: "user".to_string(),
                content: "hello caravan".to_string(),
            }],
            stream: false,
        };
        assert_eq!(
            serde_json::to_string(&request).unwrap(),
            r#"{"model":"mock-model","messages":[{"role":"user","content":"hello caravan"}],"stream":false}"#
        );
    }

    #[test]
    fn chat_response_deserializes_from_realistic_json() {
        let json = r#"{"id":"chatcmpl-abc","object":"chat.completion","created":1700000000,"model":"gpt-4o","system_fingerprint":"fp_abc","choices":[{"index":0,"message":{"role":"assistant","content":"hello caravan"},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}"#;
        let response: OpenAIChatResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.choices.len(), 1);
        assert_eq!(response.choices[0].message.role, "assistant");
        assert_eq!(response.choices[0].message.content, "hello caravan");
        let usage = response.usage.expect("usage should be Some");
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 5);
        assert_eq!(usage.total_tokens, 15);
    }

    #[test]
    fn chat_response_deserializes_without_usage() {
        let json = r#"{"choices":[{"message":{"role":"assistant","content":"hello"}}]}"#;
        let response: OpenAIChatResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.usage, None);
    }

    #[test]
    fn from_model_request_sets_model() {
        let request = ModelRequest {
            prompt: "any prompt".to_string(),
            user_message: "any message".to_string(),
        };
        let chat_request = OpenAIChatRequest::from_model_request("gpt-4o", &request);
        assert_eq!(chat_request.model, "gpt-4o");
    }

    #[test]
    fn from_model_request_builds_single_user_message() {
        let request = ModelRequest {
            prompt: "any prompt".to_string(),
            user_message: "any message".to_string(),
        };
        let chat_request = OpenAIChatRequest::from_model_request("gpt-4o", &request);
        assert_eq!(chat_request.messages.len(), 1);
        assert_eq!(chat_request.messages[0].role, "user");
    }

    #[test]
    fn from_model_request_carries_prompt_verbatim() {
        let request = ModelRequest {
            prompt: "SYSTEM: you are a helper\nUSER: hi".to_string(),
            user_message: "hi".to_string(),
        };
        let chat_request = OpenAIChatRequest::from_model_request("gpt-4o", &request);
        assert_eq!(
            chat_request.messages[0].content,
            "SYSTEM: you are a helper\nUSER: hi"
        );
    }

    #[test]
    fn from_model_request_disables_streaming() {
        let request = ModelRequest {
            prompt: "any prompt".to_string(),
            user_message: "any message".to_string(),
        };
        let chat_request = OpenAIChatRequest::from_model_request("gpt-4o", &request);
        assert!(!chat_request.stream);
    }

    #[test]
    fn first_assistant_content_returns_first_choice_content() {
        let response = OpenAIChatResponse {
            choices: vec![
                OpenAIChatChoice {
                    message: OpenAIChatMessage {
                        role: "assistant".to_string(),
                        content: "first content".to_string(),
                    },
                },
                OpenAIChatChoice {
                    message: OpenAIChatMessage {
                        role: "assistant".to_string(),
                        content: "second content".to_string(),
                    },
                },
            ],
            usage: None,
        };
        assert_eq!(response.first_assistant_content(), Some("first content"));
    }

    #[test]
    fn first_assistant_content_returns_none_when_choices_empty() {
        let response = OpenAIChatResponse {
            choices: vec![],
            usage: None,
        };
        assert_eq!(response.first_assistant_content(), None);
    }

    #[test]
    fn to_model_output_maps_response_and_tokens() {
        let response = OpenAIChatResponse {
            choices: vec![OpenAIChatChoice {
                message: OpenAIChatMessage {
                    role: "assistant".to_string(),
                    content: "Mock response for: hello caravan".to_string(),
                },
            }],
            usage: None,
        };
        let output = response.to_model_output().unwrap();
        assert_eq!(output.response, "Mock response for: hello caravan");
        assert_eq!(
            output.tokens,
            vec!["Mock", "response", "for:", "hello", "caravan"]
        );
    }

    #[test]
    fn to_model_output_errors_when_no_assistant_content() {
        let response = OpenAIChatResponse {
            choices: vec![],
            usage: None,
        };
        match response.to_model_output() {
            Err(ModelError::AdapterFailure { message }) => {
                assert_eq!(
                    message,
                    "OpenAI-compatible response did not contain assistant content"
                );
            }
            _ => panic!("expected Err(AdapterFailure), got something else"),
        }
    }
}
