use super::super::*;
use super::common::*;
use kernel::events::EventKind;
use kernel::model_runtime_config::ModelRuntimeConfig;
use kernel::storage::EventStore;
use std::collections::HashMap;

// --- ModelToolRequest guidance tests ---

#[test]
fn model_tool_request_block_does_not_produce_guidance_by_default() {
    // The default runtime no longer consumes detected_model_tool_request, so a
    // CARAVAN_TOOL_REQUEST block in the assistant response is treated as plain
    // text: no guidance lines are pushed and pending stays None.
    let mut app = App::new();
    app.input =
        "read the readme\nCARAVAN_TOOL_REQUEST\ntool=read_file\npath=README.md\nEND_CARAVAN_TOOL_REQUEST"
            .to_string();
    app.submit();

    // No guidance lines must appear in self.log.
    assert!(
        !app.log.iter().any(|l| l == "Run: /tool read README.md"),
        "log must not contain 'Run: /tool read README.md'"
    );
    assert!(
        !app.log
            .iter()
            .any(|l| l.contains("did not execute it automatically")),
        "log must not contain 'did not execute it automatically'"
    );

    // pending_model_tool_request must remain None.
    assert!(
        app.pending_model_tool_request.is_none(),
        "pending_model_tool_request must remain None (default-off)"
    );

    // No tool-execution events.
    let events = app.event_log.events();
    assert!(
        !events.iter().any(|e| e.kind == EventKind::ToolCall),
        "must not emit ToolCall events"
    );
    assert!(
        !events.iter().any(|e| e.kind == EventKind::ToolResult),
        "must not emit ToolResult events"
    );
    assert!(
        !events.iter().any(|e| e.kind == EventKind::ToolError),
        "must not emit ToolError events"
    );
    assert!(
        app.last_tool_output_candidate.is_none(),
        "last_tool_output_candidate must remain None"
    );
    assert!(
        app.pending_manual_tool_context.is_none(),
        "pending_manual_tool_context must remain None"
    );
}

#[test]
fn plain_message_produces_no_model_tool_request_guidance() {
    let mut app = App::new();
    app.input = "hello caravan".to_string();
    app.submit();

    // (c) Negative case: no "Run: /tool" log lines and no ModelToolRequest event.
    assert!(
        !app.log.iter().any(|l| l.starts_with("Run: /tool")),
        "plain message must not produce any 'Run: /tool' log lines"
    );
    let events = app.event_log.events();
    assert!(
        !events.iter().any(|e| e.kind == EventKind::ModelToolRequest),
        "plain message must not produce a ModelToolRequest event"
    );
}

// --- pending_model_tool_request field tests ---

#[test]
fn model_tool_request_block_does_not_set_pending_by_default() {
    // The default runtime no longer consumes detected_model_tool_request, so a
    // CARAVAN_TOOL_REQUEST block in the assistant response leaves pending None.
    let mut app = App::new();
    app.input =
        "read the readme\nCARAVAN_TOOL_REQUEST\ntool=read_file\npath=README.md\nEND_CARAVAN_TOOL_REQUEST"
            .to_string();
    app.submit();

    assert!(
        app.pending_model_tool_request.is_none(),
        "pending_model_tool_request must remain None (default-off)"
    );
    assert!(
        app.last_tool_output_candidate.is_none(),
        "last_tool_output_candidate must remain None"
    );
    assert!(
        app.pending_manual_tool_context.is_none(),
        "pending_manual_tool_context must remain None"
    );
}

#[test]
fn basic_mock_flow_keeps_pending_model_tool_request_none() {
    let mut app = App::new();
    app.input = "hello caravan".to_string();
    app.submit();

    assert!(
        app.pending_model_tool_request.is_none(),
        "pending_model_tool_request should remain None on plain mock response"
    );
}

#[test]
fn error_path_does_not_set_pending_model_tool_request() {
    let dir = TempDir::new();
    let store = EventStore::new(dir.path());

    let vars = HashMap::from([("CARAVAN_MODEL_PROVIDER".to_string(), "openai".to_string())]);
    let runtime_config = ModelRuntimeConfig::from_env_map(&vars).unwrap();
    let gateway = kernel::model_gateway::ModelGateway::from_runtime_config(runtime_config);

    let mut app = App::with_store_and_gateway(store, gateway);
    app.input = "hello".to_string();
    app.submit();

    assert!(
        app.pending_model_tool_request.is_none(),
        "error path must not set pending_model_tool_request"
    );
}

