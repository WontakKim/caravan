use super::*;

#[test]
fn default_is_mock_provider_with_openai_defaults() {
    let cfg = ModelRuntimeConfig::default();
    assert_eq!(
        cfg.model_config.active_profile.provider,
        ModelProvider::Mock
    );
    assert_eq!(cfg.model_config.active_profile.model, "mock-model");
    assert_eq!(
        cfg.model_config.active_profile.adapter,
        ModelAdapterKind::MockModelAdapter
    );
    assert_eq!(cfg.openai_config.base_url, "https://api.openai.com/v1");
    assert_eq!(cfg.openai_config.api_key_env, "OPENAI_API_KEY");
    assert_eq!(cfg.openai_config.timeout_secs, 30);
}

#[test]
fn from_env_map_empty_map_returns_mock_defaults() {
    let cfg = ModelRuntimeConfig::from_env_map(&HashMap::new()).unwrap();
    assert_eq!(cfg, ModelRuntimeConfig::default());
}

#[test]
fn from_env_map_mock_provider_explicit() {
    let vars = HashMap::from([("CARAVAN_MODEL_PROVIDER".into(), "mock".into())]);
    let cfg = ModelRuntimeConfig::from_env_map(&vars).unwrap();
    assert_eq!(
        cfg.model_config.active_profile.provider,
        ModelProvider::Mock
    );
    assert_eq!(
        cfg.model_config.active_profile.adapter,
        ModelAdapterKind::MockModelAdapter
    );
}

#[test]
fn from_env_map_openai_provider() {
    let vars = HashMap::from([("CARAVAN_MODEL_PROVIDER".into(), "openai".into())]);
    let cfg = ModelRuntimeConfig::from_env_map(&vars).unwrap();
    assert_eq!(
        cfg.model_config.active_profile.provider,
        ModelProvider::OpenAI
    );
    assert_eq!(
        cfg.model_config.active_profile.adapter,
        ModelAdapterKind::OpenAICompatibleAdapter
    );
    assert_eq!(cfg.model_config.active_profile.model, "openai-model");
}

#[test]
fn from_env_map_unknown_provider_returns_error() {
    let vars = HashMap::from([("CARAVAN_MODEL_PROVIDER".into(), "gpt-99".into())]);
    let err = ModelRuntimeConfig::from_env_map(&vars).unwrap_err();
    assert_eq!(
        err,
        ModelConfigError::UnknownProvider {
            value: "gpt-99".into()
        }
    );
}

#[test]
fn from_env_map_rejects_openai_compatible_provider() {
    let vars = HashMap::from([("CARAVAN_MODEL_PROVIDER".into(), "openai-compatible".into())]);
    let err = ModelRuntimeConfig::from_env_map(&vars).unwrap_err();
    assert_eq!(
        err,
        ModelConfigError::UnknownProvider {
            value: "openai-compatible".into()
        }
    );
}

#[test]
fn from_env_map_custom_model_name() {
    let vars = HashMap::from([("CARAVAN_MODEL".into(), "my-custom-model".into())]);
    let cfg = ModelRuntimeConfig::from_env_map(&vars).unwrap();
    assert_eq!(cfg.model_config.active_profile.model, "my-custom-model");
}

#[test]
fn from_env_map_custom_base_url() {
    let vars = HashMap::from([(
        "CARAVAN_OPENAI_BASE_URL".into(),
        "https://example.com/v1".into(),
    )]);
    let cfg = ModelRuntimeConfig::from_env_map(&vars).unwrap();
    assert_eq!(cfg.openai_config.base_url, "https://example.com/v1");
}

#[test]
fn from_env_map_custom_api_key_env() {
    let vars = HashMap::from([("CARAVAN_OPENAI_API_KEY_ENV".into(), "MY_API_KEY".into())]);
    let cfg = ModelRuntimeConfig::from_env_map(&vars).unwrap();
    assert_eq!(cfg.openai_config.api_key_env, "MY_API_KEY");
}

#[test]
fn from_env_map_valid_timeout() {
    let vars = HashMap::from([("CARAVAN_OPENAI_TIMEOUT_SECS".into(), "60".into())]);
    let cfg = ModelRuntimeConfig::from_env_map(&vars).unwrap();
    assert_eq!(cfg.openai_config.timeout_secs, 60);
}

#[test]
fn from_env_map_zero_timeout_returns_error() {
    let vars = HashMap::from([("CARAVAN_OPENAI_TIMEOUT_SECS".into(), "0".into())]);
    let err = ModelRuntimeConfig::from_env_map(&vars).unwrap_err();
    assert_eq!(err, ModelConfigError::InvalidTimeout { value: "0".into() });
}

#[test]
fn from_env_map_non_numeric_timeout_returns_error() {
    let vars = HashMap::from([("CARAVAN_OPENAI_TIMEOUT_SECS".into(), "abc".into())]);
    let err = ModelRuntimeConfig::from_env_map(&vars).unwrap_err();
    assert_eq!(
        err,
        ModelConfigError::InvalidTimeout {
            value: "abc".into()
        }
    );
}

