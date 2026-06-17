use super::config::OpenAICompatibleConfig;
use super::types::OpenAIChatRequest;
use crate::model::ModelRequest;

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

#[cfg(test)]
mod tests {
    use super::*;

    fn default_request() -> ModelRequest {
        ModelRequest {
            prompt: "any prompt".to_string(),
            user_message: "any message".to_string(),
        }
    }

    #[test]
    fn build_uses_default_chat_completions_url() {
        let config = OpenAICompatibleConfig::default();
        let plan = OpenAIRequestBuilder::build(&config, "gpt-4o", &default_request());
        assert_eq!(plan.url, "https://api.openai.com/v1/chat/completions");
    }

    #[test]
    fn build_does_not_duplicate_trailing_slash() {
        let config = OpenAICompatibleConfig {
            base_url: "https://example.com/v1/".to_string(),
            ..Default::default()
        };
        let plan = OpenAIRequestBuilder::build(&config, "gpt-4o", &default_request());
        assert_eq!(plan.url, "https://example.com/v1/chat/completions");
    }

    #[test]
    fn build_preserves_api_key_env() {
        let config = OpenAICompatibleConfig {
            api_key_env: "OPENAI_API_KEY".to_string(),
            ..Default::default()
        };
        let plan = OpenAIRequestBuilder::build(&config, "gpt-4o", &default_request());
        assert_eq!(plan.api_key_env, "OPENAI_API_KEY");
    }

    #[test]
    fn build_preserves_timeout_secs() {
        let config = OpenAICompatibleConfig {
            timeout_secs: 60,
            ..Default::default()
        };
        let plan = OpenAIRequestBuilder::build(&config, "gpt-4o", &default_request());
        assert_eq!(plan.timeout_secs, 60);
    }

    #[test]
    fn build_sets_body_model() {
        let config = OpenAICompatibleConfig::default();
        let plan = OpenAIRequestBuilder::build(&config, "gpt-4o-mini", &default_request());
        assert_eq!(plan.body.model, "gpt-4o-mini");
    }

    #[test]
    fn build_sets_user_role_message() {
        let config = OpenAICompatibleConfig::default();
        let plan = OpenAIRequestBuilder::build(&config, "gpt-4o", &default_request());
        assert_eq!(plan.body.messages.len(), 1);
        assert_eq!(plan.body.messages[0].role, "user");
    }

    #[test]
    fn build_maps_prompt_to_message_content() {
        let request = ModelRequest {
            prompt: "SYSTEM: be helpful\nUSER: explain recursion".to_string(),
            user_message: "explain recursion".to_string(),
        };
        let config = OpenAICompatibleConfig::default();
        let plan = OpenAIRequestBuilder::build(&config, "gpt-4o", &request);
        assert_eq!(
            plan.body.messages[0].content,
            "SYSTEM: be helpful\nUSER: explain recursion"
        );
    }

    #[test]
    fn build_sets_stream_false() {
        let config = OpenAICompatibleConfig::default();
        let plan = OpenAIRequestBuilder::build(&config, "gpt-4o", &default_request());
        assert!(!plan.body.stream);
    }

    #[test]
    fn plan_body_serializes_to_json() {
        let config = OpenAICompatibleConfig::default();
        let plan = OpenAIRequestBuilder::build(&config, "gpt-4o", &default_request());
        let json = serde_json::to_string(&plan.body);
        assert!(json.is_ok());
        let json_str = json.unwrap();
        assert!(json_str.contains("\"model\""));
        assert!(json_str.contains("\"messages\""));
        assert!(json_str.contains("\"stream\""));
    }

    #[test]
    fn plan_has_exactly_four_fields() {
        let config = OpenAICompatibleConfig::default();
        let plan = OpenAIRequestBuilder::build(&config, "gpt-4o", &default_request());
        let OpenAIRequestPlan {
            url,
            api_key_env,
            timeout_secs,
            body,
        } = plan;
        assert_eq!(url, "https://api.openai.com/v1/chat/completions");
        assert_eq!(api_key_env, "OPENAI_API_KEY");
        assert_eq!(timeout_secs, 30);
        assert_eq!(body.model, "gpt-4o");
    }
}
