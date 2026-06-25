use super::super::*;
use super::common::*;
use kernel::events::EventKind;
use kernel::storage::EventStore;

// --- /context command tests ---

#[test]
fn context_attach_with_candidate_appends_tool_context_attach_and_no_run() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();
    std::fs::write(workspace_dir.path().join("data.txt"), "some content").unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    // Populate the candidate by reading a file.
    app.input = "/tool read data.txt".to_string();
    app.submit();

    let event_len_before = app.event_log.len();
    app.input = "/context attach-last-tool".to_string();
    app.submit();

    let events = app.event_log.events();
    assert!(
        events
            .iter()
            .any(|e| e.kind == EventKind::ToolContextAttach),
        "ToolContextAttach event should be present"
    );
    assert!(
        !events.iter().any(|e| e.kind == EventKind::RunCreate),
        "/context attach-last-tool must not start a model run"
    );
    assert!(
        !events.iter().any(|e| e.kind == EventKind::RunComplete),
        "/context attach-last-tool must not complete a model run"
    );
    assert!(
        !events.iter().any(|e| e.kind == EventKind::AssistantMessage),
        "/context attach-last-tool must not produce AssistantMessage"
    );
    // SlashCommand + ToolContextAttach = 2 events appended.
    assert_eq!(app.event_log.len(), event_len_before + 2);
    assert!(app.pending_manual_tool_context.is_some());
}

#[test]
fn context_attach_without_candidate_appends_no_tool_context_attach_and_no_run() {
    let mut app = App::new();
    assert!(app.last_tool_output_candidate.is_none());

    let event_len_before = app.event_log.len();
    app.input = "/context attach-last-tool".to_string();
    app.submit(); // must not panic

    let events = app.event_log.events();
    assert!(
        !events
            .iter()
            .any(|e| e.kind == EventKind::ToolContextAttach),
        "no ToolContextAttach should be emitted when there is no candidate"
    );
    assert!(
        !events.iter().any(|e| e.kind == EventKind::RunCreate),
        "/context attach-last-tool with no candidate must not start a run"
    );
    // SlashCommand only (no ToolContextAttach).
    assert_eq!(app.event_log.len(), event_len_before + 1);
    assert!(app.pending_manual_tool_context.is_none());
}

#[test]
fn context_clear_appends_tool_context_clear_and_no_run() {
    let mut app = App::new();

    let event_len_before = app.event_log.len();
    app.input = "/context clear".to_string();
    app.submit();

    let events = app.event_log.events();
    assert!(
        events.iter().any(|e| e.kind == EventKind::ToolContextClear),
        "ToolContextClear event should be present"
    );
    assert!(
        !events.iter().any(|e| e.kind == EventKind::RunCreate),
        "/context clear must not start a model run"
    );
    // SlashCommand + ToolContextClear = 2 events.
    assert_eq!(app.event_log.len(), event_len_before + 2);
    assert!(app.pending_manual_tool_context.is_none());
}

#[test]
fn context_clear_when_nothing_pending_does_not_panic() {
    let mut app = App::new();
    assert!(app.pending_manual_tool_context.is_none());
    app.input = "/context clear".to_string();
    app.submit(); // must not panic
    assert!(app.pending_manual_tool_context.is_none());
}

#[test]
fn after_attach_user_message_prompt_compile_contains_manual_tool_context() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();
    let file_content = "bounded file body";
    std::fs::write(workspace_dir.path().join("notes.txt"), file_content).unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    app.input = "/tool read notes.txt".to_string();
    app.submit();

    app.input = "/context attach-last-tool".to_string();
    app.submit();

    // The pending context should be set.
    assert!(app.pending_manual_tool_context.is_some());

    app.input = "tell me about the file".to_string();
    app.submit();

    // After the user message, the context should be cleared (one-shot).
    assert!(app.pending_manual_tool_context.is_none());

    let events = app.event_log.events();
    let pc = events
        .iter()
        .filter(|e| e.kind == EventKind::PromptCompile)
        .next()
        .expect("PromptCompile event should exist");

    // The bounded content must appear in the Context section (after "Context:").
    assert!(
        pc.detail.contains("Attached Workspace Context:"),
        "PromptCompile must contain Attached Workspace Context: section"
    );
    assert!(
        pc.detail.contains(file_content),
        "PromptCompile must contain bounded file content"
    );

    // The bounded content must NOT appear in the Conversation section.
    let conversation_section = pc
        .detail
        .split("\n\nCurrent User:")
        .next()
        .expect("Conversation section must exist");
    assert!(
        !conversation_section.contains(file_content),
        "bounded content must not appear in the Conversation section"
    );
}

