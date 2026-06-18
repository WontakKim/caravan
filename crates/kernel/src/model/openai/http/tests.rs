use super::*;
use crate::model::ModelRequest;
use crate::model::openai::config::OpenAICompatibleConfig;
use crate::model::openai::request::OpenAIRequestBuilder;
use crate::model::openai::types::{OpenAIChatChoice, OpenAIChatMessage};

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
        body: crate::model::openai::types::OpenAIChatRequest {
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

// --- OpenAIHttpClientKind tests ---

#[test]
fn http_client_kind_default_is_stub() {
    assert_eq!(OpenAIHttpClientKind::default(), OpenAIHttpClientKind::Stub);
}

#[test]
fn http_client_kind_as_str_values() {
    assert_eq!(OpenAIHttpClientKind::Stub.as_str(), "stub");
    assert_eq!(OpenAIHttpClientKind::Blocking.as_str(), "blocking");
}
