use std::time::Duration;

use crate::model_openai_request::OpenAIRequestPlan;
use crate::model_openai_types::OpenAIChatResponse;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenAIHttpError {
    NotImplemented { message: String },
    MissingApiKey { env: String },
    RequestFailure { message: String },
    HttpStatus { status: u16, body: String },
    ResponseDecode { message: String },
}

impl OpenAIHttpError {
    pub fn kind(&self) -> &'static str {
        match self {
            OpenAIHttpError::NotImplemented { .. } => "not_implemented",
            OpenAIHttpError::MissingApiKey { .. } => "missing_api_key",
            OpenAIHttpError::RequestFailure { .. } => "request_failure",
            OpenAIHttpError::HttpStatus { .. } => "http_status",
            OpenAIHttpError::ResponseDecode { .. } => "response_decode",
        }
    }

    pub fn message(&self) -> String {
        match self {
            OpenAIHttpError::NotImplemented { message } => message.clone(),
            OpenAIHttpError::MissingApiKey { env } => {
                format!("missing or empty API key env var: {env}")
            }
            OpenAIHttpError::RequestFailure { message } => message.clone(),
            OpenAIHttpError::HttpStatus { status, body } => {
                format!("HTTP status {status}: {body}")
            }
            OpenAIHttpError::ResponseDecode { message } => message.clone(),
        }
    }
}

impl std::fmt::Display for OpenAIHttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "kind={} message=\"{}\"", self.kind(), self.message())
    }
}

pub type OpenAIHttpResult<T> = Result<T, OpenAIHttpError>;

fn api_key_from_env_value(
    env: &str,
    value: Result<String, std::env::VarError>,
) -> OpenAIHttpResult<String> {
    match value {
        Ok(s) if !s.is_empty() => Ok(s),
        _ => Err(OpenAIHttpError::MissingApiKey {
            env: env.to_string(),
        }),
    }
}

fn decode_chat_response(body: &str) -> OpenAIHttpResult<OpenAIChatResponse> {
    serde_json::from_str(body).map_err(|e| OpenAIHttpError::ResponseDecode {
        message: e.to_string(),
    })
}

fn redact_secret(text: &str, secret: &str) -> String {
    if secret.is_empty() {
        return text.to_string();
    }
    text.replace(secret, "[REDACTED_API_KEY]")
}

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

/// Blocking HTTP client that performs real network calls to an OpenAI-compatible endpoint.
///
/// Uses `reqwest::blocking::Client` under the hood.
/// **Not wired into the default App path** — `OpenAICompatibleAdapter::new/default`,
/// `ModelAdapterRegistry::new`, and `ModelGateway` defaults all inject
/// `StubOpenAIHttpClient`. This is an opt-in POC client only.
#[derive(Debug, Clone)]
pub struct BlockingOpenAIHttpClient {
    client: reqwest::blocking::Client,
}

impl Default for BlockingOpenAIHttpClient {
    fn default() -> Self {
        Self {
            client: reqwest::blocking::Client::new(),
        }
    }
}

