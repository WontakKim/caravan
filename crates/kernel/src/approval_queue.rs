use std::collections::{BTreeMap, BTreeSet};

use crate::approval::{
    ApprovalDecision, ApprovalDecisionRecord, ApprovalResumePlan, ApprovalResumeRecord,
    ParsedApprovalRequest,
};
use crate::events::{AppEvent, EventKind, EventLog, EventSeq};

/// A single pending approval derived from an `ApprovalRequest` event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingApproval {
    pub seq: EventSeq,
    pub detail: String,
}

/// A resolved approval whose request was decided via an `ApprovalDecision` event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedApproval {
    pub request_seq: EventSeq,
    pub decision_seq: EventSeq,
    pub request_detail: String,
    pub decision: ApprovalDecision,
    pub reason: String,
}

/// A read-only projection of all pending approvals collected from an event log.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ApprovalQueue {
    pub pending: Vec<PendingApproval>,
    pub resolved: Vec<ResolvedApproval>,
    /// Request seqs that have been consumed by a valid `ApprovalResume` event.
    pub resumed: BTreeSet<EventSeq>,
}

impl ApprovalQueue {
    /// Collects `ApprovalRequest` events and resolves them against `ApprovalDecision`
    /// events, partitioning into `pending` (undecided) and `resolved` (decided) lists.
    pub fn from_events(events: &[AppEvent]) -> Self {
        // Collect all requests in order, preserving original sequence.
        let requests: Vec<(EventSeq, String)> = events
            .iter()
            .filter(|e| e.kind == EventKind::ApprovalRequest)
            .map(|e| (e.seq, e.detail.clone()))
            .collect();

        // Build a set of known request seqs for fast lookup.
        let request_seqs: BTreeSet<EventSeq> = requests.iter().map(|(seq, _)| *seq).collect();

        // For each request_seq, track the best (greatest decision_seq) valid decision.
        let mut best_decisions: BTreeMap<EventSeq, (EventSeq, ApprovalDecision, String)> =
            BTreeMap::new();

        for event in events {
            if event.kind != EventKind::ApprovalDecision {
                continue;
            }
            // decision_seq is the event's own seq — NOT a value from the detail.
            let decision_seq = event.seq;

            let Some(record) = ApprovalDecisionRecord::parse_detail(&event.detail) else {
                continue;
            };
            let request_seq = record.request_seq;

            // Ignore decisions referencing an unknown request.
            if !request_seqs.contains(&request_seq) {
                continue;
            }
            // A valid decision must come strictly after the request.
            if decision_seq <= request_seq {
                continue;
            }
            // Keep only the decision with the greatest decision_seq.
            let is_better = match best_decisions.get(&request_seq) {
                Some((existing_seq, _, _)) => decision_seq > *existing_seq,
                None => true,
            };
            if is_better {
                best_decisions.insert(request_seq, (decision_seq, record.decision, record.reason));
            }
        }

        // Partition requests into pending (no decision) and resolved (has a decision).
        let mut pending = Vec::new();
        let mut resolved = Vec::new();

        for (request_seq, request_detail) in requests {
            if let Some((decision_seq, decision, reason)) = best_decisions.remove(&request_seq) {
                resolved.push(ResolvedApproval {
                    request_seq,
                    decision_seq,
                    request_detail,
                    decision,
                    reason,
                });
            } else {
                pending.push(PendingApproval {
                    seq: request_seq,
                    detail: request_detail,
                });
            }
        }

        // Build a map from request_seq → decision_seq for approved resolved requests.
        let approved_decision_seqs: BTreeMap<EventSeq, EventSeq> = resolved
            .iter()
            .filter(|r| r.decision == ApprovalDecision::Approved)
            .map(|r| (r.request_seq, r.decision_seq))
            .collect();

        // Scan `ApprovalResume` events and mark valid ones as consumed.
        let mut resumed: BTreeSet<EventSeq> = BTreeSet::new();
        for event in events {
            if event.kind != EventKind::ApprovalResume {
                continue;
            }
            let Some(record) = ApprovalResumeRecord::parse_detail(&event.detail) else {
                continue;
            };
            // The resume must reference a known approved resolved request.
            let Some(&plan_decision_seq) = approved_decision_seqs.get(&record.request_seq) else {
                continue;
            };
            // The parsed decision_seq must match the plan's actual decision_seq.
            if record.decision_seq != plan_decision_seq {
                continue;
            }
            // The resume event must come strictly after the decision.
            if event.seq <= record.decision_seq {
                continue;
            }
            resumed.insert(record.request_seq);
        }

        ApprovalQueue {
            pending,
            resolved,
            resumed,
        }
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

    /// Projects the resolved approvals into a list of resume plans for approved,
    /// supported-tool requests.
    ///
    /// Iterates `self.resolved`, keeps only entries whose `decision` is
    /// [`ApprovalDecision::Approved`], parses `request_detail` via
    /// [`ParsedApprovalRequest::parse_detail`], and drops entries that fail to
    /// parse or whose [`ParsedApprovalRequest::to_tool_request`] returns `None`
    /// (unsupported tool). Each surviving entry yields one [`ApprovalResumePlan`]
    /// carrying `request_seq`, `decision_seq`, `request_detail` (cloned verbatim),
    /// and the parsed request.
    ///
    /// This method reads `self.resolved` only and does not mutate the event log.
    pub fn resume_plans(&self) -> Vec<ApprovalResumePlan> {
        self.resolved
            .iter()
            .filter(|r| r.decision == ApprovalDecision::Approved)
            .filter(|r| !self.resumed.contains(&r.request_seq))
            .filter_map(|r| {
                let request = ParsedApprovalRequest::parse_detail(&r.request_detail)?;
                request.to_tool_request()?;
                Some(ApprovalResumePlan {
                    request_seq: r.request_seq,
                    decision_seq: r.decision_seq,
                    request_detail: r.request_detail.clone(),
                    request,
                })
            })
            .collect()
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
    use crate::approval::{ApprovalDecision, ApprovalDecisionRecord, ApprovalResumeRecord};
    use crate::events::{AppEvent, EventKind, EventLog, EventSeq};

    // --- helpers ---

    fn make_request_event(seq: u64, detail: &str) -> AppEvent {
        AppEvent {
            seq: EventSeq(seq),
            kind: EventKind::ApprovalRequest,
            detail: detail.to_string(),
        }
    }

    fn make_decision_event(
        seq: u64,
        request_seq: u64,
        decision: ApprovalDecision,
        reason: &str,
    ) -> AppEvent {
        let record = ApprovalDecisionRecord {
            request_seq: EventSeq(request_seq),
            decision,
            reason: reason.to_string(),
        };
        AppEvent {
            seq: EventSeq(seq),
            kind: EventKind::ApprovalDecision,
            detail: record.detail(),
        }
    }

    // --- existing tests (must remain unchanged) ---

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

    // --- new resolution tests ---

    #[test]
    fn one_request_no_decision_yields_one_pending_zero_resolved() {
        let events = [make_request_event(
            1,
            "tool=bash path=\"/tmp\" risk=high reason=test",
        )];
        let queue = ApprovalQueue::from_events(&events);
        assert_eq!(queue.pending.len(), 1);
        assert_eq!(queue.resolved.len(), 0);
    }

    #[test]
    fn approved_decision_moves_request_to_resolved() {
        let events = [
            make_request_event(1, "tool=bash path=\"/tmp\" risk=high reason=test"),
            make_decision_event(2, 1, ApprovalDecision::Approved, "looks good"),
        ];
        let queue = ApprovalQueue::from_events(&events);
        assert_eq!(queue.pending.len(), 0);
        assert_eq!(queue.resolved.len(), 1);
        let r = &queue.resolved[0];
        assert_eq!(r.request_seq, EventSeq(1));
        assert_eq!(r.decision_seq, EventSeq(2));
        assert_eq!(r.decision, ApprovalDecision::Approved);
        assert_eq!(r.reason, "looks good");
    }

    #[test]
    fn rejected_decision_moves_request_to_resolved() {
        let events = [
            make_request_event(1, "tool=rm path=\"/etc\" risk=critical reason=danger"),
            make_decision_event(2, 1, ApprovalDecision::Rejected, "too risky"),
        ];
        let queue = ApprovalQueue::from_events(&events);
        assert_eq!(queue.pending.len(), 0);
        assert_eq!(queue.resolved.len(), 1);
        let r = &queue.resolved[0];
        assert_eq!(r.decision, ApprovalDecision::Rejected);
        assert_eq!(r.reason, "too risky");
    }

    #[test]
    fn decision_with_missing_request_seq_does_not_affect_resolved() {
        // Decision references seq=99 which has no matching ApprovalRequest.
        let events = [
            make_request_event(1, "tool=bash path=\"/tmp\" risk=high reason=test"),
            make_decision_event(2, 99, ApprovalDecision::Approved, "ok"),
        ];
        let queue = ApprovalQueue::from_events(&events);
        assert_eq!(queue.pending.len(), 1);
        assert_eq!(queue.resolved.len(), 0);
    }

    #[test]
    fn decision_with_seq_not_exceeding_request_seq_is_not_resolved() {
        // decision_seq (5) <= request_seq (10) → decision must be ignored.
        let events = [
            make_request_event(10, "tool=bash path=\"/tmp\" risk=high reason=test"),
            make_decision_event(5, 10, ApprovalDecision::Approved, "ok"),
        ];
        let queue = ApprovalQueue::from_events(&events);
        assert_eq!(queue.pending.len(), 1);
        assert_eq!(queue.resolved.len(), 0);
    }

    #[test]
    fn malformed_decision_detail_leaves_request_pending_not_resolved() {
        let malformed = AppEvent {
            seq: EventSeq(2),
            kind: EventKind::ApprovalDecision,
            detail: "this is not valid detail".to_string(),
        };
        let events = [
            make_request_event(1, "tool=bash path=\"/tmp\" risk=high reason=test"),
            malformed,
        ];
        let queue = ApprovalQueue::from_events(&events);
        assert_eq!(queue.pending.len(), 1);
        assert_eq!(queue.resolved.len(), 0);
    }

    #[test]
    fn last_valid_decision_seq_wins_when_multiple_decisions_for_same_request_resolved() {
        // Two decisions for request_seq=1: first Approved (seq=2), then Rejected (seq=3).
        // The one with the greatest decision_seq (3, Rejected) should win.
        let events = [
            make_request_event(1, "tool=bash path=\"/tmp\" risk=high reason=test"),
            make_decision_event(2, 1, ApprovalDecision::Approved, "first decision"),
            make_decision_event(3, 1, ApprovalDecision::Rejected, "second decision"),
        ];
        let queue = ApprovalQueue::from_events(&events);
        assert_eq!(queue.pending.len(), 0);
        assert_eq!(queue.resolved.len(), 1);
        let r = &queue.resolved[0];
        assert_eq!(r.decision_seq, EventSeq(3));
        assert_eq!(r.decision, ApprovalDecision::Rejected);
        assert_eq!(r.reason, "second decision");
    }

    // --- resume_plans tests ---

    const SUPPORTED_DETAIL: &str = r#"tool=read_file path="README.md" risk=read_only reason=test"#;
    const UNSUPPORTED_DETAIL: &str =
        r#"tool=write_file path="output.txt" risk=high reason=dangerous"#;

    #[test]
    fn resume_plans_approved_supported_request_yields_one_plan_with_matching_detail() {
        let events = [
            make_request_event(1, SUPPORTED_DETAIL),
            make_decision_event(2, 1, ApprovalDecision::Approved, "ok"),
        ];
        let queue = ApprovalQueue::from_events(&events);
        let plans = queue.resume_plans();
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].request_detail, SUPPORTED_DETAIL);
        assert_eq!(plans[0].request_seq, EventSeq(1));
        assert_eq!(plans[0].decision_seq, EventSeq(2));
    }

