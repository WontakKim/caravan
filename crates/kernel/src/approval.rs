//! Approval gate types for tool-execution approval flows.
//!
//! These types are pure data: this module imports neither the event log, the
//! policy engine, nor the runner. The wiring lives elsewhere — `ToolPolicyEngine`
//! sets `approval_requirement`, and `ToolEventRunner` evaluates [`ApprovalGate`]
//! after the `ToolPolicy` event on the allow path (yielding `None`, and so no
//! `ApprovalRequest`, for production read-only tools). What is deferred to a
//! future task is the approve/reject resolution flow, an approval UI, and any
//! approval-requiring (write/shell) tool.

use crate::events::EventSeq;

/// Whether a tool invocation requires human approval before it may proceed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalRequirement {
    /// No approval is required; the tool may run immediately.
    None,
    /// A human must approve before the tool runs.
    Manual { reason: String },
}

/// The outcome of a human approval decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalDecision {
    Approved,
    Rejected,
}

/// A recorded approval decision referencing an [`ApprovalRequest`] event.
///
/// The detail string format is key=value (NOT JSON):
/// `request_seq=<u64> decision=<approved|rejected> reason=<text>`
///
/// The detail MUST NOT include secrets, API keys, auth headers, or file content.
pub struct ApprovalDecisionRecord {
    pub request_seq: EventSeq,
    pub decision: ApprovalDecision,
    pub reason: String,
}

impl ApprovalDecisionRecord {
    /// Formats this record as an EventLog detail string.
    ///
    /// Output: `request_seq=<seq> decision=<approved|rejected> reason=<reason>`
    pub fn detail(&self) -> String {
        let decision_str = match self.decision {
            ApprovalDecision::Approved => "approved",
            ApprovalDecision::Rejected => "rejected",
        };
        format!(
            "request_seq={} decision={} reason={}",
            self.request_seq, decision_str, self.reason
        )
    }

    /// Parses an EventLog detail string into an [`ApprovalDecisionRecord`].
    ///
    /// Returns `None` for any malformed input; never panics.
    pub fn parse_detail(detail: &str) -> Option<Self> {
        let mut tokens = detail.splitn(3, ' ');

        let seq_token = tokens.next()?;
        let seq_val = seq_token.strip_prefix("request_seq=")?;
        let seq: u64 = seq_val.parse().ok()?;

        let decision_token = tokens.next()?;
        let decision_val = decision_token.strip_prefix("decision=")?;
        let decision = match decision_val {
            "approved" => ApprovalDecision::Approved,
            "rejected" => ApprovalDecision::Rejected,
            _ => return None,
        };

        let reason_token = tokens.next()?;
        let reason = reason_token.strip_prefix("reason=")?.to_string();
        if reason.is_empty() {
            return None;
        }

        Some(ApprovalDecisionRecord {
            request_seq: EventSeq(seq),
            decision,
            reason,
        })
    }
}

/// A pending approval request surfaced to the operator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalRequest {
    pub tool: String,
    pub path: String,
    pub risk: String,
    pub reason: String,
}

/// Evaluates whether a tool invocation needs approval.
pub struct ApprovalGate;

impl ApprovalGate {
    /// Creates a new `ApprovalGate`.
    pub fn new() -> Self {
        ApprovalGate
    }

    /// Returns `Some(ApprovalRequest)` when `requirement` demands manual
    /// approval, or `None` when the tool may proceed without it.
    pub fn evaluate(
        &self,
        tool: &str,
        path: &str,
        risk: &str,
        requirement: &ApprovalRequirement,
    ) -> Option<ApprovalRequest> {
        match requirement {
            ApprovalRequirement::None => None,
            ApprovalRequirement::Manual { reason } => Some(ApprovalRequest {
                tool: tool.to_string(),
                path: path.to_string(),
                risk: risk.to_string(),
                reason: reason.clone(),
            }),
        }
    }
}

impl Default for ApprovalGate {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluate_none_requirement_returns_none() {
        let gate = ApprovalGate::new();
        let result = gate.evaluate(
            "read_file",
            "/tmp/foo",
            "read_only",
            &ApprovalRequirement::None,
        );
        assert_eq!(result, None);
    }

