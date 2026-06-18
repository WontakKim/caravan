use super::ids::EventSeq;
use super::kind::EventKind;
use super::record::AppEvent;
use crate::storage::EventStore;

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
