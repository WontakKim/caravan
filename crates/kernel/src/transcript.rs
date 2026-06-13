use crate::events::{AppEvent, EventKind, EventLog, EventSeq};

/// The role of a participant in a conversation turn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TranscriptRole {
    User,
    Assistant,
}

/// A single message in a conversation transcript.
#[derive(PartialEq, Debug)]
pub struct TranscriptMessage {
    pub role: TranscriptRole,
    pub content: String,
    pub seq: EventSeq,
}

/// A read-only projection of the conversation between the user and the assistant,
/// reconstructed from the event log.
#[derive(PartialEq, Debug)]
pub struct ConversationTranscript {
    pub messages: Vec<TranscriptMessage>,
}

impl ConversationTranscript {
    /// Builds a transcript from a slice of events, preserving input order.
    /// Only `UserMessage` and `AssistantMessage` events are included; all others are skipped.
    pub fn from_events(events: &[AppEvent]) -> Self {
        let messages = events
            .iter()
            .filter_map(|event| match event.kind {
                EventKind::UserMessage => Some(TranscriptMessage {
                    role: TranscriptRole::User,
                    content: event.detail.clone(),
                    seq: event.seq,
                }),
                EventKind::AssistantMessage => Some(TranscriptMessage {
                    role: TranscriptRole::Assistant,
                    content: event.detail.clone(),
                    seq: event.seq,
                }),
                _ => None,
            })
            .collect();

        ConversationTranscript { messages }
    }

    /// Builds a transcript from an event log by delegating to `from_events`.
    /// Takes an immutable borrow and performs no mutation, append, or persistence.
    pub fn from_event_log(event_log: &EventLog) -> Self {
        Self::from_events(event_log.events())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{EventKind, EventLog, EventSeq};

    fn make_event(seq: u64, kind: EventKind, detail: &str) -> AppEvent {
        AppEvent {
            seq: EventSeq(seq),
            kind,
            detail: detail.to_string(),
        }
    }

    #[test]
    fn only_user_and_assistant_messages_are_included() {
        let events = vec![
            make_event(1, EventKind::UserMessage, "hello"),
            make_event(2, EventKind::AssistantMessage, "hi there"),
        ];
        let transcript = ConversationTranscript::from_events(&events);
        assert_eq!(transcript.messages.len(), 2);
        assert_eq!(transcript.messages[0].role, TranscriptRole::User);
        assert_eq!(transcript.messages[1].role, TranscriptRole::Assistant);
    }

    #[test]
    fn ignored_kinds_are_skipped() {
        let events = vec![
            make_event(1, EventKind::ModelOutputChunk, "chunk"),
            make_event(2, EventKind::PromptCompile, "prompt"),
            make_event(3, EventKind::ModelRoute, "route"),
            make_event(4, EventKind::ModelUsage, "usage"),
            make_event(5, EventKind::ModelError, "err"),
            make_event(6, EventKind::RunFail, "fail"),
            make_event(7, EventKind::UserMessage, "real"),
        ];
        let transcript = ConversationTranscript::from_events(&events);
        assert_eq!(transcript.messages.len(), 1);
        assert_eq!(transcript.messages[0].role, TranscriptRole::User);
        assert_eq!(transcript.messages[0].content, "real");
    }

    #[test]
    fn seq_order_is_preserved() {
        let events = vec![
            make_event(1, EventKind::UserMessage, "first"),
            make_event(2, EventKind::AssistantMessage, "second"),
            make_event(3, EventKind::UserMessage, "third"),
        ];
        let transcript = ConversationTranscript::from_events(&events);
        assert_eq!(transcript.messages[0].seq, EventSeq(1));
        assert_eq!(transcript.messages[1].seq, EventSeq(2));
        assert_eq!(transcript.messages[2].seq, EventSeq(3));
    }

    #[test]
    fn content_equals_detail_verbatim() {
        let detail = "exact detail string";
        let events = vec![make_event(1, EventKind::UserMessage, detail)];
        let transcript = ConversationTranscript::from_events(&events);
        assert_eq!(transcript.messages[0].content, detail);
    }

    #[test]
    fn from_event_log_does_not_change_log_len() {
        let mut log = EventLog::new();
        log.append(EventKind::UserMessage, "hello");
        log.append(EventKind::AssistantMessage, "world");
        let len_before = log.len();
        let _ = ConversationTranscript::from_event_log(&log);
        assert_eq!(log.len(), len_before);
    }

    #[test]
    fn from_event_log_equals_from_events() {
        let mut log = EventLog::new();
        log.append(EventKind::UserMessage, "hello");
        log.append(EventKind::ModelOutputChunk, "chunk");
        log.append(EventKind::AssistantMessage, "reply");
        let from_log = ConversationTranscript::from_event_log(&log);
        let from_events = ConversationTranscript::from_events(log.events());
        assert_eq!(from_log, from_events);
    }

    #[test]
    fn transcript_role_equality() {
        assert_eq!(TranscriptRole::User, TranscriptRole::User);
        assert_eq!(TranscriptRole::Assistant, TranscriptRole::Assistant);
        assert_ne!(TranscriptRole::User, TranscriptRole::Assistant);
    }

    #[test]
    fn transcript_message_equality() {
        let msg1 = TranscriptMessage {
            role: TranscriptRole::User,
            content: "hello".to_string(),
            seq: EventSeq(1),
        };
        let msg2 = TranscriptMessage {
            role: TranscriptRole::User,
            content: "hello".to_string(),
            seq: EventSeq(1),
        };
        assert_eq!(msg1, msg2);
    }

    #[test]
    fn empty_events_produces_empty_transcript() {
        let transcript = ConversationTranscript::from_events(&[]);
        assert_eq!(transcript.messages.len(), 0);
    }
}