    #[test]
    fn evaluate_manual_requirement_returns_some_with_fields_populated() {
        let gate = ApprovalGate::new();
        let requirement = ApprovalRequirement::Manual {
            reason: "sensitive path".to_string(),
        };
        let result = gate.evaluate("write_file", "/etc/hosts", "high", &requirement);
        let request = result.expect("expected Some(ApprovalRequest)");
        assert_eq!(request.tool, "write_file");
        assert_eq!(request.path, "/etc/hosts");
        assert_eq!(request.risk, "high");
        assert_eq!(request.reason, "sensitive path");
    }

    #[test]
    fn approval_gate_default_delegates_to_new() {
        let gate = ApprovalGate::default();
        let result = gate.evaluate("list_files", ".", "read_only", &ApprovalRequirement::None);
        assert_eq!(result, None);
    }

    #[test]
    fn approval_decision_variants_are_constructible() {
        let approved = ApprovalDecision::Approved;
        let rejected = ApprovalDecision::Rejected;
        assert_ne!(approved, rejected);
    }

    #[test]
    fn approval_requirement_manual_carries_reason() {
        let req = ApprovalRequirement::Manual {
            reason: "needs review".to_string(),
        };
        if let ApprovalRequirement::Manual { reason } = &req {
            assert_eq!(reason, "needs review");
        } else {
            panic!("expected Manual variant");
        }
    }

    // --- ApprovalDecisionRecord tests ---

    #[test]
    fn detail_approved_formats_exact_string() {
        let record = ApprovalDecisionRecord {
            request_seq: EventSeq(42),
            decision: ApprovalDecision::Approved,
            reason: "looks good".to_string(),
        };
        assert_eq!(
            record.detail(),
            "request_seq=42 decision=approved reason=looks good"
        );
    }

    #[test]
    fn detail_rejected_formats_exact_string() {
        let record = ApprovalDecisionRecord {
            request_seq: EventSeq(7),
            decision: ApprovalDecision::Rejected,
            reason: "too risky".to_string(),
        };
        assert_eq!(
            record.detail(),
            "request_seq=7 decision=rejected reason=too risky"
        );
    }

    #[test]
    fn parse_detail_valid_approved() {
        let record =
            ApprovalDecisionRecord::parse_detail("request_seq=1 decision=approved reason=ok")
                .expect("should parse");
        assert_eq!(record.request_seq, EventSeq(1));
        assert_eq!(record.decision, ApprovalDecision::Approved);
        assert_eq!(record.reason, "ok");
    }

    #[test]
    fn parse_detail_valid_rejected() {
        let record = ApprovalDecisionRecord::parse_detail(
            "request_seq=99 decision=rejected reason=not safe",
        )
        .expect("should parse");
        assert_eq!(record.request_seq, EventSeq(99));
        assert_eq!(record.decision, ApprovalDecision::Rejected);
        assert_eq!(record.reason, "not safe");
    }

    #[test]
    fn parse_detail_returns_none_for_empty_string() {
        assert!(ApprovalDecisionRecord::parse_detail("").is_none());
    }

    #[test]
    fn parse_detail_returns_none_for_missing_request_seq() {
        assert!(ApprovalDecisionRecord::parse_detail("decision=approved reason=ok").is_none());
    }

    #[test]
    fn parse_detail_returns_none_for_non_numeric_request_seq() {
        assert!(
            ApprovalDecisionRecord::parse_detail("request_seq=abc decision=approved reason=ok")
                .is_none()
        );
    }

    #[test]
    fn parse_detail_returns_none_for_bad_decision_value() {
        assert!(
            ApprovalDecisionRecord::parse_detail("request_seq=1 decision=maybe reason=ok")
                .is_none()
        );
    }

    #[test]
    fn parse_detail_returns_none_for_missing_reason() {
        assert!(ApprovalDecisionRecord::parse_detail("request_seq=1 decision=approved").is_none());
    }

    #[test]
    fn parse_detail_returns_none_for_empty_reason() {
        assert!(
            ApprovalDecisionRecord::parse_detail("request_seq=1 decision=approved reason=")
                .is_none()
        );
    }

    #[test]
    fn detail_parse_detail_round_trip() {
        let original = ApprovalDecisionRecord {
            request_seq: EventSeq(55),
            decision: ApprovalDecision::Approved,
            reason: "all clear".to_string(),
        };
        let detail = original.detail();
        let parsed =
            ApprovalDecisionRecord::parse_detail(&detail).expect("round-trip should parse");
        assert_eq!(parsed.request_seq, original.request_seq);
        assert_eq!(parsed.decision, original.decision);
        assert_eq!(parsed.reason, original.reason);
    }
}
