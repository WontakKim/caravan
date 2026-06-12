#![allow(dead_code)]
// The HTTP client boundary is unused until the OpenAI-compatible adapter wires it in;
// remove this allow when the adapter sends real requests through this client.

use crate::model_openai_request::OpenAIRequestPlan;
use crate::model_openai_types::OpenAIChatResponse;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenAIHttpError {
    NotImplemented { message: String },
}

impl OpenAIHttpError {
    pub fn kind(&self) -> &'static str {
        match self {
            OpenAIHttpError::NotImplemented { .. } => "not_implemented",
        }
    }

    pub fn message(&self) -> &str {
        match self {
            OpenAIHttpError::NotImplemented { message } => message,
        }
    }
}

impl std::fmt::Display for OpenAIHttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "kind={} message=\"{}\"", self.kind(), self.message())
    }
}

pub type OpenAIHttpResult<T> = Result<T, OpenAIHttpError>;

/// Boundary that would transmit an [`OpenAIRequestPlan`] to an OpenAI-compatible endpoint.
///
/// Synchronous by design for this POC — no async runtime exists in the workspace.
/// Implementations receive the full plan (url, api_key_env, timeout_secs, body) but
/// this POC never resolves API key values or constructs HTTP requests.
pub trait OpenAIHttpClient {
    fn send_chat_completion(
        &self,
        plan: &OpenAIRequestPlan,
    ) -> OpenAIHttpResult<OpenAIChatResponse>;
}

/// Stub client: performs no network I/O and always returns [`OpenAIHttpError::NotImplemented`].
#[derive(Debug, Default, Clone, Copy)]
pub struct StubOpenAIHttpClient;

impl OpenAIHttpClient for StubOpenAIHttpClient {
    fn send_chat_completion(
        &self,
        _plan: &OpenAIRequestPlan,
    ) -> OpenAIHttpResult<OpenAIChatResponse> {
        Err(OpenAIHttpError::NotImplemented {
            message: "OpenAI-compatible HTTP client is a skeleton in this POC".to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ModelRequest;
    use crate::model_openai_config::OpenAICompatibleConfig;
    use crate::model_openai_request::OpenAIRequestBuilder;
    use crate::model_openai_types::{OpenAIChatChoice, OpenAIChatMessage};

    fn default_plan() -> OpenAIRequestPlan {
        let config = OpenAICompatibleConfig::default();
        let request = ModelRequest {
            prompt: "any prompt".to_string(),
            user_message: "any message".to_string(),
        };
        OpenAIRequestBuilder::build(&config, "gpt-4o", &request)
    }

    struct FakeSuccessClient;

    impl OpenAIHttpClient for FakeSuccessClient {
        fn send_chat_completion(
            &self,
            _plan: &OpenAIRequestPlan,
        ) -> OpenAIHttpResult<OpenAIChatResponse> {
            Ok(OpenAIChatResponse {
                choices: vec![OpenAIChatChoice {
                    message: OpenAIChatMessage {
                        role: "assistant".to_string(),
                        content: "hello".to_string(),
                    },
                }],
                usage: None,
            })
        }
    }

    fn send_via<C: OpenAIHttpClient>(
        client: &C,
        plan: &OpenAIRequestPlan,
    ) -> OpenAIHttpResult<OpenAIChatResponse> {
        client.send_chat_completion(plan)
    }

    #[test]
    fn stub_is_constructible_default() {
        let _client = StubOpenAIHttpClient::default();
        let _client2 = StubOpenAIHttpClient;
    }

    #[test]
    fn stub_returns_not_implemented_error() {
        let client = StubOpenAIHttpClient;
        let plan = default_plan();
        let result = client.send_chat_completion(&plan);
        assert!(matches!(
            result,
            Err(OpenAIHttpError::NotImplemented { .. })
        ));
    }

    #[test]
    fn stub_error_message_is_exact() {
        let client = StubOpenAIHttpClient;
        let plan = default_plan();
        let err = client.send_chat_completion(&plan).unwrap_err();
        assert_eq!(
            err.message(),
            "OpenAI-compatible HTTP client is a skeleton in this POC"
        );
    }

    #[test]
    fn stub_error_kind_and_display() {
        let client = StubOpenAIHttpClient;
        let plan = default_plan();
        let err = client.send_chat_completion(&plan).unwrap_err();
        assert_eq!(err.kind(), "not_implemented");
        assert_eq!(
            err.to_string(),
            "kind=not_implemented message=\"OpenAI-compatible HTTP client is a skeleton in this POC\""
        );
    }

    #[test]
    fn stub_usable_as_trait_object() {
        let client = StubOpenAIHttpClient;
        let plan = default_plan();
        let dyn_client: &dyn OpenAIHttpClient = &client;
        let result = dyn_client.send_chat_completion(&plan);
        assert!(result.is_err());
    }

    #[test]
    fn stub_usable_via_generic_bound() {
        let client = StubOpenAIHttpClient;
        let plan = default_plan();
        let result = send_via(&client, &plan);
        assert!(matches!(
            result,
            Err(OpenAIHttpError::NotImplemented { .. })
        ));
    }

    #[test]
    fn fake_success_client_returns_ok() {
        let client = FakeSuccessClient;
        let plan = default_plan();
        let result = client.send_chat_completion(&plan);
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.choices.len(), 1);
        assert_eq!(response.choices[0].message.content, "hello");
    }
}