#[test]
fn after_attach_second_user_message_does_not_reuse_context() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();
    let file_content = "unique attached content xyz";
    std::fs::write(workspace_dir.path().join("unique.txt"), file_content).unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    app.input = "/tool read unique.txt".to_string();
    app.submit();

    app.input = "/context attach-last-tool".to_string();
    app.submit();

    // First user message consumes the context.
    app.input = "first message".to_string();
    app.submit();

    // Second user message must NOT have the context.
    app.input = "second message".to_string();
    app.submit();

    let events = app.event_log.events();
    let second_pc = events
        .iter()
        .filter(|e| e.kind == EventKind::PromptCompile)
        .nth(1)
        .expect("second PromptCompile event should exist");

    assert!(
        !second_pc.detail.contains("Attached Workspace Context:"),
        "second PromptCompile must NOT contain Attached Workspace Context"
    );
    assert!(
        !second_pc.detail.contains(file_content),
        "second PromptCompile must NOT contain bounded content (one-shot consumed)"
    );
}

#[test]
fn one_shot_auto_clear_emits_no_tool_context_clear_event() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();
    std::fs::write(workspace_dir.path().join("sample.txt"), "data").unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    app.input = "/tool read sample.txt".to_string();
    app.submit();

    app.input = "/context attach-last-tool".to_string();
    app.submit();

    // Verify ToolContextClear is NOT in the log yet.
    assert!(
        !app.event_log
            .events()
            .iter()
            .any(|e| e.kind == EventKind::ToolContextClear),
        "no ToolContextClear before user message"
    );

    // Consuming user message — triggers one-shot clear internally.
    app.input = "consume the context".to_string();
    app.submit();

    // ToolContextClear must still NOT appear (auto-clear emits no event).
    assert!(
        !app.event_log
            .events()
            .iter()
            .any(|e| e.kind == EventKind::ToolContextClear),
        "one-shot auto-clear must NOT emit ToolContextClear event"
    );
}

#[test]
fn attached_content_does_not_appear_in_assistant_message_or_transcript() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();
    let file_content = "super_secret_content_xyz_12345";
    std::fs::write(workspace_dir.path().join("secret.txt"), file_content).unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    app.input = "/tool read secret.txt".to_string();
    app.submit();

    app.input = "/context attach-last-tool".to_string();
    app.submit();

    app.input = "what is in the file".to_string();
    app.submit();

    let events = app.event_log.events();

    // AssistantMessage detail must not contain the raw file content.
    for ev in events
        .iter()
        .filter(|e| e.kind == EventKind::AssistantMessage)
    {
        assert!(
            !ev.detail.contains(file_content),
            "AssistantMessage must not contain raw file content: {}",
            ev.detail
        );
    }

    // ConversationTranscript projection must not contain the raw file content.
    let transcript = kernel::transcript::ConversationTranscript::from_event_log(&app.event_log);
    for msg in &transcript.messages {
        assert!(
            !msg.content.contains(file_content),
            "ConversationTranscript must not contain raw file content: {}",
            msg.content
        );
    }
}

