//! Core execution and model logic for Caravan.

pub mod commands;
pub mod events;
pub mod manual_context;
pub mod model;
pub mod model_config;
pub mod model_gateway;
pub mod model_registry;
pub mod model_runtime_config;
pub mod model_tool_request;
pub mod model_types;
pub mod prompt;
pub mod runner;
pub mod storage;
pub mod tool;
pub mod transcript;

pub use commands::{Command, ContextCommand, ParsedInput, RequestCommand, ToolCommand};
pub use events::{AppEvent, EventKind, EventLog, EventSeq, RunId, TurnId};
pub use manual_context::{MANUAL_TOOL_CONTEXT_MAX_BYTES, ManualToolContext};
pub use model::openai::http::{
    BlockingOpenAIHttpClient, OpenAIHttpClient, OpenAIHttpClientKind, OpenAIHttpError,
    OpenAIHttpResult, StubOpenAIHttpClient,
};
pub use model::types::{ModelAdapterKind, ModelProvider};
pub use model::{
    ModelAdapter, ModelAdapterContext, ModelError, ModelOutput, ModelRequest, ModelResult,
    ModelUsage,
};
pub use model_config::{ModelConfig, ModelProfile};
pub use model_gateway::{ModelGateway, ModelResponse, ModelRoute};
pub use model_runtime_config::{ModelConfigError, ModelRuntimeConfig};
pub use model_tool_request::{
    ModelToolRequest, ModelToolRequestKind, parse_first_model_tool_request,
};
pub use runner::{MockRunOutput, run_mock_turn};
pub use storage::EventStore;
pub use tool::events::ToolEventRunner;
pub use tool::policy::{ToolPolicyDecision, ToolPolicyEngine, ToolPolicyOutcome};
pub use tool::registry::{
    ToolError, ToolExecutionContext, ToolName, ToolOutput, ToolRegistry, ToolRequest, ToolRisk,
};
pub use tool::schema::{ToolCatalog, ToolInputSpec, ToolSpec};
pub use transcript::{ConversationTranscript, TranscriptMessage, TranscriptRole};
