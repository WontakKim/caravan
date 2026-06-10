#![allow(dead_code)]
// Payload types are test-only until the OpenAI-compatible adapter wires in;
// remove this allow when complete() builds real requests.

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenAIChatResponse {
    pub choices: Vec<OpenAIChatChoice>,
    pub usage: Option<OpenAIUsage>,
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
}