#[test]
fn tool_context_attach_event_detail_does_not_contain_full_file_content() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();
    let file_content = "very_unique_raw_content_do_not_expose_9999";
    std::fs::write(workspace_dir.path().join("expose.txt"), file_content).unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    app.input = "/tool read expose.txt".to_string();
    app.submit();

    app.input = "/context attach-last-tool".to_string();
    app.submit();

    let events = app.event_log.events();
    let attach_ev = events
        .iter()
        .find(|e| e.kind == EventKind::ToolContextAttach)
        .expect("ToolContextAttach event should exist");

    assert!(
        !attach_ev.detail.contains(file_content),
        "ToolContextAttach detail must not contain raw file content: {}",
        attach_ev.detail
    );
    assert!(
        attach_ev.detail.contains("risk=read_only"),
        "ToolContextAttach detail should contain risk=read_only: {}",
        attach_ev.detail
    );
    assert!(
        attach_ev.detail.contains("bytes="),
        "ToolContextAttach detail should contain bytes=: {}",
        attach_ev.detail
    );
}

#[test]
fn context_unknown_produces_unknown_slash_command() {
    let mut app = App::new();
    app.input = "/context unknown".to_string();
    app.submit();

    let events = app.event_log.events();
    assert!(
        events
            .iter()
            .any(|e| e.kind == EventKind::UnknownSlashCommand),
        "/context unknown must produce UnknownSlashCommand event"
    );
    assert!(
        !events.iter().any(|e| e.kind == EventKind::RunCreate),
        "/context unknown must not start a run"
    );
}

// --- /context status tests ---

#[test]
fn context_status_appends_only_slash_command_and_no_run() {
    let mut app = App::new();
    let before = app.event_log.len();
    app.input = "/context status".to_string();
    app.submit();

    assert_eq!(
        app.event_log.len(),
        before + 1,
        "only one SlashCommand event should be appended"
    );

    let events = app.event_log.events();
    let last = &events[events.len() - 1];
    assert_eq!(last.kind, EventKind::SlashCommand);

    assert!(!events.iter().any(|e| e.kind == EventKind::RunCreate));
    assert!(!events.iter().any(|e| e.kind == EventKind::RunComplete));
    assert!(!events.iter().any(|e| e.kind == EventKind::AssistantMessage));
    assert!(
        !events
            .iter()
            .any(|e| e.kind == EventKind::ToolContextAttach)
    );
    assert!(!events.iter().any(|e| e.kind == EventKind::ToolContextClear));
}

#[test]
fn context_status_no_context_shows_none() {
    let mut app = App::new();
    app.input = "/context status".to_string();
    app.submit();

    assert!(
        app.log.contains(&"- pending: none".to_string()),
        "log should contain '- pending: none'"
    );
    assert!(
        app.log.contains(&"- last tool output: none".to_string()),
        "log should contain '- last tool output: none'"
    );
}

#[test]
fn context_status_candidate_only_shows_pending_none() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();
    std::fs::write(workspace_dir.path().join("file.txt"), "some content").unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    // Populate last_tool_output_candidate but do NOT attach.
    app.input = "/tool read file.txt".to_string();
    app.submit();

    app.input = "/context status".to_string();
    app.submit();

    // After T-1, /tool read auto-sets pending_manual_tool_context.
    // The pending line now reflects the auto-set read, not "none".
    assert!(
        app.log.iter().any(|l| l.starts_with("- pending: tool=")),
        "pending should reflect the auto-set read (tool= summary line)"
    );
    assert!(
        app.log
            .iter()
            .any(|l| l.starts_with("- last tool output: tool=")),
        "last tool output line should start with 'tool='"
    );
    let last_tool_line = app
        .log
        .iter()
        .find(|l| l.starts_with("- last tool output: tool="))
        .expect("last tool output line must be present");
    assert!(
        last_tool_line.contains("risk=read_only"),
        "last tool output line should contain risk=read_only: {last_tool_line}"
    );
    assert!(
        last_tool_line.contains("bytes="),
        "last tool output line should contain bytes=: {last_tool_line}"
    );
    assert!(
        last_tool_line.contains("truncated="),
        "last tool output line should contain truncated=: {last_tool_line}"
    );
    assert!(
        !last_tool_line.contains("some content"),
        "last tool output line must not contain raw file content: {last_tool_line}"
    );
}

