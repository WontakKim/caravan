use serde::{Deserialize, Serialize};

use crate::storage::EventStore;

mod ids;
pub use ids::{EventSeq, RunId, TurnId};

/// The kind of application event that occurred.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum EventKind {
    AppStart,
    SlashCommand,
    HelpRequest,
    UserMessage,
    LogClear,
    ExitRequest,
    UnknownSlashCommand,
    RunCreate,
    RunStart,
    TurnStart,
    PromptCompile,
    ModelRoute,
    ModelOutputChunk,
    AssistantMessage,
    ModelUsage,
    RunComplete,
    RunFail,
    ModelError,
    ModelToolRequest,
    ToolPolicy,
    ToolCall,
    ToolResult,
    ToolError,
    ToolContextAttach,
    ToolContextClear,
}

impl EventKind {
    /// Returns the name of this variant as a static string.
    pub fn name(&self) -> &'static str {
        match self {
            EventKind::AppStart => "AppStart",
            EventKind::SlashCommand => "SlashCommand",
            EventKind::HelpRequest => "HelpRequest",
            EventKind::UserMessage => "UserMessage",
            EventKind::LogClear => "LogClear",
            EventKind::ExitRequest => "ExitRequest",
            EventKind::UnknownSlashCommand => "UnknownSlashCommand",
            EventKind::RunCreate => "RunCreate",
            EventKind::RunStart => "RunStart",
            EventKind::TurnStart => "TurnStart",
            EventKind::PromptCompile => "PromptCompile",
            EventKind::ModelRoute => "ModelRoute",
            EventKind::ModelOutputChunk => "ModelOutputChunk",
            EventKind::AssistantMessage => "AssistantMessage",
            EventKind::ModelUsage => "ModelUsage",
            EventKind::RunComplete => "RunComplete",
            EventKind::RunFail => "RunFail",
            EventKind::ModelError => "ModelError",
            EventKind::ModelToolRequest => "ModelToolRequest",
            EventKind::ToolPolicy => "ToolPolicy",
            EventKind::ToolCall => "ToolCall",
            EventKind::ToolResult => "ToolResult",
            EventKind::ToolError => "ToolError",
            EventKind::ToolContextAttach => "ToolContextAttach",
            EventKind::ToolContextClear => "ToolContextClear",
        }
    }
}

/// A single application event with its sequence number, kind, and detail string.
#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub struct AppEvent {
    pub seq: EventSeq,
    pub kind: EventKind,
    pub detail: String,
}

/// An append-only log of application events with monotonically increasing sequence numbers.
pub struct EventLog {
    events: Vec<AppEvent>,
    next_seq: u64,
    store: Option<EventStore>,
}

impl EventLog {
    /// Creates a new, empty in-memory event log. The first appended event will have seq = 1.
    /// No persistence: `store` is `None`.
    pub fn new() -> Self {
        EventLog {
            events: Vec::new(),
            next_seq: 1,
            store: None,
        }
    }

    /// Constructs a store-backed event log: ensures the store directory exists,
    /// loads any previously persisted events, and sets `next_seq` to continue
    /// the sequence (`max(seq) + 1`, or `1` when there are no prior events).
    pub fn load_from(store: EventStore) -> EventLog {
        store.ensure_store_dir().ok();
        let events = store.load_events();
        let next_seq = events
            .iter()
            .map(|e| e.seq.0)
            .max()
            .map(|m| m + 1)
            .unwrap_or(1);
        EventLog {
            events,
            next_seq,
            store: Some(store),
        }
    }

    /// Appends a new event, assigns the current sequence number, increments the counter,
    /// and returns the assigned sequence number. When a store is present, the event is
    /// persisted best-effort (write failures are silently discarded) before being pushed
    /// into memory.
    pub fn append(&mut self, kind: EventKind, detail: impl Into<String>) -> EventSeq {
        let seq = EventSeq(self.next_seq);
        let event = AppEvent {
            seq,
            kind,
            detail: detail.into(),
        };
        if let Some(store) = &self.store {
            store.append_event(&event).ok();
        }
        self.events.push(event);
        self.next_seq += 1;
        seq
    }

    /// Returns the number of events in the log.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Returns `true` if the log contains no events.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Returns the event at the given index, or `None` if out of bounds.
    pub fn get(&self, index: usize) -> Option<&AppEvent> {
        self.events.get(index)
    }

    /// Returns a slice of all events in the log.
    pub fn events(&self) -> &[AppEvent] {
        &self.events
    }