impl OpenAIHttpClient for BlockingOpenAIHttpClient {
    fn send_chat_completion(
        &self,
        plan: &OpenAIRequestPlan,
    ) -> OpenAIHttpResult<OpenAIChatResponse> {
        let api_key = api_key_from_env_value(&plan.api_key_env, std::env::var(&plan.api_key_env))?;

        let response = self
            .client
            .post(&plan.url)
            .header("Authorization", format!("Bearer {api_key}"))
            .json(&plan.body)
            .timeout(Duration::from_secs(plan.timeout_secs))
            .send()
            .map_err(|e| OpenAIHttpError::RequestFailure {
                message: redact_secret(&e.to_string(), &api_key),
            })?;

        let status = response.status();
        let text = response
            .text()
            .map_err(|e| OpenAIHttpError::RequestFailure {
                message: redact_secret(&e.to_string(), &api_key),
            })?;

        if !status.is_success() {
            return Err(OpenAIHttpError::HttpStatus {
                status: status.as_u16(),
                body: redact_secret(&text, &api_key),
            });
        }

        decode_chat_response(&text)
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

    #[test]
    fn missing_api_key_kind_and_message() {
        let err = OpenAIHttpError::MissingApiKey {
            env: "OPENAI_API_KEY".to_string(),
        };
        assert_eq!(err.kind(), "missing_api_key");
        assert_eq!(
            err.message(),
            "missing or empty API key env var: OPENAI_API_KEY"
        );
    }

    #[test]
    fn request_failure_kind_and_message() {
        let err = OpenAIHttpError::RequestFailure {
            message: "connection refused".to_string(),
        };
        assert_eq!(err.kind(), "request_failure");
        assert_eq!(err.message(), "connection refused");
    }

    #[test]
    fn http_status_kind_and_message() {
        let err = OpenAIHttpError::HttpStatus {
            status: 429,
            body: "rate limited".to_string(),
        };
        assert_eq!(err.kind(), "http_status");
        assert_eq!(err.message(), "HTTP status 429: rate limited");
    }

    #[test]
    fn response_decode_kind_and_message() {
        let err = OpenAIHttpError::ResponseDecode {
            message: "unexpected EOF".to_string(),
        };
        assert_eq!(err.kind(), "response_decode");
        assert_eq!(err.message(), "unexpected EOF");
    }

    #[test]
    fn missing_api_key_message_contains_env_name() {
        let err = OpenAIHttpError::MissingApiKey {
            env: "SOME_ENV_NAME".to_string(),
        };
        assert!(err.message().contains("SOME_ENV_NAME"));
    }

    #[test]
    fn missing_api_key_display_format() {
        let err = OpenAIHttpError::MissingApiKey {
            env: "OPENAI_API_KEY".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "kind=missing_api_key message=\"missing or empty API key env var: OPENAI_API_KEY\""
        );
    }

    // --- BlockingOpenAIHttpClient tests ---

    #[test]
    fn blocking_client_constructs_via_default() {
        let _client = BlockingOpenAIHttpClient::default();
    }

    #[test]
    fn blocking_client_usable_as_boxed_trait_object() {
        let client = BlockingOpenAIHttpClient::default();
        let _boxed: Box<dyn OpenAIHttpClient> = Box::new(client);
    }

    #[test]
    fn blocking_client_missing_env_returns_error_before_network() {
        let client = BlockingOpenAIHttpClient::default();
        let plan = OpenAIRequestPlan {
            url: "http://127.0.0.1:9/unreachable".to_string(),
            api_key_env: "CARAVAN_TEST_MISSING_OPENAI_KEY_SHOULD_NOT_EXIST".to_string(),
            timeout_secs: 1,
            body: crate::model_openai_types::OpenAIChatRequest {
                model: "test-model".to_string(),
                messages: vec![],
                stream: false,
            },
        };
        let result = client.send_chat_completion(&plan);
        assert!(matches!(result, Err(OpenAIHttpError::MissingApiKey { .. })));
        assert!(
            result
                .unwrap_err()
                .message()
                .contains("CARAVAN_TEST_MISSING_OPENAI_KEY_SHOULD_NOT_EXIST")
        );
    }

    // --- api_key_from_env_value helper tests ---

    #[test]
    fn api_key_from_env_value_err_not_present_returns_missing() {
        let result = api_key_from_env_value("MY_KEY", Err(std::env::VarError::NotPresent));
        assert!(matches!(result, Err(OpenAIHttpError::MissingApiKey { .. })));
    }

    #[test]
    fn api_key_from_env_value_ok_empty_returns_missing() {
        let result = api_key_from_env_value("MY_KEY", Ok(String::new()));
        assert!(matches!(result, Err(OpenAIHttpError::MissingApiKey { .. })));
    }

    #[test]
    fn api_key_from_env_value_ok_non_empty_returns_value() {
        let result = api_key_from_env_value("MY_KEY", Ok("sk-secret".to_string()));
        assert_eq!(result, Ok("sk-secret".to_string()));
    }

    // --- decode_chat_response helper tests ---

    #[test]
    fn decode_chat_response_invalid_json_returns_decode_error() {
        let result = decode_chat_response("not json");
        assert!(matches!(
            result,
            Err(OpenAIHttpError::ResponseDecode { .. })
        ));
    }

    #[test]
    fn decode_chat_response_valid_json_returns_response() {
        let json = r#"{"choices":[{"message":{"role":"assistant","content":"hello"}}]}"#;
        let result = decode_chat_response(json);
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.choices.len(), 1);
        assert_eq!(response.choices[0].message.content, "hello");
    }

    // --- redact_secret helper tests ---

    #[test]
    fn redact_secret_replaces_key_in_body() {
        let body = "error: invalid key sk-FAKE-123 provided";
        let redacted = redact_secret(body, "sk-FAKE-123");
        assert!(redacted.contains("[REDACTED_API_KEY]"));
        assert!(!redacted.contains("sk-FAKE-123"));
    }

    #[test]
    fn redact_secret_empty_secret_returns_input_unchanged() {
        let body = "some error message";
        let result = redact_secret(body, "");
        assert_eq!(result, "some error message");
    }
}