#[test]
fn context_status_pending_shows_pending_summary() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();
    std::fs::write(workspace_dir.path().join("data.txt"), "hello world").unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    app.input = "/tool read data.txt".to_string();
    app.submit();

    app.input = "/context attach-last-tool".to_string();
    app.submit();

    app.input = "/context status".to_string();
    app.submit();

    assert!(
        app.log.iter().any(|l| l.starts_with("- pending: tool=")),
        "pending line should start with 'tool=' after attach"
    );
    let pending_line = app
        .log
        .iter()
        .find(|l| l.starts_with("- pending: tool="))
        .expect("pending line must be present");
    assert!(
        pending_line.contains("risk=read_only"),
        "pending line should contain risk=read_only: {pending_line}"
    );
    assert!(
        pending_line.contains("bytes="),
        "pending line should contain bytes=: {pending_line}"
    );
    assert!(
        pending_line.contains("truncated="),
        "pending line should contain truncated=: {pending_line}"
    );
    assert!(
        !pending_line.contains("hello world"),
        "pending line must not contain raw file content: {pending_line}"
    );
}

#[test]
fn context_status_pending_and_candidate_both_present() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();

    // fileA and fileB have distinct paths and contents so their summaries differ.
    std::fs::write(workspace_dir.path().join("fileA.txt"), "content of file A").unwrap();
    std::fs::write(
        workspace_dir.path().join("fileB.txt"),
        "content of file B different",
    )
    .unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    // Read fileA and attach it (pending = fileA's summary).
    app.input = "/tool read fileA.txt".to_string();
    app.submit();
    let file_a_summary = app
        .last_tool_output_candidate
        .as_ref()
        .unwrap()
        .attach_summary();

    app.input = "/context attach-last-tool".to_string();
    app.submit();

    // Read fileB — after T-1 (D4 last-write-wins), this auto-sets pending to
    // fileB's summary, overwriting the previously attached fileA pending.
    app.input = "/tool read fileB.txt".to_string();
    app.submit();
    let file_b_summary = app
        .last_tool_output_candidate
        .as_ref()
        .unwrap()
        .attach_summary();

    assert_ne!(
        file_a_summary, file_b_summary,
        "fileA and fileB summaries must differ for this test to be meaningful"
    );

    app.input = "/context status".to_string();
    app.submit();

    // Find the "Workspace context status:" line in the log.
    let status_pos = app
        .log
        .iter()
        .rposition(|l| l == "Workspace context status:")
        .expect("'Workspace context status:' line must be present");

    assert_eq!(app.log[status_pos], "Workspace context status:");
    // D4 last-write-wins: reading fileB overwrites pending to fileB's summary.
    assert_eq!(
        app.log[status_pos + 1],
        format!("- pending: {}", file_b_summary),
        "pending line must contain fileB summary (last-write-wins)"
    );
    assert_eq!(
        app.log[status_pos + 2],
        format!("- last tool output: {}", file_b_summary),
        "last tool output line must contain fileB summary"
    );
}

#[test]
fn context_status_does_not_mutate_context_state() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();
    std::fs::write(workspace_dir.path().join("note.txt"), "important data").unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    // Stage both pending and candidate.
    app.input = "/tool read note.txt".to_string();
    app.submit();

    app.input = "/context attach-last-tool".to_string();
    app.submit();

    assert!(app.pending_manual_tool_context.is_some());
    assert!(app.last_tool_output_candidate.is_some());

    let pending_summary_before = app
        .pending_manual_tool_context
        .as_ref()
        .unwrap()
        .attach_summary();
    let candidate_summary_before = app
        .last_tool_output_candidate
        .as_ref()
        .unwrap()
        .attach_summary();

    // Run /context status — must not mutate either field.
    app.input = "/context status".to_string();
    app.submit();

    assert!(
        app.pending_manual_tool_context.is_some(),
        "pending_manual_tool_context must remain Some after /context status"
    );
    assert!(
        app.last_tool_output_candidate.is_some(),
        "last_tool_output_candidate must remain Some after /context status"
    );
    assert_eq!(
        app.pending_manual_tool_context
            .as_ref()
            .unwrap()
            .attach_summary(),
        pending_summary_before,
        "pending context must be unchanged"
    );
    assert_eq!(
        app.last_tool_output_candidate
            .as_ref()
            .unwrap()
            .attach_summary(),
        candidate_summary_before,
        "last tool output candidate must be unchanged"
    );

    // A subsequent user message should still consume the pending context.
    app.input = "use the context".to_string();
    app.submit();

    assert!(
        app.pending_manual_tool_context.is_none(),
        "pending context must be consumed (None) after the user message"
    );
}