    /// Returns the sequence number that will be assigned to the next appended event.
    pub fn next_seq_value(&self) -> u64 {
        self.next_seq
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::storage::EventStore;

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TempDir {
        path: std::path::PathBuf,
    }

    impl TempDir {
        fn new() -> Self {
            let count = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
            let name = format!("caravan_evtest_{}_{}", std::process::id(), count);
            let path = std::env::temp_dir().join(name);
            std::fs::create_dir_all(&path).expect("failed to create temp dir");
            TempDir { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn restart_reloads_events_and_continues_seq() {
        let dir = TempDir::new();

        // First "run": build a store-backed log, append a few events.
        let store1 = EventStore::new(dir.path());
        let mut log1 = EventLog::load_from(store1);
        log1.append(EventKind::AppStart, "first start");
        log1.append(EventKind::UserMessage, "hello");
        log1.append(EventKind::UserMessage, "world");
        let max_seq = log1.get(log1.len() - 1).unwrap().seq.0;
        drop(log1);

        // Second "run": reload from the same directory.
        let store2 = EventStore::new(dir.path());
        let mut log2 = EventLog::load_from(store2);

        // All prior events are present.
        assert_eq!(log2.len(), 3);
        assert_eq!(log2.get(0).unwrap().kind, EventKind::AppStart);
        assert_eq!(log2.get(1).unwrap().kind, EventKind::UserMessage);
        assert_eq!(log2.get(2).unwrap().kind, EventKind::UserMessage);

        // Next appended event continues the sequence.
        let new_seq = log2.append(EventKind::SlashCommand, "cmd");
        assert_eq!(new_seq.0, max_seq + 1);
    }

    #[test]
    fn first_append_returns_seq_one() {
        let mut log = EventLog::new();
        let seq = log.append(EventKind::AppStart, "started");
        assert_eq!(seq, EventSeq(1));
    }

    #[test]
    fn second_append_returns_seq_two() {
        let mut log = EventLog::new();
        log.append(EventKind::AppStart, "started");
        let seq = log.append(EventKind::SlashCommand, "cmd");
        assert_eq!(seq, EventSeq(2));
    }

    #[test]
    fn new_log_is_empty() {
        let log = EventLog::new();
        assert!(log.is_empty());
        assert_eq!(log.len(), 0);
    }

    #[test]
    fn len_and_is_empty_reflect_appended_events() {
        let mut log = EventLog::new();
        log.append(EventKind::UserMessage, "hello");
        assert!(!log.is_empty());
        assert_eq!(log.len(), 1);
        log.append(EventKind::ExitRequest, "");
        assert_eq!(log.len(), 2);
    }

    #[test]
    fn get_returns_correct_event() {
        let mut log = EventLog::new();
        log.append(EventKind::HelpRequest, "help detail");
        let event = log.get(0).expect("event at index 0 should exist");
        assert_eq!(event.kind, EventKind::HelpRequest);
        assert_eq!(event.detail, "help detail");
    }

    #[test]
    fn get_out_of_bounds_returns_none() {
        let log = EventLog::new();
        assert!(log.get(0).is_none());
    }

    #[test]
    fn stored_event_preserves_kind_and_detail() {
        let mut log = EventLog::new();
        log.append(EventKind::LogClear, "cleared by user");
        let event = log.get(0).unwrap();
        assert_eq!(event.kind, EventKind::LogClear);
        assert_eq!(event.detail, "cleared by user");
    }

    #[test]
    fn events_slice_matches_appended_events() {
        let mut log = EventLog::new();
        log.append(EventKind::AppStart, "a");
        log.append(EventKind::UnknownSlashCommand, "b");
        let events = log.events();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, EventKind::AppStart);
        assert_eq!(events[1].kind, EventKind::UnknownSlashCommand);
    }

    #[test]
    fn app_event_serializes_to_expected_jsonl() {
        let event = AppEvent {
            seq: EventSeq(1),
            kind: EventKind::AppStart,
            detail: "Caravan started.".into(),
        };
        let json = serde_json::to_string(&event).expect("serialization should succeed");
        assert_eq!(
            json,
            r#"{"seq":1,"kind":"AppStart","detail":"Caravan started."}"#
        );
    }

    #[test]
    fn app_event_json_round_trip() {
        let original = AppEvent {
            seq: EventSeq(1),
            kind: EventKind::AppStart,
            detail: "Caravan started.".into(),
        };
        let json = serde_json::to_string(&original).expect("serialization should succeed");
        let restored: AppEvent =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(original, restored);

        let v: serde_json::Value =
            serde_json::from_str(&json).expect("parsing to Value should succeed");
        assert_eq!(v["seq"], 1);
        assert_eq!(v["kind"], "AppStart");
        assert_eq!(v["detail"], "Caravan started.");
    }

    #[test]
    fn event_kind_json_round_trip() {
        for kind in [EventKind::ExitRequest, EventKind::UnknownSlashCommand] {
            let json = serde_json::to_string(&kind).expect("serialization should succeed");
            let restored: EventKind =
                serde_json::from_str(&json).expect("deserialization should succeed");
            assert_eq!(kind, restored);
        }
    }

    #[test]
    fn run_turn_event_kinds_json_round_trip() {
        let new_kinds = [
            EventKind::RunCreate,
            EventKind::RunStart,
            EventKind::TurnStart,
            EventKind::PromptCompile,
            EventKind::ModelRoute,
            EventKind::ModelOutputChunk,
            EventKind::ModelUsage,
            EventKind::RunComplete,
            EventKind::RunFail,
        ];
        for (i, kind) in new_kinds.iter().enumerate() {
            let event = AppEvent {
                seq: EventSeq((i + 1) as u64),
                kind: *kind,
                detail: format!("detail-{i}"),
            };
            let json = serde_json::to_string(&event).expect("serialization should succeed");
            let restored: AppEvent =
                serde_json::from_str(&json).expect("deserialization should succeed");
            assert_eq!(event, restored);
        }

        // Assert the serialized `kind` field is the variant-name string for RunCreate and ModelOutputChunk.
        let run_create_event = AppEvent {
            seq: EventSeq(1),
            kind: EventKind::RunCreate,
            detail: String::new(),
        };
        let json = serde_json::to_string(&run_create_event).expect("serialization should succeed");
        let v: serde_json::Value =
            serde_json::from_str(&json).expect("parsing to Value should succeed");
        assert_eq!(v["kind"], "RunCreate");

        let model_output_chunk_event = AppEvent {
            seq: EventSeq(2),
            kind: EventKind::ModelOutputChunk,
            detail: String::new(),
        };
        let json =
            serde_json::to_string(&model_output_chunk_event).expect("serialization should succeed");
        let v: serde_json::Value =
            serde_json::from_str(&json).expect("parsing to Value should succeed");
        assert_eq!(v["kind"], "ModelOutputChunk");
    }

    #[test]
    fn model_route_event_kind_serializes_and_round_trips() {
        let event = AppEvent {
            seq: EventSeq(1),
            kind: EventKind::ModelRoute,
            detail: "provider=mock model=mock-model adapter=MockModelAdapter".into(),
        };
        let json = serde_json::to_string(&event).expect("serialization should succeed");
        let v: serde_json::Value =
            serde_json::from_str(&json).expect("parsing to Value should succeed");
        assert_eq!(v["kind"], "ModelRoute");
        let restored: AppEvent =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(event, restored);
        assert_eq!(
            restored.detail,
            "provider=mock model=mock-model adapter=MockModelAdapter"
        );
    }

    #[test]
    fn model_error_event_kind_serializes_and_round_trips() {
        let event = AppEvent {
            seq: EventSeq(1),
            kind: EventKind::ModelError,
            detail: "model layer failure".into(),
        };
        let json = serde_json::to_string(&event).expect("serialization should succeed");
        let v: serde_json::Value =
            serde_json::from_str(&json).expect("parsing to Value should succeed");
        assert_eq!(v["kind"], "ModelError");
        let restored: AppEvent =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(event, restored);
        assert_eq!(restored.kind, EventKind::ModelError);
    }

    #[test]
    fn model_tool_request_event_kind_serializes_and_round_trips() {
        assert_eq!(EventKind::ModelToolRequest.name(), "ModelToolRequest");

        let event = AppEvent {
            seq: EventSeq(1),
            kind: EventKind::ModelToolRequest,
            detail: "detected CARAVAN_TOOL_REQUEST block".into(),
        };
        let json = serde_json::to_string(&event).expect("serialization should succeed");
        assert!(json.contains("ModelToolRequest"));
        let v: serde_json::Value =
            serde_json::from_str(&json).expect("parsing to Value should succeed");
        assert_eq!(v["kind"], "ModelToolRequest");
        let restored: AppEvent =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(event, restored);
    }

    #[test]
    fn tool_call_event_kind_name_and_json_round_trip() {
        assert_eq!(EventKind::ToolCall.name(), "ToolCall");

        let event = AppEvent {
            seq: EventSeq(1),
            kind: EventKind::ToolCall,
            detail: "tool=read_file".into(),
        };
        let json = serde_json::to_string(&event).expect("serialization should succeed");
        let v: serde_json::Value =
            serde_json::from_str(&json).expect("parsing to Value should succeed");
        assert_eq!(v["kind"], "ToolCall");
        let restored: AppEvent =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(event, restored);
    }

    #[test]
    fn tool_policy_event_kind_name_and_json_round_trip() {
        assert_eq!(EventKind::ToolPolicy.name(), "ToolPolicy");

        let event = AppEvent {
            seq: EventSeq(1),
            kind: EventKind::ToolPolicy,
            detail: "policy=allow tool=read_file".into(),
        };
        let json = serde_json::to_string(&event).expect("serialization should succeed");
        let v: serde_json::Value =
            serde_json::from_str(&json).expect("parsing to Value should succeed");
        assert_eq!(v["kind"], "ToolPolicy");
        let restored: AppEvent =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(event, restored);
    }

    #[test]
    fn tool_result_event_kind_name_and_json_round_trip() {
        assert_eq!(EventKind::ToolResult.name(), "ToolResult");

        let event = AppEvent {
            seq: EventSeq(1),
            kind: EventKind::ToolResult,
            detail: "tool=read_file status=ok".into(),
        };
        let json = serde_json::to_string(&event).expect("serialization should succeed");
        let v: serde_json::Value =
            serde_json::from_str(&json).expect("parsing to Value should succeed");
        assert_eq!(v["kind"], "ToolResult");
        let restored: AppEvent =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(event, restored);
    }

    #[test]
    fn tool_error_event_kind_name_and_json_round_trip() {
        assert_eq!(EventKind::ToolError.name(), "ToolError");

        let event = AppEvent {
            seq: EventSeq(1),
            kind: EventKind::ToolError,
            detail: "tool=read_file error=permission denied".into(),
        };
        let json = serde_json::to_string(&event).expect("serialization should succeed");
        let v: serde_json::Value =
            serde_json::from_str(&json).expect("parsing to Value should succeed");
        assert_eq!(v["kind"], "ToolError");
        let restored: AppEvent =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(event, restored);
    }

    #[test]
    fn tool_context_attach_event_kind_name_and_json_round_trip() {
        assert_eq!(EventKind::ToolContextAttach.name(), "ToolContextAttach");

        let event = AppEvent {
            seq: EventSeq(1),
            kind: EventKind::ToolContextAttach,
            detail: "tool_use_id=abc123".into(),
        };
        let json = serde_json::to_string(&event).expect("serialization should succeed");
        let v: serde_json::Value =
            serde_json::from_str(&json).expect("parsing to Value should succeed");
        assert_eq!(v["kind"], "ToolContextAttach");
        let restored: AppEvent =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(event, restored);
    }

    #[test]
    fn tool_context_clear_event_kind_name_and_json_round_trip() {
        assert_eq!(EventKind::ToolContextClear.name(), "ToolContextClear");

        let event = AppEvent {
            seq: EventSeq(1),
            kind: EventKind::ToolContextClear,
            detail: String::new(),
        };
        let json = serde_json::to_string(&event).expect("serialization should succeed");
        let v: serde_json::Value =
            serde_json::from_str(&json).expect("parsing to Value should succeed");
        assert_eq!(v["kind"], "ToolContextClear");
        let restored: AppEvent =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(event, restored);
    }

    #[test]
    fn assistant_message_event_kind_name_and_json_round_trip() {
        assert_eq!(EventKind::AssistantMessage.name(), "AssistantMessage");

        let event = AppEvent {
            seq: EventSeq(1),
            kind: EventKind::AssistantMessage,
            detail: "The assistant replied.".into(),
        };
        let json = serde_json::to_string(&event).expect("serialization should succeed");
        let v: serde_json::Value =
            serde_json::from_str(&json).expect("parsing to Value should succeed");
        assert_eq!(v["kind"], "AssistantMessage");
        let restored: AppEvent =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(event, restored);
    }

    #[test]
    fn next_seq_value_returns_next_assigned_seq() {
        let mut log = EventLog::new();
        assert_eq!(log.next_seq_value(), 1);
        log.append(EventKind::AppStart, "started");
        assert_eq!(log.next_seq_value(), 2);
    }

    #[test]
    fn run_id_turn_id_display() {
        let run_id = RunId("run-12".to_string());
        assert_eq!(run_id.to_string(), "run-12");

        let turn_id = TurnId("turn-7".to_string());
        assert_eq!(turn_id.to_string(), "turn-7");
    }
}
