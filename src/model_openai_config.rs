#![allow(dead_code)]
// Config fields are unused until the OpenAI-compatible adapter performs real calls;
// remove this allow when complete() wires in the config.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenAICompatibleConfig {
    pub base_url: String,
    pub api_key_env: String,
    pub timeout_secs: u64,
}

impl OpenAICompatibleConfig {
    pub fn chat_completions_url(&self) -> String {
        format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
    }
}

impl Default for OpenAICompatibleConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.openai.com/v1".to_string(),
            api_key_env: "OPENAI_API_KEY".to_string(),
            timeout_secs: 30,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_base_url_is_openai_v1() {
        assert_eq!(
            OpenAICompatibleConfig::default().base_url,
            "https://api.openai.com/v1"
        );
    }

    #[test]
    fn default_api_key_env_is_openai_api_key() {
        assert_eq!(
            OpenAICompatibleConfig::default().api_key_env,
            "OPENAI_API_KEY"
        );
    }

    #[test]
    fn default_timeout_secs_is_30() {
        assert_eq!(OpenAICompatibleConfig::default().timeout_secs, 30);
    }

    #[test]
    fn chat_completions_url_appends_endpoint_to_default_base_url() {
        assert_eq!(
            OpenAICompatibleConfig::default().chat_completions_url(),
            "https://api.openai.com/v1/chat/completions"
        );
    }

    #[test]
    fn chat_completions_url_does_not_double_trailing_slash() {
        let config = OpenAICompatibleConfig {
            base_url: "https://api.openai.com/v1/".to_string(),
            ..Default::default()
        };
        assert_eq!(
            config.chat_completions_url(),
            "https://api.openai.com/v1/chat/completions"
        );
    }
}
