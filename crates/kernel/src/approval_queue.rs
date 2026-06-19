use crate::events::{AppEvent, EventKind, EventLog, EventSeq};

/// A single pending approval derived from an `ApprovalRequest` event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingApproval {
    pub seq: EventSeq,
    pub detail: String,
}

/// A read-only projection of all pending approvals collected from an event log.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ApprovalQueue {
    pub pending: Vec<PendingApproval>,
}

impl ApprovalQueue {
    /// Collects all `ApprovalRequest` events from a slice, in order, into a queue.
    pub fn from_events(events: &[AppEvent]) -> Self {
        let pending = events
            .iter()
            .filter(|e| e.kind == EventKind::ApprovalRequest)
            .map(|e| PendingApproval {
                seq: e.seq,
                detail: e.detail.clone(),
            })
            .collect();
        ApprovalQueue { pending }
    }

    /// Builds a queue from an `EventLog` by delegating to `from_events`.
    pub fn from_event_log(event_log: &EventLog) -> Self {
        Self::from_events(event_log.events())
    }

    /// Returns `true` if there are no pending approvals.
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Returns the number of pending approvals.
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    /// Returns a human-readable status summary.
    ///
    /// - First line: `"Approval status:"`
    /// - When empty: second line is `"- pending: none"`
    /// - When non-empty: second line is `"- pending: <count>"` followed by one
    ///   line per item formatted as `"- seq=<seq> <detail>"`
    pub fn render_status_lines(&self) -> Vec<String> {
        let mut lines = vec!["Approval status:".to_string()];
        if self.is_empty() {
            lines.push("- pending: none".to_string());
        } else {
            lines.push(format!("- pending: {}", self.len()));
            for item in &self.pending {
                lines.push(format!("- seq={} {}", item.seq, item.detail));
            }
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{EventKind, EventLog};

    #[test]
    fn empty_input_yields_empty_queue() {
        let queue = ApprovalQueue::from_events(&[]);
        assert!(queue.is_empty());
        assert_eq!(queue.len(), 0);
    }

    #[test]
    fn non_approval_request_events_are_ignored() {
        let mut log = EventLog::new();
        log.append(EventKind::AppStart, "started");
        log.append(EventKind::UserMessage, "hello");
        log.append(EventKind::ToolCall, "tool=foo");

        let queue = ApprovalQueue::from_event_log(&log);
        assert!(queue.is_empty());
    }

    #[test]
    fn approval_request_events_are_collected_with_correct_seq_and_detail() {
        let mut log = EventLog::new();
        log.append(EventKind::AppStart, "started");
        let seq = log.append(
            EventKind::ApprovalRequest,
            "tool=bash path=\"/tmp\" risk=high reason=dangerous",
        );

        let queue = ApprovalQueue::from_event_log(&log);
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.pending[0].seq, seq);
        assert_eq!(
            queue.pending[0].detail,
            "tool=bash path=\"/tmp\" risk=high reason=dangerous"
        );
    }

    #[test]
    fn original_event_order_is_preserved() {
        let mut log = EventLog::new();
        let seq1 = log.append(
            EventKind::ApprovalRequest,
            "tool=a path=\"/a\" risk=low reason=r1",
        );
        log.append(EventKind::ToolCall, "unrelated");
        let seq2 = log.append(
            EventKind::ApprovalRequest,
            "tool=b path=\"/b\" risk=high reason=r2",
        );

        let queue = ApprovalQueue::from_event_log(&log);
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.pending[0].seq, seq1);
        assert_eq!(queue.pending[1].seq, seq2);
    }

    #[test]
    fn from_event_log_does_not_change_log_len() {
        let mut log = EventLog::new();
        log.append(
            EventKind::ApprovalRequest,
            "tool=x path=\"/x\" risk=low reason=test",
        );
        let len_before = log.len();

        let _queue = ApprovalQueue::from_event_log(&log);

        assert_eq!(log.len(), len_before);
    }

    #[test]
    fn render_status_lines_returns_pending_none_when_empty() {
        let queue = ApprovalQueue::default();
        let lines = queue.render_status_lines();
        assert_eq!(lines, vec!["Approval status:", "- pending: none"]);
    }

    #[test]
    fn render_status_lines_returns_seq_detail_lines_when_non_empty() {
        let mut log = EventLog::new();
        let seq1 = log.append(
            EventKind::ApprovalRequest,
            "tool=bash path=\"/tmp\" risk=high reason=test",
        );
        let seq2 = log.append(
            EventKind::ApprovalRequest,
            "tool=rm path=\"/etc\" risk=critical reason=danger",
        );

        let queue = ApprovalQueue::from_event_log(&log);
        let lines = queue.render_status_lines();

        assert_eq!(lines[0], "Approval status:");
        assert_eq!(lines[1], format!("- pending: 2"));
        assert_eq!(
            lines[2],
            format!(
                "- seq={} tool=bash path=\"/tmp\" risk=high reason=test",
                seq1
            )
        );
        assert_eq!(
            lines[3],
            format!(
                "- seq={} tool=rm path=\"/etc\" risk=critical reason=danger",
                seq2
            )
        );
    }
}