#[test]
fn context_status_output_excludes_raw_content() {
    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();
    let raw_content = "raw_secret_xyz_do_not_expose_in_status_98765";
    std::fs::write(workspace_dir.path().join("secret.txt"), raw_content).unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    app.input = "/tool read secret.txt".to_string();
    app.submit();

    app.input = "/context status".to_string();
    app.submit();

    // Find the three status lines in the log.
    let status_pos = app
        .log
        .iter()
        .rposition(|l| l == "Workspace context status:")
        .expect("'Workspace context status:' line must be present");

    for i in 0..3 {
        assert!(
            !app.log[status_pos + i].contains(raw_content),
            "status line {} must not contain raw file content: {}",
            i,
            app.log[status_pos + i]
        );
    }
}

#[test]
fn prompt_context_survives_clear() {
    // /clear is a screen-only command: it clears the visible log but not the
    // event log, so prior conversation must still appear in a later turn's
    // compiled prompt. This locks /clear as NOT a prompt-context boundary.
    let mut app = App::new();

    app.input = "first".to_string();
    app.submit();

    app.input = "/clear".to_string();
    app.submit();

    app.input = "second".to_string();
    app.submit();

    let events = app.event_log.events();
    let second_pc = events
        .iter()
        .filter(|e| e.kind == EventKind::PromptCompile)
        .nth(1)
        .expect("second PromptCompile event should exist");

    assert!(second_pc.detail.contains("User: first"));
    assert!(
        second_pc
            .detail
            .contains("Assistant: Mock response for: first")
    );
    assert!(second_pc.detail.contains("Current User:\nsecond"));
    assert!(!second_pc.detail.contains("No prior conversation context."));
}

#[test]
fn request_run_success_then_attach_last_tool_includes_output() {
    use kernel::model_tool_request::{ModelToolRequest, ModelToolRequestKind};

    let store_dir = TempDir::new();
    let workspace_dir = TempDir::new();
    let file_content = "caravan tool output";
    std::fs::write(workspace_dir.path().join("output.txt"), file_content).unwrap();

    let store = EventStore::new(store_dir.path());
    let mut app = App::with_store_gateway_and_workspace_root(
        store,
        kernel::model_gateway::ModelGateway::default(),
        workspace_dir.path().to_path_buf(),
    );

    app.pending_model_tool_request = Some(ModelToolRequest {
        kind: ModelToolRequestKind::ReadFile,
        path: "output.txt".to_string(),
    });

    // Step 1: /request run (success).
    app.input = "/request run".to_string();
    app.submit();
    assert!(
        app.pending_model_tool_request.is_none(),
        "pending must be cleared after successful run"
    );
    assert!(
        app.last_tool_output_candidate.is_some(),
        "candidate must be set after successful run"
    );

    // Step 2: /context attach-last-tool.
    app.input = "/context attach-last-tool".to_string();
    app.submit();
    assert!(
        app.pending_manual_tool_context.is_some(),
        "pending_manual_tool_context must be set after attach"
    );

    // Step 3: user message — the PromptCompile event must include the tool output.
    app.input = "use the output".to_string();
    app.submit();

    let events = app.event_log.events();
    let prompt_compile = events
        .iter()
        .filter(|e| e.kind == EventKind::PromptCompile)
        .last()
        .expect("PromptCompile event should exist");

    assert!(
        prompt_compile.detail.contains(file_content),
        "PromptCompile detail must include the tool output content"
    );
}
