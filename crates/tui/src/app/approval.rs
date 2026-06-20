use kernel::events::{EventKind, EventSeq};
use kernel::{ApprovalCommand, ApprovalDecision, ApprovalDecisionRecord, ApprovalQueue};

impl super::App {
    pub(super) fn handle_approval_command(&mut self, ac: ApprovalCommand) {
        match ac {
            ApprovalCommand::Status => {
                let queue = ApprovalQueue::from_event_log(&self.event_log);
                self.log.extend(queue.render_status_lines());
            }
            ApprovalCommand::Approve { seq } => {
                let is_pending = {
                    let queue = ApprovalQueue::from_event_log(&self.event_log);
                    queue.pending.iter().any(|p| p.seq == EventSeq(seq))
                };
                if is_pending {
                    let record = ApprovalDecisionRecord {
                        request_seq: EventSeq(seq),
                        decision: ApprovalDecision::Approved,
                        reason: "operator_approved".to_string(),
                    };
                    self.event_log
                        .append(EventKind::ApprovalDecision, &record.detail());
                    self.log
                        .push(format!("Approved approval request seq={seq}"));
                } else {
                    self.log.push(format!("No pending approval for seq={seq}"));
                }
            }
            ApprovalCommand::Reject { seq } => {
                let is_pending = {
                    let queue = ApprovalQueue::from_event_log(&self.event_log);
                    queue.pending.iter().any(|p| p.seq == EventSeq(seq))
                };
                if is_pending {
                    let record = ApprovalDecisionRecord {
                        request_seq: EventSeq(seq),
                        decision: ApprovalDecision::Rejected,
                        reason: "operator_rejected".to_string(),
                    };
                    self.event_log
                        .append(EventKind::ApprovalDecision, &record.detail());
                    self.log
                        .push(format!("Rejected approval request seq={seq}"));
                } else {
                    self.log.push(format!("No pending approval for seq={seq}"));
                }
            }
        }
    }
}
