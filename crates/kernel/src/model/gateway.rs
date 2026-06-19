use crate::model::config::ModelConfig;
#[cfg(test)]
use crate::model::openai::config::OpenAICompatibleConfig;
#[cfg(test)]
use crate::model::openai::http::OpenAIHttpClient;
use crate::model::registry::ModelAdapterRegistry;
use crate::model::runtime_config::ModelRuntimeConfig;
use crate::model::types::{ModelAdapterKind, ModelProvider};
use crate::model::{ModelError, ModelRequest, ModelUsage};

pub struct ModelRoute {
    pub provider: ModelProvider,
    pub model: String,
    pub adapter: ModelAdapterKind,
}

impl ModelRoute {
    pub fn detail(&self) -> String {
        format!(
            "provider={} model={} adapter={}",
            self.provider.as_str(),
            self.model,
            self.adapter.as_str()
        )
    }
}

pub struct ModelResponse {
    pub route: ModelRoute,
    pub assistant_response: String,
    pub chunks: Vec<String>,
    pub usage: Option<ModelUsage>,
}

pub struct ModelGateway {
    config: ModelConfig,
    registry: ModelAdapterRegistry,
    #[cfg(test)]
    forced_error: Option<ModelError>,
}

impl ModelGateway {
    pub fn new(config: ModelConfig) -> Self {
        ModelGateway {
            config,
            registry: ModelAdapterRegistry::default(),
            #[cfg(test)]
            forced_error: None,
        }
    }

    pub fn from_runtime_config(runtime_config: ModelRuntimeConfig) -> Self {
        Self {
            config: runtime_config.model_config,
            registry: ModelAdapterRegistry::from_openai_runtime(
                runtime_config.openai_config,
                runtime_config.openai_http_client_kind,
            ),
            #[cfg(test)]
            forced_error: None,
        }
    }

    #[cfg(test)]
    pub fn openai_config_for_test(&self) -> &OpenAICompatibleConfig {
        self.registry.openai_config_for_test()
    }

    #[cfg(test)]
    pub fn failing_for_test(error: ModelError) -> Self {
        ModelGateway {
            config: ModelConfig::default(),
            registry: ModelAdapterRegistry::default(),
            forced_error: Some(error),
        }
    }

    #[cfg(test)]
    pub fn with_openai_http_client_for_test(
        config: ModelConfig,
        http_client: Box<dyn OpenAIHttpClient>,
    ) -> Self {
        ModelGateway {
            config,
            registry: ModelAdapterRegistry::with_openai_http_client(
                OpenAICompatibleConfig::default(),
                http_client,
            ),
            forced_error: None,
        }
    }

    pub fn complete(&self, request: ModelRequest) -> Result<ModelResponse, ModelError> {
        #[cfg(test)]
        if let Some(ref err) = self.forced_error {
            return Err(err.clone());
        }

        let profile = &self.config.active_profile;
        match self.registry.complete(profile, &request) {
            Ok(output) => Ok(ModelResponse {
                route: ModelRoute {
                    provider: profile.provider,
                    model: profile.model.clone(),
                    adapter: profile.adapter,
                },
                assistant_response: output.response,
                chunks: output.chunks,
                usage: output.usage,
            }),
            Err(e) => Err(e),
        }
    }
}

impl Default for ModelGateway {
    fn default() -> Self {
        Self::new(ModelConfig::default())
    }
}

#[cfg(test)]
mod tests;
