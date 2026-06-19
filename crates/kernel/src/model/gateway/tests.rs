use super::*;
use crate::model::config::ModelProfile;
use crate::model::openai::http::{OpenAIHttpClientKind, OpenAIHttpResult};
use crate::model::openai::request::OpenAIRequestPlan;
use crate::model::openai::types::{
    OpenAIChatChoice, OpenAIChatMessage, OpenAIChatResponse, OpenAIUsage,
};

#[test]
fn complete_openai_profile_returns_adapter_failure() {
    let config = ModelConfig {
        active_profile: ModelProfile {
            provider: ModelProvider::OpenAI,
            model: "gpt-4o".into(),
            adapter: ModelAdapterKind::OpenAICompatibleAdapter,
        },
    };
    let result = ModelGateway::new(config).complete(ModelRequest {
        prompt: "any".into(),
        user_message: "hello".into(),
    });
    match result {
        Err(ModelError::AdapterFailure { message }) => {
            assert_eq!(
                message,
                "OpenAI-compatible HTTP client is a skeleton in this POC"
            );
        }
        _ => panic!("expected Err(AdapterFailure)"),
    }
}

#[test]
fn complete_returns_expected_response_and_chunks() {
    let response = ModelGateway::default()
        .complete(ModelRequest {
            prompt: "any".into(),
            user_message: "hello caravan".into(),
        })
        .unwrap();
    assert_eq!(
        response.assistant_response,
        "Mock response for: hello caravan"
    );
    assert_eq!(
        response.chunks,
        vec!["Mock", "response", "for:", "hello", "caravan"]
    );
}

#[test]
fn complete_returns_expected_route() {
    let response = ModelGateway::default()
        .complete(ModelRequest {
            prompt: "any".into(),
            user_message: "hello caravan".into(),
        })
        .unwrap();
    assert_eq!(response.route.provider, ModelProvider::Mock);
    assert_eq!(response.route.model, "mock-model");
    assert_eq!(response.route.adapter, ModelAdapterKind::MockModelAdapter);
}

#[test]
fn route_detail_formats_mock_route() {
    let response = ModelGateway::default()
        .complete(ModelRequest {
            prompt: "any".into(),
            user_message: "hello caravan".into(),
        })
        .unwrap();
    assert_eq!(
        response.route.detail(),
        "provider=mock model=mock-model adapter=MockModelAdapter"
    );
}

#[test]
fn route_detail_formats_arbitrary_fields() {
    let route = ModelRoute {
        provider: ModelProvider::Mock,
        model: "m".into(),
        adapter: ModelAdapterKind::MockModelAdapter,
    };
    assert_eq!(
        route.detail(),
        "provider=mock model=m adapter=MockModelAdapter"
    );
}

#[test]
fn route_detail_formats_openai_route() {
    let route = ModelRoute {
        provider: ModelProvider::OpenAI,
        model: "test-model".to_string(),
        adapter: ModelAdapterKind::OpenAICompatibleAdapter,
    };
    assert_eq!(
        route.detail(),
        "provider=openai model=test-model adapter=OpenAICompatibleAdapter"
    );
}

#[test]
fn default_config_route_detail_and_response_match_mock_adapter() {
    let response = ModelGateway::default()
        .complete(ModelRequest {
            prompt: String::new(),
            user_message: String::new(),
        })
        .unwrap();
    assert_eq!(
        response.route.detail(),
        "provider=mock model=mock-model adapter=MockModelAdapter"
    );
    assert_eq!(response.assistant_response, "Mock response for: ");
}

#[test]
fn from_runtime_config_default_returns_mock_response() {
    let gateway = ModelGateway::from_runtime_config(ModelRuntimeConfig::default());
    let response = gateway
        .complete(ModelRequest {
            prompt: "any".into(),
            user_message: "hello caravan".into(),
        })
        .unwrap();
    assert_eq!(
        response.assistant_response,
        "Mock response for: hello caravan"
    );
    assert_eq!(
        response.chunks,
        vec!["Mock", "response", "for:", "hello", "caravan"]
    );
}

#[test]
fn from_runtime_config_default_returns_mock_route() {
    let gateway = ModelGateway::from_runtime_config(ModelRuntimeConfig::default());
    let response = gateway
        .complete(ModelRequest {
            prompt: "any".into(),
            user_message: "hello caravan".into(),
        })
        .unwrap();
    assert_eq!(
        response.route.detail(),
        "provider=mock model=mock-model adapter=MockModelAdapter"
    );
}

#[test]
fn from_runtime_config_openai_profile_returns_adapter_failure() {
    let runtime_config = ModelRuntimeConfig {
        model_config: ModelConfig {
            active_profile: ModelProfile {
                provider: ModelProvider::OpenAI,
                model: "gpt-4o".into(),
                adapter: ModelAdapterKind::OpenAICompatibleAdapter,
            },
        },
        openai_config: OpenAICompatibleConfig::default(),
        openai_http_client_kind: OpenAIHttpClientKind::Stub,
    };
    let result = ModelGateway::from_runtime_config(runtime_config).complete(ModelRequest {
        prompt: "any".into(),
        user_message: "hello caravan".into(),
    });
    match result {
        Err(ModelError::AdapterFailure { message }) => {
            assert_eq!(
                message,
                "OpenAI-compatible HTTP client is a skeleton in this POC"
            );
        }
        _ => panic!("expected Err(AdapterFailure)"),
    }
}