    #[test]
    fn resume_plans_rejected_request_yields_none() {
        let events = [
            make_request_event(1, SUPPORTED_DETAIL),
            make_decision_event(2, 1, ApprovalDecision::Rejected, "too risky"),
        ];
        let queue = ApprovalQueue::from_events(&events);
        assert_eq!(queue.resume_plans().len(), 0);
    }

    #[test]
    fn resume_plans_latest_rejected_after_earlier_approve_yields_none() {
        // First decision: Approved (seq=2), then overridden by Rejected (seq=3).
        let events = [
            make_request_event(1, SUPPORTED_DETAIL),
            make_decision_event(2, 1, ApprovalDecision::Approved, "first ok"),
            make_decision_event(3, 1, ApprovalDecision::Rejected, "changed mind"),
        ];
        let queue = ApprovalQueue::from_events(&events);
        assert_eq!(queue.resume_plans().len(), 0);
    }

    #[test]
    fn resume_plans_latest_approved_after_earlier_reject_yields_one_plan() {
        // First decision: Rejected (seq=2), then overridden by Approved (seq=3).
        let events = [
            make_request_event(1, SUPPORTED_DETAIL),
            make_decision_event(2, 1, ApprovalDecision::Rejected, "initially refused"),
            make_decision_event(3, 1, ApprovalDecision::Approved, "reconsidered"),
        ];
        let queue = ApprovalQueue::from_events(&events);
        let plans = queue.resume_plans();
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].request_detail, SUPPORTED_DETAIL);
    }

    #[test]
    fn resume_plans_malformed_request_detail_yields_none() {
        // Inject a ResolvedApproval with Approved decision but a detail that
        // ParsedApprovalRequest::parse_detail cannot parse.
        let mut queue = ApprovalQueue::default();
        queue.resolved.push(ResolvedApproval {
            request_seq: EventSeq(1),
            decision_seq: EventSeq(2),
            request_detail: "this is not valid request detail".to_string(),
            decision: ApprovalDecision::Approved,
            reason: "ok".to_string(),
        });
        assert_eq!(queue.resume_plans().len(), 0);
    }

    #[test]
    fn resume_plans_unsupported_tool_yields_none() {
        let events = [
            make_request_event(1, UNSUPPORTED_DETAIL),
            make_decision_event(2, 1, ApprovalDecision::Approved, "approved anyway"),
        ];
        let queue = ApprovalQueue::from_events(&events);
        assert_eq!(queue.resume_plans().len(), 0);
    }

    #[test]
    fn resume_plans_decision_seq_not_greater_than_request_seq_yields_none() {
        // decision_seq (1) <= request_seq (5): this entry is excluded from resolved
        // upstream by from_events, so resume_plans sees an empty resolved list.
        let events = [
            make_request_event(5, SUPPORTED_DETAIL),
            make_decision_event(1, 5, ApprovalDecision::Approved, "too early"),
        ];
        let queue = ApprovalQueue::from_events(&events);
        assert_eq!(queue.resolved.len(), 0);
        assert_eq!(queue.resume_plans().len(), 0);
    }

    // --- ApprovalResume consumption tests ---

    /// Appends an `ApprovalResume` event built from `record` at the given seq.
    fn make_resume_event(seq: u64, record: &ApprovalResumeRecord) -> AppEvent {
        AppEvent {
            seq: EventSeq(seq),
            kind: EventKind::ApprovalResume,
            detail: record.detail(),
        }
    }

    #[test]
    fn resume_plans_excludes_plan_consumed_by_valid_approval_resume() {
        // request at seq=1, approved at seq=2, resumed at seq=3 → plan is consumed.
        let record = ApprovalResumeRecord {
            request_seq: EventSeq(1),
            decision_seq: EventSeq(2),
            tool: "read_file".to_string(),
            path: "README.md".to_string(),
            risk: "read_only".to_string(),
            reason: "test".to_string(),
        };
        let events = [
            make_request_event(1, SUPPORTED_DETAIL),
            make_decision_event(2, 1, ApprovalDecision::Approved, "ok"),
            make_resume_event(3, &record),
        ];
        let queue = ApprovalQueue::from_events(&events);
        assert!(queue.resumed.contains(&EventSeq(1)));
        assert_eq!(queue.resume_plans().len(), 0);
    }

    #[test]
    fn resume_plans_keeps_not_yet_resumed_approved_plan() {
        // Approved plan with no corresponding ApprovalResume → still in resume_plans.
        let events = [
            make_request_event(1, SUPPORTED_DETAIL),
            make_decision_event(2, 1, ApprovalDecision::Approved, "ok"),
        ];
        let queue = ApprovalQueue::from_events(&events);
        assert!(queue.resumed.is_empty());
        assert_eq!(queue.resume_plans().len(), 1);
    }

    #[test]
    fn resume_plans_ignores_malformed_approval_resume_detail() {
        // Malformed detail → ApprovalResumeRecord::parse_detail returns None → ignored.
        let malformed_resume = AppEvent {
            seq: EventSeq(3),
            kind: EventKind::ApprovalResume,
            detail: "this is not valid resume detail".to_string(),
        };
        let events = [
            make_request_event(1, SUPPORTED_DETAIL),
            make_decision_event(2, 1, ApprovalDecision::Approved, "ok"),
            malformed_resume,
        ];
        let queue = ApprovalQueue::from_events(&events);
        assert!(queue.resumed.is_empty());
        assert_eq!(queue.resume_plans().len(), 1);
    }

    #[test]
    fn resume_plans_ignores_approval_resume_with_unknown_request_seq() {
        // ApprovalResume references request_seq=99, which has no known ApprovalRequest.
        let record = ApprovalResumeRecord {
            request_seq: EventSeq(99),
            decision_seq: EventSeq(2),
            tool: "read_file".to_string(),
            path: "README.md".to_string(),
            risk: "read_only".to_string(),
            reason: "test".to_string(),
        };
        let events = [
            make_request_event(1, SUPPORTED_DETAIL),
            make_decision_event(2, 1, ApprovalDecision::Approved, "ok"),
            make_resume_event(3, &record),
        ];
        let queue = ApprovalQueue::from_events(&events);
        assert!(queue.resumed.is_empty());
        assert_eq!(queue.resume_plans().len(), 1);
    }

    #[test]
    fn resume_plans_ignores_approval_resume_whose_event_seq_not_exceeding_decision_seq() {
        // event.seq (2) == decision_seq (2) → not strictly greater → ignored.
        let record = ApprovalResumeRecord {
            request_seq: EventSeq(1),
            decision_seq: EventSeq(2),
            tool: "read_file".to_string(),
            path: "README.md".to_string(),
            risk: "read_only".to_string(),
            reason: "test".to_string(),
        };
        // Construct events manually so the resume event seq == decision_seq.
        let events = [
            make_request_event(1, SUPPORTED_DETAIL),
            make_decision_event(2, 1, ApprovalDecision::Approved, "ok"),
            make_resume_event(2, &record),
        ];
        let queue = ApprovalQueue::from_events(&events);
        assert!(queue.resumed.is_empty());
        assert_eq!(queue.resume_plans().len(), 1);
    }

    /// decision_seq mismatch: plan was approved at decision_seq=20 but the resume
    /// claims decision_seq=1 → the plan must NOT be consumed.
    #[test]
    fn resume_plans_ignores_approval_resume_with_decision_seq_mismatch() {
        // Request at seq=1, approved at seq=20; the ApprovalResume carries decision_seq=1
        // (stale/bogus) which does not match the plan's decision_seq=20 → ignored.
        let record = ApprovalResumeRecord {
            request_seq: EventSeq(1),
            decision_seq: EventSeq(1), // mismatch: actual decision_seq is 20
            tool: "read_file".to_string(),
            path: "README.md".to_string(),
            risk: "read_only".to_string(),
            reason: "test".to_string(),
        };
        let events = [
            make_request_event(1, SUPPORTED_DETAIL),
            make_decision_event(20, 1, ApprovalDecision::Approved, "ok"),
            make_resume_event(30, &record),
        ];
        let queue = ApprovalQueue::from_events(&events);
        // The plan should NOT be consumed due to the decision_seq mismatch.
        assert!(queue.resumed.is_empty());
        assert_eq!(queue.resume_plans().len(), 1);
    }

    #[test]
    fn resume_plans_stays_consumed_when_multiple_resume_events_for_same_request_seq() {
        // Two ApprovalResume events for the same request_seq=1 → still consumed once.
        let record = ApprovalResumeRecord {
            request_seq: EventSeq(1),
            decision_seq: EventSeq(2),
            tool: "read_file".to_string(),
            path: "README.md".to_string(),
            risk: "read_only".to_string(),
            reason: "test".to_string(),
        };
        let events = [
            make_request_event(1, SUPPORTED_DETAIL),
            make_decision_event(2, 1, ApprovalDecision::Approved, "ok"),
            make_resume_event(3, &record),
            make_resume_event(4, &record),
        ];
        let queue = ApprovalQueue::from_events(&events);
        assert!(queue.resumed.contains(&EventSeq(1)));
        assert_eq!(queue.resume_plans().len(), 0);
    }
}