#[test]
fn none_detection_keeps_existing_pending() {
    use kernel::model_tool_request::{ModelToolRequest, ModelToolRequestKind};

    let mut app = App::new();
    let existing = ModelToolRequest {
        kind: ModelToolRequestKind::ReadFile,
        path: "README.md".to_string(),
    };
    app.pending_model_tool_request = Some(existing.clone());

    // Plain message — default mock returns no tool request block (detected = None).
    app.input = "hello caravan".to_string();
    app.submit();

    assert_eq!(
        app.pending_model_tool_request,
        Some(existing),
        "existing pending must remain unchanged when detection is None"
    );
}

#[test]
fn error_path_keeps_existing_pending() {
    use kernel::model_tool_request::{ModelToolRequest, ModelToolRequestKind};

    let dir = TempDir::new();
    let store = EventStore::new(dir.path());

    let vars = HashMap::from([("CARAVAN_MODEL_PROVIDER".to_string(), "openai".to_string())]);
    let runtime_config = ModelRuntimeConfig::from_env_map(&vars).unwrap();
    let gateway = kernel::model_gateway::ModelGateway::from_runtime_config(runtime_config);

    let mut app = App::with_store_and_gateway(store, gateway);
    let existing = ModelToolRequest {
        kind: ModelToolRequestKind::ReadFile,
        path: "README.md".to_string(),
    };
    app.pending_model_tool_request = Some(existing.clone());

    app.input = "hello".to_string();
    app.submit();

    assert_eq!(
        app.pending_model_tool_request,
        Some(existing),
        "error path must not clear existing pending_model_tool_request"
    );
}

#[test]
fn model_response_does_not_replace_seeded_pending() {
    use kernel::model_tool_request::{ModelToolRequest, ModelToolRequestKind};

    // The default runtime ignores detected_model_tool_request, so a seeded
    // pending value must be preserved exactly — not replaced — when a model
    // response containing a CARAVAN_TOOL_REQUEST block is submitted.
    let mut app = App::new();
    let seeded = ModelToolRequest {
        kind: ModelToolRequestKind::ListFiles,
        path: ".".to_string(),
    };
    app.pending_model_tool_request = Some(seeded.clone());

    // Submit a message whose response contains a CARAVAN_TOOL_REQUEST block.
    app.input =
        "read the readme\nCARAVAN_TOOL_REQUEST\ntool=read_file\npath=README.md\nEND_CARAVAN_TOOL_REQUEST"
            .to_string();
    app.submit();

    assert_eq!(
        app.pending_model_tool_request,
        Some(seeded),
        "model response must not replace the seeded pending_model_tool_request"
    );
}

// --- /request command tests ---

#[test]
fn request_status_without_pending_logs_none_and_only_slash_command() {
    let mut app = App::new();
    assert!(app.pending_model_tool_request.is_none());

    let log_len_before = app.log.len();
    let event_len_before = app.event_log.len();

    app.input = "/request status".to_string();
    app.submit();

    // Exactly two log lines added: header and "none" line.
    assert_eq!(app.log[log_len_before], "Model tool request status:");
    assert_eq!(app.log[log_len_before + 1], "- pending: none");
    assert_eq!(app.log.len(), log_len_before + 2);

    // Exactly one event appended and it is SlashCommand.
    assert_eq!(app.event_log.len(), event_len_before + 1);
    let new_event = app.event_log.get(event_len_before).unwrap();
    assert_eq!(new_event.kind, EventKind::SlashCommand);
    assert_eq!(new_event.detail, "/request status");

    // No RunCreate event anywhere.
    assert!(
        !app.event_log
            .events()
            .iter()
            .any(|e| e.kind == EventKind::RunCreate),
        "/request status must not start a model run"
    );
}

#[test]
fn request_status_with_pending_logs_suggested_command_and_next_step() {
    use kernel::model_tool_request::{ModelToolRequest, ModelToolRequestKind};

    let mut app = App::new();
    let req = ModelToolRequest {
        kind: ModelToolRequestKind::ReadFile,
        path: "README.md".to_string(),
    };
    app.pending_model_tool_request = Some(req.clone());

    let log_len_before = app.log.len();
    let event_len_before = app.event_log.len();

    app.input = "/request status".to_string();
    app.submit();

    // Exactly four log lines added.
    assert_eq!(app.log[log_len_before], "Model tool request status:");
    assert_eq!(
        app.log[log_len_before + 1],
        format!("- pending: {}", req.detail())
    );
    assert_eq!(
        app.log[log_len_before + 2],
        format!("- suggested command: {}", req.suggested_command())
    );
    assert_eq!(
        app.log[log_len_before + 3],
        "- next: run /context attach-last-tool after the tool succeeds"
    );
    assert_eq!(app.log.len(), log_len_before + 4);

    // Exactly one event appended and it is SlashCommand.
    assert_eq!(app.event_log.len(), event_len_before + 1);
    let new_event = app.event_log.get(event_len_before).unwrap();
    assert_eq!(new_event.kind, EventKind::SlashCommand);

    // Status must NOT clear the pending request.
    assert_eq!(
        app.pending_model_tool_request,
        Some(req),
        "/request status must not clear pending_model_tool_request"
    );

    // No RunCreate event.
    assert!(
        !app.event_log
            .events()
            .iter()
            .any(|e| e.kind == EventKind::RunCreate),
        "/request status must not start a model run"
    );
}

