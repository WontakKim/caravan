//! Approval gate types for tool-execution approval flows.
//!
//! These types are pure data; they do not touch the event log, policy engine,
//! or runner. Wiring into the runtime is deferred to a future task.

/// Whether a tool invocation requires human approval before it may proceed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalRequirement {
    /// No approval is required; the tool may run immediately.
    None,
    /// A human must approve before the tool runs.
    Manual { reason: String },
}

/// The outcome of a human approval decision.
#[allow(dead_code)] // variants wired in a future approval-flow task
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalDecision {
    Approved,
    Rejected,
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
}
