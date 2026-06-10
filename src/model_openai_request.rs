#![allow(dead_code)]
// The request plan is unused until the OpenAI-compatible adapter performs real calls;
// remove this allow when the adapter wires in the plan.

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
