#![allow(dead_code)]
// The request plan is unused until the OpenAI-compatible adapter performs real calls;
// remove this allow when the adapter wires in the plan.

use crate::model::ModelRequest;
use crate::model_openai_config::OpenAICompatibleConfig;
use crate::model_openai_types::OpenAIChatRequest;

/// Describes what would be sent to an OpenAI-compatible endpoint.
///
/// This is a coordinator type — it is never transmitted over the network.
/// `api_key_env` holds only the environment variable NAME (e.g. `"OPENAI_API_KEY"`),
/// never a resolved secret value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenAIRequestPlan {
    pub url: String,
    pub api_key_env: String,
    pub timeout_secs: u64,
    pub body: OpenAIChatRequest,
}

/// Assembles an [`OpenAIRequestPlan`] from config, model name, and request.
///
/// Pure assembly — no I/O, no validation, no network access.
pub struct OpenAIRequestBuilder;

impl OpenAIRequestBuilder {
    /// Build a request plan from the given config, model name, and model request.
    ///
    /// The `api_key_env` field on the returned plan holds only the variable name
    /// copied from `config` — the value is never resolved here.
    pub fn build(
        config: &OpenAICompatibleConfig,
        model: &str,
        request: &ModelRequest,
    ) -> OpenAIRequestPlan {
        OpenAIRequestPlan {
            url: config.chat_completions_url(),
            api_key_env: config.api_key_env.clone(),
            timeout_secs: config.timeout_secs,
            body: OpenAIChatRequest::from_model_request(model, request),
        }
    }
}
