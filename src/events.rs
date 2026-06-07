use std::fmt;

use serde::{Deserialize, Serialize};

use crate::storage::EventStore;

/// A monotonically increasing sequence number assigned to each event.
/// Numbering starts at 1 and increases by 1 per append.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EventSeq(pub u64);

impl fmt::Display for EventSeq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The kind of application event that occurred.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum EventKind {
    AppStarted,
    CommandEntered,
    HelpRequested,
    UserTextEntered,
    LogCleared,
    InspectorSelectionChanged,
    ExitRequested,
    UnknownCommand,
}

impl EventKind {
    /// Returns the name of this variant as a static string.
    pub fn name(&self) -> &'static str {
        match self {
            EventKind::AppStarted => "AppStarted",
            EventKind::CommandEntered => "CommandEntered",
            EventKind::HelpRequested => "HelpRequested",
            EventKind::UserTextEntered => "UserTextEntered",
            EventKind::LogCleared => "LogCleared",
            EventKind::InspectorSelectionChanged => "InspectorSelectionChanged",
            EventKind::ExitRequested => "ExitRequested",
            EventKind::UnknownCommand => "UnknownCommand",
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
        log1.append(EventKind::AppStarted, "first start");
        log1.append(EventKind::UserTextEntered, "hello");
        log1.append(EventKind::UserTextEntered, "world");
        let max_seq = log1.get(log1.len() - 1).unwrap().seq.0;
        drop(log1);

        // Second "run": reload from the same directory.
        let store2 = EventStore::new(dir.path());
        let mut log2 = EventLog::load_from(store2);

        // All prior events are present.
        assert_eq!(log2.len(), 3);
        assert_eq!(log2.get(0).unwrap().kind, EventKind::AppStarted);
        assert_eq!(log2.get(1).unwrap().kind, EventKind::UserTextEntered);
        assert_eq!(log2.get(2).unwrap().kind, EventKind::UserTextEntered);

        // Next appended event continues the sequence.
        let new_seq = log2.append(EventKind::CommandEntered, "cmd");
        assert_eq!(new_seq.0, max_seq + 1);
    }

    #[test]
    fn first_append_returns_seq_one() {
        let mut log = EventLog::new();
        let seq = log.append(EventKind::AppStarted, "started");
        assert_eq!(seq, EventSeq(1));
    }

    #[test]
    fn second_append_returns_seq_two() {
        let mut log = EventLog::new();
        log.append(EventKind::AppStarted, "started");
        let seq = log.append(EventKind::CommandEntered, "cmd");
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
        log.append(EventKind::UserTextEntered, "hello");
        assert!(!log.is_empty());
        assert_eq!(log.len(), 1);
        log.append(EventKind::ExitRequested, "");
        assert_eq!(log.len(), 2);
    }

    #[test]
    fn get_returns_correct_event() {
        let mut log = EventLog::new();
        log.append(EventKind::HelpRequested, "help detail");
        let event = log.get(0).expect("event at index 0 should exist");
        assert_eq!(event.kind, EventKind::HelpRequested);
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
        log.append(EventKind::LogCleared, "cleared by user");
        let event = log.get(0).unwrap();
        assert_eq!(event.kind, EventKind::LogCleared);
        assert_eq!(event.detail, "cleared by user");
    }

    #[test]
    fn events_slice_matches_appended_events() {
        let mut log = EventLog::new();
        log.append(EventKind::AppStarted, "a");
        log.append(EventKind::UnknownCommand, "b");
        let events = log.events();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, EventKind::AppStarted);
        assert_eq!(events[1].kind, EventKind::UnknownCommand);
    }

    #[test]
    fn app_event_serializes_to_expected_jsonl() {
        let event = AppEvent {
            seq: EventSeq(1),
            kind: EventKind::AppStarted,
            detail: "Caravan started.".into(),
        };
        let json = serde_json::to_string(&event).expect("serialization should succeed");
        assert_eq!(
            json,
            r#"{"seq":1,"kind":"AppStarted","detail":"Caravan started."}"#
        );
    }

    #[test]
    fn app_event_json_round_trip() {
        let original = AppEvent {
            seq: EventSeq(1),
            kind: EventKind::AppStarted,
            detail: "Caravan started.".into(),
        };
        let json = serde_json::to_string(&original).expect("serialization should succeed");
        let restored: AppEvent =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(original, restored);

        let v: serde_json::Value =
            serde_json::from_str(&json).expect("parsing to Value should succeed");
        assert_eq!(v["seq"], 1);
        assert_eq!(v["kind"], "AppStarted");
        assert_eq!(v["detail"], "Caravan started.");
    }

    #[test]
    fn event_kind_json_round_trip() {
        for kind in [EventKind::ExitRequested, EventKind::UnknownCommand] {
            let json = serde_json::to_string(&kind).expect("serialization should succeed");
            let restored: EventKind =
                serde_json::from_str(&json).expect("deserialization should succeed");
            assert_eq!(kind, restored);
        }
    }
}