#[test]
fn model_config_error_unknown_provider_display() {
    let err = ModelConfigError::UnknownProvider {
        value: "bad-provider".into(),
    };
    assert_eq!(err.kind(), "unknown_provider");
    assert_eq!(
        err.to_string(),
        "kind=unknown_provider message=\"unknown model provider: bad-provider\""
    );
}

#[test]
fn model_config_error_invalid_timeout_display() {
    let err = ModelConfigError::InvalidTimeout {
        value: "oops".into(),
    };
    assert_eq!(err.kind(), "invalid_timeout");
    assert_eq!(
        err.to_string(),
        "kind=invalid_timeout message=\"invalid timeout seconds: oops\""
    );
}

#[test]
fn model_config_error_unknown_openai_http_client_display() {
    let err = ModelConfigError::UnknownOpenAIHttpClient {
        value: "tcp".into(),
    };
    assert_eq!(err.kind(), "unknown_openai_http_client");
    assert_eq!(
        err.to_string(),
        "kind=unknown_openai_http_client message=\"unknown OpenAI HTTP client: tcp\""
    );
}

#[test]
fn default_runtime_config_uses_mock_model_config() {
    let cfg = ModelRuntimeConfig::default();
    assert_eq!(cfg.model_config, ModelConfig::default());
    assert_eq!(
        cfg.model_config.active_profile.provider,
        ModelProvider::Mock
    );
    assert_eq!(cfg.model_config.active_profile.model, "mock-model");
    assert_eq!(
        cfg.model_config.active_profile.adapter,
        ModelAdapterKind::MockModelAdapter
    );
}

#[test]
fn default_runtime_config_uses_default_openai_config() {
    let cfg = ModelRuntimeConfig::default();
    assert_eq!(cfg.openai_config, OpenAICompatibleConfig::default());
    assert_eq!(cfg.openai_config.base_url, "https://api.openai.com/v1");
    assert_eq!(cfg.openai_config.api_key_env, "OPENAI_API_KEY");
    assert_eq!(cfg.openai_config.timeout_secs, 30);
}

#[test]
fn from_env_map_empty_map_equals_default() {
    assert_eq!(
        ModelRuntimeConfig::from_env_map(&HashMap::new()).unwrap(),
        ModelRuntimeConfig::default()
    );
}

#[test]
fn from_env_map_mock_provider_sets_mock_adapter() {
    let vars = HashMap::from([("CARAVAN_MODEL_PROVIDER".into(), "mock".into())]);
    let cfg = ModelRuntimeConfig::from_env_map(&vars).unwrap();
    assert_eq!(
        cfg.model_config.active_profile.provider,
        ModelProvider::Mock
    );
    assert_eq!(
        cfg.model_config.active_profile.adapter,
        ModelAdapterKind::MockModelAdapter
    );
    assert_eq!(cfg.model_config.active_profile.model, "mock-model");
}

#[test]
fn from_env_map_openai_provider_sets_openai_adapter() {
    let vars = HashMap::from([("CARAVAN_MODEL_PROVIDER".into(), "openai".into())]);
    let cfg = ModelRuntimeConfig::from_env_map(&vars).unwrap();
    assert_eq!(
        cfg.model_config.active_profile.provider,
        ModelProvider::OpenAI
    );
    assert_eq!(
        cfg.model_config.active_profile.adapter,
        ModelAdapterKind::OpenAICompatibleAdapter
    );
}

#[test]
fn from_env_map_openai_without_model_uses_placeholder() {
    let vars = HashMap::from([("CARAVAN_MODEL_PROVIDER".into(), "openai".into())]);
    let cfg = ModelRuntimeConfig::from_env_map(&vars).unwrap();
    assert_eq!(cfg.model_config.active_profile.model, "openai-model");
}

#[test]
fn from_env_map_mock_provider_applies_model_override() {
    let vars = HashMap::from([
        ("CARAVAN_MODEL_PROVIDER".into(), "mock".into()),
        ("CARAVAN_MODEL".into(), "custom-model".into()),
    ]);
    let cfg = ModelRuntimeConfig::from_env_map(&vars).unwrap();
    assert_eq!(cfg.model_config.active_profile.model, "custom-model");
}

#[test]
fn from_env_map_openai_provider_applies_model_override() {
    let vars = HashMap::from([
        ("CARAVAN_MODEL_PROVIDER".into(), "openai".into()),
        ("CARAVAN_MODEL".into(), "custom-model".into()),
    ]);
    let cfg = ModelRuntimeConfig::from_env_map(&vars).unwrap();
    assert_eq!(cfg.model_config.active_profile.model, "custom-model");
}

#[test]
fn from_env_map_applies_base_url_override() {
    let vars = HashMap::from([(
        "CARAVAN_OPENAI_BASE_URL".into(),
        "https://example.test/v1".into(),
    )]);
    let cfg = ModelRuntimeConfig::from_env_map(&vars).unwrap();
    assert_eq!(cfg.openai_config.base_url, "https://example.test/v1");
}

