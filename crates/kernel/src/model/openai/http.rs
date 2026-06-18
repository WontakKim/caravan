use std::time::Duration;

use super::request::OpenAIRequestPlan;
use super::types::OpenAIChatResponse;

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

/// Boundary that transmits an [`OpenAIRequestPlan`] to an OpenAI-compatible endpoint.
///
/// Synchronous by design. The default App path injects [`StubOpenAIHttpClient`], which
/// performs no network call and always returns [`OpenAIHttpError::NotImplemented`].
/// [`BlockingOpenAIHttpClient`] is the opt-in real implementation (reqwest blocking,
/// Bearer auth from the env var named by `api_key_env`, per-plan timeout, typed errors)
/// but is **not wired into the default App path**.
pub trait OpenAIHttpClient {
    fn send_chat_completion(
        &self,
        plan: &OpenAIRequestPlan,
    ) -> OpenAIHttpResult<OpenAIChatResponse>;
}

/// Selects which [`OpenAIHttpClient`] implementation to construct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OpenAIHttpClientKind {
    #[default]
    Stub,
    Blocking,
}

impl OpenAIHttpClientKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            OpenAIHttpClientKind::Stub => "stub",
            OpenAIHttpClientKind::Blocking => "blocking",
        }
    }
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
mod tests;