#[test]
fn request_clear_clears_pending_and_logs_message() {
    use kernel::model_tool_request::{ModelToolRequest, ModelToolRequestKind};

    let mut app = App::new();
    app.pending_model_tool_request = Some(ModelToolRequest {
        kind: ModelToolRequestKind::ReadFile,
        path: "README.md".to_string(),
    });

    let log_len_before = app.log.len();
    let event_len_before = app.event_log.len();

    app.input = "/request clear".to_string();
    app.submit();

    // pending_model_tool_request must be None after clear.
    assert!(
        app.pending_model_tool_request.is_none(),
        "pending_model_tool_request must be None after /request clear"
    );

    // Exactly one log line added.
    assert_eq!(
        app.log[log_len_before],
        "Cleared pending model tool request."
    );
    assert_eq!(app.log.len(), log_len_before + 1);

    // Exactly one event appended and it is SlashCommand.
    assert_eq!(app.event_log.len(), event_len_before + 1);
    let new_event = app.event_log.get(event_len_before).unwrap();
    assert_eq!(new_event.kind, EventKind::SlashCommand);

    // No RunCreate event.
    assert!(
        !app.event_log
            .events()
            .iter()
            .any(|e| e.kind == EventKind::RunCreate),
        "/request clear must not start a model run"
    );
}

#[test]
fn request_clear_without_pending_does_not_panic() {
    let mut app = App::new();
    assert!(app.pending_model_tool_request.is_none());

    let log_len_before = app.log.len();
    let event_len_before = app.event_log.len();

    // Must not panic.
    app.input = "/request clear".to_string();
    app.submit();

    assert!(app.pending_model_tool_request.is_none());

    // Exactly one log line added.
    assert_eq!(
        app.log[log_len_before],
        "Cleared pending model tool request."
    );
    assert_eq!(app.log.len(), log_len_before + 1);

    // Exactly one event appended and it is SlashCommand.
    assert_eq!(app.event_log.len(), event_len_before + 1);
    let new_event = app.event_log.get(event_len_before).unwrap();
    assert_eq!(new_event.kind, EventKind::SlashCommand);
}

// --- /request run tests ---

#[test]
fn request_run_without_pending() {
    let mut app = App::new();
    assert!(app.pending_model_tool_request.is_none());

    let event_len_before = app.event_log.len();

    app.input = "/request run".to_string();
    app.submit();

    let new_events = &app.event_log.events()[event_len_before..];

    // No ToolCall/ToolResult/ToolError events appended.
    assert!(
        !new_events.iter().any(|e| e.kind == EventKind::ToolCall),
        "must not emit ToolCall when no pending request"
    );
    assert!(
        !new_events.iter().any(|e| e.kind == EventKind::ToolResult),
        "must not emit ToolResult when no pending request"
    );
    assert!(
        !new_events.iter().any(|e| e.kind == EventKind::ToolError),
        "must not emit ToolError when no pending request"
    );
    // No model run events.
    assert!(
        !new_events.iter().any(|e| e.kind == EventKind::RunCreate),
        "must not start a model run"
    );
    assert!(
        !new_events.iter().any(|e| e.kind == EventKind::ModelRoute),
        "must not emit ModelRoute"
    );
    assert!(
        !new_events
            .iter()
            .any(|e| e.kind == EventKind::AssistantMessage),
        "must not emit AssistantMessage"
    );
    // "No pending model tool request." must appear in the log.
    assert!(
        app.log
            .iter()
            .any(|l| l == "No pending model tool request."),
        "log must contain 'No pending model tool request.'"
    );
    // pending_model_tool_request remains None (pending_model_tool_request is hidden from the default header).
    assert!(app.pending_model_tool_request.is_none());
}