#[test]
fn from_env_map_applies_api_key_env_override() {
    let vars = HashMap::from([("CARAVAN_OPENAI_API_KEY_ENV".into(), "MY_KEY_ENV".into())]);
    let cfg = ModelRuntimeConfig::from_env_map(&vars).unwrap();
    assert_eq!(cfg.openai_config.api_key_env, "MY_KEY_ENV");
}

#[test]
fn from_env_map_applies_timeout_override() {
    let vars = HashMap::from([("CARAVAN_OPENAI_TIMEOUT_SECS".into(), "90".into())]);
    let cfg = ModelRuntimeConfig::from_env_map(&vars).unwrap();
    assert_eq!(cfg.openai_config.timeout_secs, 90);
}

#[test]
fn from_process_env_matches_from_env_map_over_ambient() {
    // Verifies the wiring invariant: from_process_env() must produce the same
    // Result as from_env_map() when given the same set of ambient CARAVAN_* keys.
    // By reading both sides from the same ambient environment this test is
    // deterministic regardless of which (if any) CARAVAN_* vars are present —
    // both sides are Ok-equal when the env is clean and both sides return the
    // same Err when the env holds an invalid provider value. No env mutation is
    // performed (unsafe in edition 2024 with parallel test threads).
    let mut ambient = HashMap::new();
    for key in [
        "CARAVAN_MODEL_PROVIDER",
        "CARAVAN_MODEL",
        "CARAVAN_OPENAI_BASE_URL",
        "CARAVAN_OPENAI_API_KEY_ENV",
        "CARAVAN_OPENAI_TIMEOUT_SECS",
        "CARAVAN_OPENAI_HTTP_CLIENT",
    ] {
        if let Ok(value) = std::env::var(key) {
            ambient.insert(key.to_string(), value);
        }
    }
    assert_eq!(
        ModelRuntimeConfig::from_process_env(),
        ModelRuntimeConfig::from_env_map(&ambient)
    );
}

#[test]
fn default_runtime_config_uses_stub_http_client() {
    assert_eq!(
        ModelRuntimeConfig::default().openai_http_client_kind,
        OpenAIHttpClientKind::Stub
    );
}

#[test]
fn from_env_map_http_client_absent_defaults_to_stub() {
    let cfg = ModelRuntimeConfig::from_env_map(&HashMap::new()).unwrap();
    assert_eq!(cfg.openai_http_client_kind, OpenAIHttpClientKind::Stub);
}

#[test]
fn from_env_map_http_client_stub() {
    let vars = HashMap::from([("CARAVAN_OPENAI_HTTP_CLIENT".into(), "stub".into())]);
    let cfg = ModelRuntimeConfig::from_env_map(&vars).unwrap();
    assert_eq!(cfg.openai_http_client_kind, OpenAIHttpClientKind::Stub);
}

#[test]
fn from_env_map_http_client_blocking() {
    let vars = HashMap::from([("CARAVAN_OPENAI_HTTP_CLIENT".into(), "blocking".into())]);
    let cfg = ModelRuntimeConfig::from_env_map(&vars).unwrap();
    assert_eq!(cfg.openai_http_client_kind, OpenAIHttpClientKind::Blocking);
}

#[test]
fn from_env_map_http_client_trims_whitespace() {
    let vars = HashMap::from([("CARAVAN_OPENAI_HTTP_CLIENT".into(), " blocking ".into())]);
    let cfg = ModelRuntimeConfig::from_env_map(&vars).unwrap();
    assert_eq!(cfg.openai_http_client_kind, OpenAIHttpClientKind::Blocking);
}

#[test]
fn from_env_map_http_client_unknown_returns_error() {
    let vars = HashMap::from([("CARAVAN_OPENAI_HTTP_CLIENT".into(), "tcp".into())]);
    let err = ModelRuntimeConfig::from_env_map(&vars).unwrap_err();
    assert_eq!(
        err,
        ModelConfigError::UnknownOpenAIHttpClient {
            value: "tcp".into()
        }
    );
}

#[test]
fn from_env_map_http_client_empty_value_returns_error() {
    let vars = HashMap::from([("CARAVAN_OPENAI_HTTP_CLIENT".into(), "".into())]);
    let err = ModelRuntimeConfig::from_env_map(&vars).unwrap_err();
    assert_eq!(
        err,
        ModelConfigError::UnknownOpenAIHttpClient { value: "".into() }
    );
}

#[test]
fn from_env_map_http_client_rejects_uppercase() {
    let vars = HashMap::from([("CARAVAN_OPENAI_HTTP_CLIENT".into(), "Blocking".into())]);
    let err = ModelRuntimeConfig::from_env_map(&vars).unwrap_err();
    assert_eq!(
        err,
        ModelConfigError::UnknownOpenAIHttpClient {
            value: "Blocking".into()
        }
    );
}

#[test]
fn from_env_map_http_client_unknown_value_is_trimmed() {
    let vars = HashMap::from([("CARAVAN_OPENAI_HTTP_CLIENT".into(), " tcp ".into())]);
    let err = ModelRuntimeConfig::from_env_map(&vars).unwrap_err();
    assert_eq!(
        err,
        ModelConfigError::UnknownOpenAIHttpClient {
            value: "tcp".into()
        }
    );
}