#[test]
fn from_runtime_config_passes_custom_openai_config_to_adapter() {
    let runtime_config = ModelRuntimeConfig {
        model_config: ModelConfig::default(),
        openai_config: OpenAICompatibleConfig {
            base_url: "https://example.test/v1".into(),
            api_key_env: "CUSTOM_KEY_ENV".into(),
            timeout_secs: 99,
        },
        openai_http_client_kind: OpenAIHttpClientKind::Stub,
    };
    let gateway = ModelGateway::from_runtime_config(runtime_config);
    assert_eq!(
        gateway.openai_config_for_test().base_url,
        "https://example.test/v1"
    );
    assert_eq!(
        gateway.openai_config_for_test().api_key_env,
        "CUSTOM_KEY_ENV"
    );
    assert_eq!(gateway.openai_config_for_test().timeout_secs, 99);
}

#[test]
fn from_runtime_config_blocking_kind_missing_key_returns_missing_api_key() {
    use std::collections::HashMap;
    let vars = HashMap::from([
        ("CARAVAN_MODEL_PROVIDER".into(), "openai".into()),
        ("CARAVAN_OPENAI_HTTP_CLIENT".into(), "blocking".into()),
        (
            "CARAVAN_OPENAI_API_KEY_ENV".into(),
            "CARAVAN_TEST_MISSING_OPENAI_KEY_SHOULD_NOT_EXIST".into(),
        ),
    ]);
    let runtime_config = crate::model::runtime_config::ModelRuntimeConfig::from_env_map(&vars)
        .expect("valid config");
    let result = ModelGateway::from_runtime_config(runtime_config).complete(ModelRequest {
        prompt: "any".into(),
        user_message: "hello caravan".into(),
    });
    match result {
        Err(ModelError::AdapterFailure { message }) => {
            assert!(
                message.contains(
                    "missing or empty API key env var: CARAVAN_TEST_MISSING_OPENAI_KEY_SHOULD_NOT_EXIST"
                ),
                "unexpected message: {message}"
            );
            assert!(
                !message.contains("Bearer"),
                "message must not contain Bearer: {message}"
            );
        }
        _ => panic!("expected Err(AdapterFailure)"),
    }
}

#[test]
fn from_runtime_config_explicit_stub_kind_returns_skeleton_error() {
    use std::collections::HashMap;
    let vars = HashMap::from([
        ("CARAVAN_MODEL_PROVIDER".into(), "openai".into()),
        ("CARAVAN_OPENAI_HTTP_CLIENT".into(), "stub".into()),
    ]);
    let runtime_config = crate::model::runtime_config::ModelRuntimeConfig::from_env_map(&vars)
        .expect("valid config");
    let result = ModelGateway::from_runtime_config(runtime_config).complete(ModelRequest {
        prompt: "any".into(),
        user_message: "hello caravan".into(),
    });
    match result {
        Err(ModelError::AdapterFailure { message }) => {
            assert_eq!(
                message,
                "OpenAI-compatible HTTP client is a skeleton in this POC"
            );
        }
        _ => panic!("expected Err(AdapterFailure)"),
    }
}

struct FakeSuccessOpenAIClient;

impl OpenAIHttpClient for FakeSuccessOpenAIClient {
    fn send_chat_completion(
        &self,
        _plan: &OpenAIRequestPlan,
    ) -> OpenAIHttpResult<OpenAIChatResponse> {
        Ok(OpenAIChatResponse {
            choices: vec![OpenAIChatChoice {
                message: OpenAIChatMessage {
                    role: "assistant".to_string(),
                    content: "Hello from fake OpenAI".to_string(),
                },
            }],
            usage: None,
        })
    }
}

#[test]
fn gateway_completes_with_injected_fake_success_client() {
    let config = ModelConfig {
        active_profile: ModelProfile {
            provider: ModelProvider::OpenAI,
            model: "gpt-4o".into(),
            adapter: ModelAdapterKind::OpenAICompatibleAdapter,
        },
    };
    let gateway =
        ModelGateway::with_openai_http_client_for_test(config, Box::new(FakeSuccessOpenAIClient));
    let result = gateway.complete(ModelRequest {
        prompt: "any".into(),
        user_message: "hello".into(),
    });
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.assistant_response, "Hello from fake OpenAI");
}

struct FakeSuccessWithUsageClient;

impl OpenAIHttpClient for FakeSuccessWithUsageClient {
    fn send_chat_completion(
        &self,
        _plan: &OpenAIRequestPlan,
    ) -> OpenAIHttpResult<OpenAIChatResponse> {
        Ok(OpenAIChatResponse {
            choices: vec![OpenAIChatChoice {
                message: OpenAIChatMessage {
                    role: "assistant".to_string(),
                    content: "Hello from fake OpenAI".to_string(),
                },
            }],
            usage: Some(OpenAIUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            }),
        })
    }
}

#[test]
fn gateway_completes_with_injected_fake_success_with_usage_client() {
    let config = ModelConfig {
        active_profile: ModelProfile {
            provider: ModelProvider::OpenAI,
            model: "gpt-4o".into(),
            adapter: ModelAdapterKind::OpenAICompatibleAdapter,
        },
    };
    let gateway = ModelGateway::with_openai_http_client_for_test(
        config,
        Box::new(FakeSuccessWithUsageClient),
    );
    let response = gateway
        .complete(ModelRequest {
            prompt: "any".into(),
            user_message: "hello".into(),
        })
        .unwrap();
    assert_eq!(response.assistant_response, "Hello from fake OpenAI");
    assert_eq!(
        response.usage,
        Some(ModelUsage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
        })
    );
}

#[test]
fn default_gateway_usage_is_none() {
    let response = ModelGateway::default()
        .complete(ModelRequest {
            prompt: String::new(),
            user_message: String::new(),
        })
        .unwrap();
    assert_eq!(
        response.route.detail(),
        "provider=mock model=mock-model adapter=MockModelAdapter"
    );
    assert_eq!(response.assistant_response, "Mock response for: ");
    assert!(response.usage.is_none());
}
