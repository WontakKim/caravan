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
mod tests;
