use crate::model::{ModelError, ModelOutput, ModelRequest, ModelResult, ModelUsage};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenAIFunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenAIToolDefinition {
    #[serde(rename = "type")]
    pub kind: String,
    pub function: OpenAIFunctionDefinition,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenAIToolCallFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenAIToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub function: OpenAIToolCallFunction,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenAIChatRequest {
    pub model: String,
    pub messages: Vec<OpenAIChatMessage>,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<OpenAIToolDefinition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenAIChatMessage {
    pub role: String,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<OpenAIToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl OpenAIChatRequest {
    pub fn from_model_request(model: impl Into<String>, request: &ModelRequest) -> Self {
        Self {
            model: model.into(),
            messages: vec![OpenAIChatMessage {
                role: "user".to_string(),
                content: Some(request.prompt.clone()),
                tool_calls: None,
                tool_call_id: None,
            }],
            stream: false,
            tools: None,
            tool_choice: None,
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
            .and_then(|choice| choice.message.content.as_deref())
    }

    pub fn to_model_output(&self) -> ModelResult<ModelOutput> {
        // Error on non-empty tool calls — this is the assistant-only completion path.
        if let Some(first_choice) = self.choices.first() {
            if let Some(tool_calls) = &first_choice.message.tool_calls {
                if !tool_calls.is_empty() {
                    return Err(ModelError::AdapterFailure {
                        message: "unexpected_tool_call: model returned a tool call on the assistant-only completion path".to_string(),
                    });
                }
            }
        }

        let text = self
            .first_assistant_content()
            .ok_or_else(|| ModelError::AdapterFailure {
                message: "OpenAI-compatible response did not contain assistant content".to_string(),
            })?;

        let usage = self.usage.as_ref().map(|u| ModelUsage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            total_tokens: u.total_tokens,
        });
        Ok(ModelOutput {
            response: text.to_string(),
            chunks: text.split_whitespace().map(str::to_string).collect(),
            usage,
        })
    }

    pub fn to_model_step_output(&self) -> ModelResult<crate::model::tool_use::ModelStepOutput> {
        use crate::model::tool_use::{ModelStepOutput, ModelToolCall};

        let usage = self.usage.as_ref().map(|u| ModelUsage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            total_tokens: u.total_tokens,
        });

        let first_message = match self.choices.first() {
            Some(c) => &c.message,
            None => {
                return Err(ModelError::AdapterFailure {
                    message: "empty model response: no content and no tool calls".to_string(),
                });
            }
        };

        // Treat Some(vec![]) the same as None (zero tool calls).
        let effective_tool_calls: Option<&[OpenAIToolCall]> = match &first_message.tool_calls {
            None => None,
            Some(v) if v.is_empty() => None,
            Some(v) => Some(v),
        };

        match effective_tool_calls {
            Some(tool_calls) if tool_calls.len() >= 2 => Err(ModelError::AdapterFailure {
                message:
                    "multiple_tool_calls_not_supported: model returned more than one tool call"
                        .to_string(),
            }),
            Some(tool_calls) => {
                // Exactly one tool call — validate wire shape.
                let tc = &tool_calls[0];
                if tc.kind != "function" || tc.id.is_empty() || tc.function.name.is_empty() {
                    return Err(ModelError::AdapterFailure {
                        message: "malformed_tool_arguments: invalid tool call wire shape"
                            .to_string(),
                    });
                }
                let arguments = serde_json::from_str::<serde_json::Value>(&tc.function.arguments)
                    .map_err(|_| ModelError::AdapterFailure {
                    message: "malformed_tool_arguments: arguments is not valid JSON".to_string(),
                })?;
                if !arguments.is_object() {
                    return Err(ModelError::AdapterFailure {
                        message: "malformed_tool_arguments: arguments must be a JSON object"
                            .to_string(),
                    });
                }
                Ok(ModelStepOutput::ToolCall {
                    call: ModelToolCall {
                        id: tc.id.clone(),
                        name: tc.function.name.clone(),
                        arguments,
                    },
                    usage,
                })
            }
            None => {
                // Zero tool calls — require non-empty content.
                let content = first_message.content.as_deref().unwrap_or("");
                if content.is_empty() {
                    return Err(ModelError::AdapterFailure {
                        message: "empty model response: no content and no tool calls".to_string(),
                    });
                }
                Ok(ModelStepOutput::Assistant(ModelOutput {
                    response: content.to_string(),
                    chunks: content.split_whitespace().map(str::to_string).collect(),
                    usage,
                }))
            }
        }
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
                content: Some("hello caravan".to_string()),
                tool_calls: None,
                tool_call_id: None,
            }],
            stream: false,
            tools: None,
            tool_choice: None,
        };
        assert_eq!(
            serde_json::to_string(&request).unwrap(),
            r#"{"model":"mock-model","messages":[{"role":"user","content":"hello caravan"}],"stream":false}"#
        );
    }

    #[test]
    fn chat_request_with_tools_serializes_tools_and_tool_choice() {
        let request = OpenAIChatRequest {
            model: "mock-model".to_string(),
            messages: vec![],
            stream: false,
            tools: Some(vec![OpenAIToolDefinition {
                kind: "function".to_string(),
                function: OpenAIFunctionDefinition {
                    name: "search".to_string(),
                    description: "Search the web".to_string(),
                    parameters: serde_json::json!({"type": "object"}),
                },
            }]),
            tool_choice: Some("auto".to_string()),
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"tools\""));
        assert!(json.contains("\"tool_choice\":\"auto\""));
    }

    #[test]
    fn message_with_content_none_serializes_as_explicit_null() {
        let msg = OpenAIChatMessage {
            role: "assistant".to_string(),
            content: None,
            tool_calls: Some(vec![OpenAIToolCall {
                id: "call-1".to_string(),
                kind: "function".to_string(),
                function: OpenAIToolCallFunction {
                    name: "search".to_string(),
                    arguments: r#"{"q":"hello"}"#.to_string(),
                },
            }]),
            tool_call_id: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"content\":null"));
    }

    #[test]
    fn chat_response_deserializes_from_realistic_json() {
        let json = r#"{"id":"chatcmpl-abc","object":"chat.completion","created":1700000000,"model":"gpt-4o","system_fingerprint":"fp_abc","choices":[{"index":0,"message":{"role":"assistant","content":"hello caravan"},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}"#;
        let response: OpenAIChatResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.choices.len(), 1);
        assert_eq!(response.choices[0].message.role, "assistant");
        assert_eq!(
            response.choices[0].message.content,
            Some("hello caravan".to_string())
        );
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
            Some("SYSTEM: you are a helper\nUSER: hi".to_string())
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
                        content: Some("first content".to_string()),
                        tool_calls: None,
                        tool_call_id: None,
                    },
                },
                OpenAIChatChoice {
                    message: OpenAIChatMessage {
                        role: "assistant".to_string(),
                        content: Some("second content".to_string()),
                        tool_calls: None,
                        tool_call_id: None,
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
    fn to_model_output_maps_response_and_chunks() {
        let response = OpenAIChatResponse {
            choices: vec![OpenAIChatChoice {
                message: OpenAIChatMessage {
                    role: "assistant".to_string(),
                    content: Some("Mock response for: hello caravan".to_string()),
                    tool_calls: None,
                    tool_call_id: None,
                },
            }],
            usage: None,
        };
        let output = response.to_model_output().unwrap();
        assert_eq!(output.response, "Mock response for: hello caravan");
        assert_eq!(
            output.chunks,
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

    #[test]
    fn to_model_output_maps_usage_when_present() {
        let response = OpenAIChatResponse {
            choices: vec![OpenAIChatChoice {
                message: OpenAIChatMessage {
                    role: "assistant".to_string(),
                    content: Some("hello caravan".to_string()),
                    tool_calls: None,
                    tool_call_id: None,
                },
            }],
            usage: Some(OpenAIUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            }),
        };
        let output = response.to_model_output().unwrap();
        assert_eq!(
            output.usage,
            Some(crate::model::ModelUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            })
        );
    }

    #[test]
    fn to_model_output_usage_is_none_when_absent() {
        let response = OpenAIChatResponse {
            choices: vec![OpenAIChatChoice {
                message: OpenAIChatMessage {
                    role: "assistant".to_string(),
                    content: Some("hello caravan".to_string()),
                    tool_calls: None,
                    tool_call_id: None,
                },
            }],
            usage: None,
        };
        let output = response.to_model_output().unwrap();
        assert_eq!(output.usage, None);
    }

    #[test]
    fn to_model_output_non_empty_tool_calls_returns_adapter_failure() {
        let response = OpenAIChatResponse {
            choices: vec![OpenAIChatChoice {
                message: OpenAIChatMessage {
                    role: "assistant".to_string(),
                    content: None,
                    tool_calls: Some(vec![OpenAIToolCall {
                        id: "c1".to_string(),
                        kind: "function".to_string(),
                        function: OpenAIToolCallFunction {
                            name: "f".to_string(),
                            arguments: "{}".to_string(),
                        },
                    }]),
                    tool_call_id: None,
                },
            }],
            usage: None,
        };
        match response.to_model_output() {
            Err(ModelError::AdapterFailure { message }) => {
                assert!(
                    message.contains("unexpected_tool_call"),
                    "unexpected message: {message}"
                );
            }
            _ => panic!("expected Err(AdapterFailure)"),
        }
    }

    #[test]
    fn to_model_output_empty_tool_calls_behaves_as_content_only() {
        let response = OpenAIChatResponse {
            choices: vec![OpenAIChatChoice {
                message: OpenAIChatMessage {
                    role: "assistant".to_string(),
                    content: Some("hello".to_string()),
                    tool_calls: Some(vec![]),
                    tool_call_id: None,
                },
            }],
            usage: None,
        };
        let output = response.to_model_output().unwrap();
        assert_eq!(output.response, "hello");
    }

    #[test]
    fn to_model_step_output_text_response_returns_assistant() {
        let response = OpenAIChatResponse {
            choices: vec![OpenAIChatChoice {
                message: OpenAIChatMessage {
                    role: "assistant".to_string(),
                    content: Some("hello caravan".to_string()),
                    tool_calls: None,
                    tool_call_id: None,
                },
            }],
            usage: None,
        };
        match response.to_model_step_output().unwrap() {
            crate::model::tool_use::ModelStepOutput::Assistant(o) => {
                assert_eq!(o.response, "hello caravan");
            }
            _ => panic!("expected Assistant"),
        }
    }

    #[test]
    fn to_model_step_output_tool_call_returns_tool_call_with_usage() {
        let response = OpenAIChatResponse {
            choices: vec![OpenAIChatChoice {
                message: OpenAIChatMessage {
                    role: "assistant".to_string(),
                    content: None,
                    tool_calls: Some(vec![OpenAIToolCall {
                        id: "call-1".to_string(),
                        kind: "function".to_string(),
                        function: OpenAIToolCallFunction {
                            name: "search".to_string(),
                            arguments: r#"{"q":"hello"}"#.to_string(),
                        },
                    }]),
                    tool_call_id: None,
                },
            }],
            usage: Some(OpenAIUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            }),
        };
        match response.to_model_step_output().unwrap() {
            crate::model::tool_use::ModelStepOutput::ToolCall { call, usage } => {
                assert_eq!(call.id, "call-1");
                assert_eq!(call.name, "search");
                assert!(call.arguments.is_object());
                let u = usage.unwrap();
                assert_eq!(u.prompt_tokens, 10);
                assert_eq!(u.completion_tokens, 5);
                assert_eq!(u.total_tokens, 15);
            }
            _ => panic!("expected ToolCall"),
        }
    }

    #[test]
    fn to_model_step_output_empty_tool_calls_falls_through_to_content() {
        let response = OpenAIChatResponse {
            choices: vec![OpenAIChatChoice {
                message: OpenAIChatMessage {
                    role: "assistant".to_string(),
                    content: Some("hello".to_string()),
                    tool_calls: Some(vec![]),
                    tool_call_id: None,
                },
            }],
            usage: None,
        };
        match response.to_model_step_output().unwrap() {
            crate::model::tool_use::ModelStepOutput::Assistant(o) => {
                assert_eq!(o.response, "hello");
            }
            _ => panic!("expected Assistant"),
        }
    }

    #[test]
    fn to_model_step_output_multiple_tool_calls_returns_error() {
        let tc = OpenAIToolCall {
            id: "c1".to_string(),
            kind: "function".to_string(),
            function: OpenAIToolCallFunction {
                name: "f".to_string(),
                arguments: "{}".to_string(),
            },
        };
        let response = OpenAIChatResponse {
            choices: vec![OpenAIChatChoice {
                message: OpenAIChatMessage {
                    role: "assistant".to_string(),
                    content: None,
                    tool_calls: Some(vec![tc.clone(), tc]),
                    tool_call_id: None,
                },
            }],
            usage: None,
        };
        match response.to_model_step_output() {
            Err(ModelError::AdapterFailure { message }) => {
                assert!(
                    message.contains("multiple_tool_calls_not_supported"),
                    "unexpected message: {message}"
                );
            }
            _ => panic!("expected Err(AdapterFailure)"),
        }
    }

    #[test]
    fn to_model_step_output_non_object_arguments_returns_error() {
        let response = OpenAIChatResponse {
            choices: vec![OpenAIChatChoice {
                message: OpenAIChatMessage {
                    role: "assistant".to_string(),
                    content: None,
                    tool_calls: Some(vec![OpenAIToolCall {
                        id: "c1".to_string(),
                        kind: "function".to_string(),
                        function: OpenAIToolCallFunction {
                            name: "f".to_string(),
                            arguments: r#""not an object""#.to_string(),
                        },
                    }]),
                    tool_call_id: None,
                },
            }],
            usage: None,
        };
        match response.to_model_step_output() {
            Err(ModelError::AdapterFailure { message }) => {
                assert!(
                    message.contains("malformed_tool_arguments"),
                    "unexpected message: {message}"
                );
            }
            _ => panic!("expected Err(AdapterFailure)"),
        }
    }

    #[test]
    fn to_model_step_output_malformed_call_empty_id_returns_error() {
        let response = OpenAIChatResponse {
            choices: vec![OpenAIChatChoice {
                message: OpenAIChatMessage {
                    role: "assistant".to_string(),
                    content: None,
                    tool_calls: Some(vec![OpenAIToolCall {
                        id: "".to_string(),
                        kind: "function".to_string(),
                        function: OpenAIToolCallFunction {
                            name: "f".to_string(),
                            arguments: "{}".to_string(),
                        },
                    }]),
                    tool_call_id: None,
                },
            }],
            usage: None,
        };
        match response.to_model_step_output() {
            Err(ModelError::AdapterFailure { message }) => {
                assert!(
                    message.contains("malformed_tool_arguments"),
                    "unexpected message: {message}"
                );
            }
            _ => panic!("expected Err(AdapterFailure)"),
        }
    }

    #[test]
    fn to_model_step_output_malformed_call_non_function_type_returns_error() {
        let response = OpenAIChatResponse {
            choices: vec![OpenAIChatChoice {
                message: OpenAIChatMessage {
                    role: "assistant".to_string(),
                    content: None,
                    tool_calls: Some(vec![OpenAIToolCall {
                        id: "c1".to_string(),
                        kind: "retrieval".to_string(), // not "function"
                        function: OpenAIToolCallFunction {
                            name: "f".to_string(),
                            arguments: "{}".to_string(),
                        },
                    }]),
                    tool_call_id: None,
                },
            }],
            usage: None,
        };
        match response.to_model_step_output() {
            Err(ModelError::AdapterFailure { message }) => {
                assert!(
                    message.contains("malformed_tool_arguments"),
                    "unexpected message: {message}"
                );
            }
            _ => panic!("expected Err(AdapterFailure)"),
        }
    }

    #[test]
    fn to_model_step_output_no_content_and_no_tool_calls_returns_error() {
        let response = OpenAIChatResponse {
            choices: vec![OpenAIChatChoice {
                message: OpenAIChatMessage {
                    role: "assistant".to_string(),
                    content: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
            }],
            usage: None,
        };
        match response.to_model_step_output() {
            Err(ModelError::AdapterFailure { message }) => {
                assert!(
                    message.contains("empty model response"),
                    "unexpected message: {message}"
                );
            }
            _ => panic!("expected Err(AdapterFailure)"),
        }
    }
}
