//! Config-assembly layer for `ModelRuntimeConfig`.
//!
//! `ModelConfigError` (config-assembly phase) is intentionally distinct from
//! `ModelError` (runtime adapter phase): config errors arise before any adapter
//! is instantiated, whereas `ModelError` is produced by an already-running adapter.

use crate::model::openai::config::OpenAICompatibleConfig;
use crate::model::openai::http::OpenAIHttpClientKind;
use crate::model_config::{ModelConfig, ModelProfile};
use crate::model_types::{ModelAdapterKind, ModelProvider};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelConfigError {
    UnknownProvider { value: String },
    InvalidTimeout { value: String },
    UnknownOpenAIHttpClient { value: String },
}

impl ModelConfigError {
    pub fn kind(&self) -> &'static str {
        match self {
            ModelConfigError::UnknownProvider { .. } => "unknown_provider",
            ModelConfigError::InvalidTimeout { .. } => "invalid_timeout",
            ModelConfigError::UnknownOpenAIHttpClient { .. } => "unknown_openai_http_client",
        }
    }

    pub fn message(&self) -> String {
        match self {
            ModelConfigError::UnknownProvider { value } => {
                format!("unknown model provider: {value}")
            }
            ModelConfigError::InvalidTimeout { value } => {
                format!("invalid timeout seconds: {value}")
            }
            ModelConfigError::UnknownOpenAIHttpClient { value } => {
                format!("unknown OpenAI HTTP client: {value}")
            }
        }
    }
}

impl std::fmt::Display for ModelConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "kind={} message=\"{}\"", self.kind(), self.message())
    }
}

impl std::error::Error for ModelConfigError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRuntimeConfig {
    pub model_config: ModelConfig,
    pub openai_config: OpenAICompatibleConfig,
    pub openai_http_client_kind: OpenAIHttpClientKind,
}

impl Default for ModelRuntimeConfig {
    fn default() -> Self {
        Self {
            model_config: ModelConfig::default(),
            openai_config: OpenAICompatibleConfig::default(),
            openai_http_client_kind: OpenAIHttpClientKind::Stub,
        }
    }
}

impl ModelRuntimeConfig {
    /// Reads only the six allowed CARAVAN_* keys from the process environment
    /// via individual `std::env::var` calls and delegates to [`Self::from_env_map`].
    ///
    /// Note: `CARAVAN_OPENAI_API_KEY_ENV` is treated as the **name** of the env var
    /// that holds the API key — its value is never resolved here.
    pub fn from_process_env() -> Result<Self, ModelConfigError> {
        let mut vars = HashMap::new();
        for key in [
            "CARAVAN_MODEL_PROVIDER",
            "CARAVAN_MODEL",
            "CARAVAN_OPENAI_BASE_URL",
            "CARAVAN_OPENAI_API_KEY_ENV",
            "CARAVAN_OPENAI_TIMEOUT_SECS",
            "CARAVAN_OPENAI_HTTP_CLIENT",
        ] {
            if let Ok(value) = std::env::var(key) {
                vars.insert(key.to_string(), value);
            }
        }
        Self::from_env_map(&vars)
    }

    pub fn from_env_map(vars: &HashMap<String, String>) -> Result<Self, ModelConfigError> {
        let provider = match vars.get("CARAVAN_MODEL_PROVIDER") {
            None => ModelProvider::Mock,
            Some(v) => parse_provider(v)?,
        };

        let adapter = match provider {
            ModelProvider::Mock => ModelAdapterKind::MockModelAdapter,
            ModelProvider::OpenAI => ModelAdapterKind::OpenAICompatibleAdapter,
        };

        let default_model = match provider {
            ModelProvider::Mock => "mock-model",
            ModelProvider::OpenAI => "openai-model",
        };
        let model = vars
            .get("CARAVAN_MODEL")
            .cloned()
            .unwrap_or_else(|| default_model.to_string());

        let base_url = vars
            .get("CARAVAN_OPENAI_BASE_URL")
            .cloned()
            .unwrap_or_else(|| "https://api.openai.com/v1".to_string());

        let api_key_env = vars
            .get("CARAVAN_OPENAI_API_KEY_ENV")
            .cloned()
            .unwrap_or_else(|| "OPENAI_API_KEY".to_string());

        let timeout_secs = match vars.get("CARAVAN_OPENAI_TIMEOUT_SECS") {
            None => 30u64,
            Some(v) => {
                let parsed: u64 = v
                    .parse()
                    .map_err(|_| ModelConfigError::InvalidTimeout { value: v.clone() })?;
                if parsed == 0 {
                    return Err(ModelConfigError::InvalidTimeout { value: v.clone() });
                }
                parsed
            }
        };

        let openai_http_client_kind = match vars.get("CARAVAN_OPENAI_HTTP_CLIENT") {
            None => OpenAIHttpClientKind::Stub,
            Some(v) => parse_http_client_kind(v)?,
        };

        Ok(Self {
            model_config: ModelConfig {
                active_profile: ModelProfile {
                    provider,
                    model,
                    adapter,
                },
            },
            openai_config: OpenAICompatibleConfig {
                base_url,
                api_key_env,
                timeout_secs,
            },
            openai_http_client_kind,
        })
    }
}

fn parse_provider(value: &str) -> Result<ModelProvider, ModelConfigError> {
    match value {
        "mock" => Ok(ModelProvider::Mock),
        "openai" => Ok(ModelProvider::OpenAI),
        other => Err(ModelConfigError::UnknownProvider {
            value: other.to_string(),
        }),
    }
}

fn parse_http_client_kind(value: &str) -> Result<OpenAIHttpClientKind, ModelConfigError> {
    match value.trim() {
        "stub" => Ok(OpenAIHttpClientKind::Stub),
        "blocking" => Ok(OpenAIHttpClientKind::Blocking),
        other => Err(ModelConfigError::UnknownOpenAIHttpClient {
            value: other.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
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
}