#[test]
fn request_run_pending_read_file_success() {
    use kernel::manual_context::ManualToolContext;
    use kernel::model_tool_request::{ModelToolRequest, ModelToolRequestKind};

    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();
    std::fs::write(workspace_dir.path().join("notes.txt"), "hello world").unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    app.pending_model_tool_request = Some(ModelToolRequest {
        kind: ModelToolRequestKind::ReadFile,
        path: "notes.txt".to_string(),
    });

    // Pre-seed a sentinel pending_manual_tool_context.
    let sentinel_ctx = ManualToolContext::from_read_file("sentinel.txt", "sentinel content");
    let sentinel_summary = sentinel_ctx.attach_summary();
    app.pending_manual_tool_context = Some(sentinel_ctx);

    let event_len_before = app.event_log.len();

    app.input = "/request run".to_string();
    app.submit();

    let new_events = &app.event_log.events()[event_len_before..];

    // Events in order: SlashCommand → ToolPolicy → ToolCall → ToolResult.
    assert!(
        new_events.len() >= 4,
        "expected at least 4 new events, got {}",
        new_events.len()
    );
    assert_eq!(new_events[0].kind, EventKind::SlashCommand);
    assert_eq!(new_events[1].kind, EventKind::ToolPolicy);
    assert_eq!(new_events[2].kind, EventKind::ToolCall);
    assert_eq!(new_events[3].kind, EventKind::ToolResult);

    // No model run events.
    assert!(!new_events.iter().any(|e| e.kind == EventKind::RunCreate));
    assert!(!new_events.iter().any(|e| e.kind == EventKind::ModelRoute));
    assert!(
        !new_events
            .iter()
            .any(|e| e.kind == EventKind::AssistantMessage)
    );

    // pending_model_tool_request cleared on success (pending_model_tool_request is hidden from the default header).
    assert!(
        app.pending_model_tool_request.is_none(),
        "pending_model_tool_request must be None after successful /request run"
    );

    // last_tool_output_candidate updated.
    assert!(
        app.last_tool_output_candidate.is_some(),
        "last_tool_output_candidate must be Some after successful /request run"
    );

    // pending_manual_tool_context unchanged.
    assert_eq!(
        app.pending_manual_tool_context
            .as_ref()
            .map(|c| c.attach_summary()),
        Some(sentinel_summary),
        "pending_manual_tool_context must remain unchanged"
    );

    // Log contains read preview and attach-guidance line.
    assert!(
        app.log.iter().any(|l| l.contains("Tool read")),
        "log must contain read preview"
    );
    assert!(
        app.log.iter().any(|l| {
            l == "Run /context attach-last-tool to include this tool output in the next prompt."
        }),
        "log must contain attach-guidance line"
    );
}

#[test]
fn request_run_pending_list_files_success() {
    use kernel::model_tool_request::{ModelToolRequest, ModelToolRequestKind};

    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();
    std::fs::write(workspace_dir.path().join("file_a.txt"), "a").unwrap();
    std::fs::write(workspace_dir.path().join("file_b.txt"), "b").unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    app.pending_model_tool_request = Some(ModelToolRequest {
        kind: ModelToolRequestKind::ListFiles,
        path: ".".to_string(),
    });

    let event_len_before = app.event_log.len();

    app.input = "/request run".to_string();
    app.submit();

    let new_events = &app.event_log.events()[event_len_before..];

    // Events in order: SlashCommand → ToolPolicy → ToolCall → ToolResult.
    assert!(
        new_events.len() >= 4,
        "expected at least 4 new events, got {}",
        new_events.len()
    );
    assert_eq!(new_events[0].kind, EventKind::SlashCommand);
    assert_eq!(new_events[1].kind, EventKind::ToolPolicy);
    assert_eq!(new_events[2].kind, EventKind::ToolCall);
    assert_eq!(new_events[3].kind, EventKind::ToolResult);

    // No model run events.
    assert!(!new_events.iter().any(|e| e.kind == EventKind::RunCreate));
    assert!(!new_events.iter().any(|e| e.kind == EventKind::ModelRoute));
    assert!(
        !new_events
            .iter()
            .any(|e| e.kind == EventKind::AssistantMessage)
    );

    // pending cleared (pending_model_tool_request is hidden from the default header).
    assert!(
        app.pending_model_tool_request.is_none(),
        "pending_model_tool_request must be None after successful /request run"
    );

    // last_tool_output_candidate updated.
    assert!(
        app.last_tool_output_candidate.is_some(),
        "last_tool_output_candidate must be Some after successful /request run"
    );

    // Log contains list preview and attach-guidance line.
    assert!(
        app.log.iter().any(|l| l.contains("Tool list")),
        "log must contain list preview"
    );
    assert!(
        app.log.iter().any(|l| {
            l == "Run /context attach-last-tool to include this tool output in the next prompt."
        }),
        "log must contain attach-guidance line"
    );
}

