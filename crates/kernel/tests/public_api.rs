use kernel::{
    AppEvent, Command, EventKind, EventLog, EventSeq, EventStore, MockRunOutput, ModelAdapter,
    ModelAdapterKind, ModelConfig, ModelConfigError, ModelError, ModelGateway, ModelOutput,
    ModelProfile, ModelProvider, ModelRequest, ModelResponse, ModelResult, ModelRoute,
    ModelRuntimeConfig, OpenAIHttpClient, OpenAIHttpError, OpenAIHttpResult, ParsedInput, RunId,
    StubOpenAIHttpClient, TurnId, run_mock_turn,
};

#[test]
fn default_gateway_complete_mock_response() {
    let response = ModelGateway::default()
        .complete(ModelRequest {
            prompt: String::new(),
            user_message: "hello caravan".to_string(),
        })
        .unwrap();
    assert_eq!(
        response.route.detail(),
        "provider=mock model=mock-model adapter=MockModelAdapter"
    );
    assert_eq!(
        response.assistant_response,
        "Mock response for: hello caravan"
    );
}

#[test]
fn from_runtime_config_default_mock_response() {
    let gateway = ModelGateway::from_runtime_config(ModelRuntimeConfig::default());
    let response = gateway
        .complete(ModelRequest {
            prompt: String::new(),
            user_message: "hello caravan".to_string(),
        })
        .unwrap();
    assert_eq!(
        response.route.detail(),
        "provider=mock model=mock-model adapter=MockModelAdapter"
    );
    assert_eq!(
        response.assistant_response,
        "Mock response for: hello caravan"
    );
}

#[test]
fn input_routing_through_root_types() {
    assert!(matches!(
        kernel::commands::parse_input("hello caravan"),
        ParsedInput::UserMessage(_)
    ));
    assert!(matches!(
        kernel::commands::parse_input("/exit"),
        ParsedInput::SlashCommand(Command::Exit)
    ));
    assert!(matches!(
        kernel::commands::parse_input("/quit"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
    assert!(matches!(
        kernel::commands::parse_input("/ask hello"),
        ParsedInput::SlashCommand(Command::Unknown(_))
    ));
}

#[test]
fn run_mock_turn_event_sequence() {
    let mut log = EventLog::new();
    run_mock_turn(&mut log, "hello caravan", &ModelGateway::default());
    let events = log.events();
    assert!(matches!(events.first().unwrap().kind, EventKind::RunCreate));
    assert!(matches!(
        events.last().unwrap().kind,
        EventKind::RunComplete
    ));
}

#[test]
fn importability_checks() {
    let _: Option<AppEvent> = None;
    let _: Option<EventSeq> = None;
    let _: Option<EventStore> = None;
    let _: Option<MockRunOutput> = None;
    let _: Option<ModelAdapterKind> = None;
    let _: Option<ModelConfig> = None;
    let _: Option<ModelConfigError> = None;
    let _: Option<ModelError> = None;
    let _: Option<ModelOutput> = None;
    let _: Option<ModelProfile> = None;
    let _: Option<ModelProvider> = None;
    let _: Option<ModelResult<()>> = None;
    let _: Option<ModelResponse> = None;
    let _: Option<ModelRoute> = None;
    let _: Option<OpenAIHttpError> = None;
    let _: Option<OpenAIHttpResult<()>> = None;
    let _: Option<RunId> = None;
    let _: Option<StubOpenAIHttpClient> = None;
    let _: Option<TurnId> = None;

    fn _assert_adapter<T: ModelAdapter>() {}
    fn _assert_http_client<T: OpenAIHttpClient>() {}
}

#[test]
fn stub_http_client_returns_skeleton_error_via_root_exports() {
    let plan = kernel::model_openai_request::OpenAIRequestBuilder::build(
        &kernel::model_openai_config::OpenAICompatibleConfig::default(),
        "gpt-4o",
        &ModelRequest {
            prompt: String::new(),
            user_message: "hello caravan".to_string(),
        },
    );
    let err = StubOpenAIHttpClient
        .send_chat_completion(&plan)
        .unwrap_err();
    assert_eq!(
        err.message(),
        "OpenAI-compatible HTTP client is a skeleton in this POC"
    );
}
