//! Core execution and model logic for Caravan.

pub mod commands;
pub mod events;
pub mod model;
pub mod model_config;
pub mod model_gateway;
pub mod model_openai_compatible;
pub mod model_openai_config;
pub mod model_openai_http;
pub mod model_openai_request;
pub mod model_openai_types;
pub mod model_registry;
pub mod model_runtime_config;
pub mod model_types;
pub mod prompt;
pub mod runner;
pub mod storage;
pub mod tool_events;
pub mod tools;
pub mod transcript;

pub use commands::{Command, ParsedInput, ToolCommand};
pub use events::{AppEvent, EventKind, EventLog, EventSeq, RunId, TurnId};
pub use model::{
    ModelAdapter, ModelAdapterContext, ModelError, ModelOutput, ModelRequest, ModelResult,
    ModelUsage,
};
pub use model_config::{ModelConfig, ModelProfile};
pub use model_gateway::{ModelGateway, ModelResponse, ModelRoute};
pub use model_openai_http::{
    BlockingOpenAIHttpClient, OpenAIHttpClient, OpenAIHttpClientKind, OpenAIHttpError,
    OpenAIHttpResult, StubOpenAIHttpClient,
};
pub use model_runtime_config::{ModelConfigError, ModelRuntimeConfig};
pub use model_types::{ModelAdapterKind, ModelProvider};
pub use runner::{MockRunOutput, run_mock_turn};
pub use storage::EventStore;
pub use tool_events::ToolEventRunner;
pub use tools::{
    ToolError, ToolExecutionContext, ToolName, ToolOutput, ToolRegistry, ToolRequest, ToolRisk,
};
pub use transcript::{ConversationTranscript, TranscriptMessage, TranscriptRole};
