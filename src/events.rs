use std::fmt;

use serde::{Deserialize, Serialize};

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
}

impl EventLog {
    /// Creates a new, empty event log. The first appended event will have seq = 1.
    pub fn new() -> Self {
        EventLog {
            events: Vec::new(),
            next_seq: 1,
        }
    }

    /// Appends a new event, assigns the current sequence number, increments the counter,
    /// and returns the assigned sequence number.
    pub fn append(&mut self, kind: EventKind, detail: impl Into<String>) -> EventSeq {
        let seq = EventSeq(self.next_seq);
        self.events.push(AppEvent {
            seq,
            kind,
            detail: detail.into(),
        });
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
    use super::*;

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