#[test]
fn request_run_pending_path_violation_tool_error() {
    use kernel::manual_context::ManualToolContext;
    use kernel::model_tool_request::{ModelToolRequest, ModelToolRequestKind};

    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    let escaping_req = ModelToolRequest {
        kind: ModelToolRequestKind::ReadFile,
        path: "../../etc/passwd".to_string(),
    };
    app.pending_model_tool_request = Some(escaping_req.clone());

    // Pre-seed sentinel values for both candidates.
    let sentinel_candidate = ManualToolContext::from_read_file("sentinel.txt", "sentinel content");
    let sentinel_candidate_summary = sentinel_candidate.attach_summary();
    app.last_tool_output_candidate = Some(sentinel_candidate);

    let sentinel_ctx = ManualToolContext::from_read_file("ctx_sentinel.txt", "ctx content");
    let sentinel_ctx_summary = sentinel_ctx.attach_summary();
    app.pending_manual_tool_context = Some(sentinel_ctx);

    let event_len_before = app.event_log.len();

    app.input = "/request run".to_string();
    app.submit();

    let new_events = &app.event_log.events()[event_len_before..];

    // Events in order: SlashCommand → ToolPolicy → ToolCall → ToolError.
    assert!(
        new_events.len() >= 4,
        "expected at least 4 new events, got {}",
        new_events.len()
    );
    assert_eq!(new_events[0].kind, EventKind::SlashCommand);
    assert_eq!(new_events[1].kind, EventKind::ToolPolicy);
    assert_eq!(new_events[2].kind, EventKind::ToolCall);
    assert_eq!(new_events[3].kind, EventKind::ToolError);

    // No model run events.
    assert!(!new_events.iter().any(|e| e.kind == EventKind::RunCreate));
    assert!(!new_events.iter().any(|e| e.kind == EventKind::ModelRoute));
    assert!(
        !new_events
            .iter()
            .any(|e| e.kind == EventKind::AssistantMessage)
    );

    // pending_model_tool_request kept (pending_model_tool_request is hidden from the default header).
    assert!(
        app.pending_model_tool_request.is_some(),
        "pending_model_tool_request must remain Some after tool error"
    );

    // Both sentinels unchanged.
    assert_eq!(
        app.last_tool_output_candidate
            .as_ref()
            .map(|c| c.attach_summary()),
        Some(sentinel_candidate_summary),
        "last_tool_output_candidate must be unchanged on failure"
    );
    assert_eq!(
        app.pending_manual_tool_context
            .as_ref()
            .map(|c| c.attach_summary()),
        Some(sentinel_ctx_summary),
        "pending_manual_tool_context must be unchanged on failure"
    );

    // Log contains a tool error message.
    assert!(
        app.log.iter().any(|l| l.contains("Tool error:")),
        "log must contain a tool error message"
    );
}

// --- New /request run event-sequence tests (Step 9) ---

#[test]
fn request_run_pending_success_appends_slash_policy_call_result() {
    use kernel::model_tool_request::{ModelToolRequest, ModelToolRequestKind};

    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();
    std::fs::write(workspace_dir.path().join("notes.txt"), "hello world").unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    app.pending_model_tool_request = Some(ModelToolRequest {
        kind: ModelToolRequestKind::ReadFile,
        path: "notes.txt".to_string(),
    });

    let event_len_before = app.event_log.len();
    app.input = "/request run".to_string();
    app.submit();

    let new_events = &app.event_log.events()[event_len_before..];

    assert_eq!(new_events.len(), 4);
    assert_eq!(new_events[0].kind, EventKind::SlashCommand);
    assert_eq!(new_events[1].kind, EventKind::ToolPolicy);
    assert_eq!(new_events[2].kind, EventKind::ToolCall);
    assert_eq!(new_events[3].kind, EventKind::ToolResult);
}

#[test]
fn request_run_pending_failure_appends_slash_policy_call_error() {
    use kernel::model_tool_request::{ModelToolRequest, ModelToolRequestKind};

    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    app.pending_model_tool_request = Some(ModelToolRequest {
        kind: ModelToolRequestKind::ReadFile,
        path: "../secret.txt".to_string(),
    });

    let event_len_before = app.event_log.len();
    app.input = "/request run".to_string();
    app.submit();

    let new_events = &app.event_log.events()[event_len_before..];

    assert_eq!(new_events.len(), 4);
    assert_eq!(new_events[0].kind, EventKind::SlashCommand);
    assert_eq!(new_events[1].kind, EventKind::ToolPolicy);
    assert_eq!(new_events[2].kind, EventKind::ToolCall);
    assert_eq!(new_events[3].kind, EventKind::ToolError);
}
