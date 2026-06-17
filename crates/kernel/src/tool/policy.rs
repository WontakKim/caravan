//! Policy engine that classifies a [`ToolRequest`] and produces a decision.
//!
//! The current policy auto-allows every read-only tool. `ToolPolicyDecision`
//! still defines a `Deny` variant for type completeness and future approval
//! gating; the `Deny` branch is exercised only by tests.

use crate::tool::registry::{ToolRequest, ToolRisk};

/// The outcome of a policy evaluation: allow or deny.
#[derive(Debug, PartialEq)]
pub enum ToolPolicyDecision {
    Allow,
    Deny,
}

impl ToolPolicyDecision {
    /// Returns the canonical snake_case string for this decision.
    pub fn as_str(&self) -> &'static str {
        match self {
            ToolPolicyDecision::Allow => "allow",
            ToolPolicyDecision::Deny => "deny",
        }
    }
}

/// Full outcome of a policy evaluation.
#[derive(Debug, PartialEq)]
pub struct ToolPolicyOutcome {
    pub decision: ToolPolicyDecision,
    pub risk: ToolRisk,
    pub reason: String,
}

/// Evaluates tool requests against a policy.
pub struct ToolPolicyEngine {
    deny_all: bool,
}

impl ToolPolicyEngine {
    /// Creates an engine that auto-allows all read-only tools.
    pub fn read_only() -> Self {
        ToolPolicyEngine { deny_all: false }
    }

    /// Creates an engine that denies all requests (test-only).
    #[cfg(test)]
    pub(crate) fn deny_all() -> Self {
        ToolPolicyEngine { deny_all: true }
    }

    /// Evaluates a tool request and returns the policy outcome.
    pub fn evaluate(&self, request: &ToolRequest) -> ToolPolicyOutcome {
        let risk = match request {
            ToolRequest::ListFiles { .. } | ToolRequest::ReadFile { .. } => ToolRisk::ReadOnly,
        };

        if self.deny_all {
            ToolPolicyOutcome {
                decision: ToolPolicyDecision::Deny,
                risk,
                reason: "deny_all".to_string(),
            }
        } else {
            ToolPolicyOutcome {
                decision: ToolPolicyDecision::Allow,
                risk,
                reason: "read_only_auto_allow".to_string(),
            }
        }
    }
}

/// Formats a canonical detail string for a tool policy event.
///
/// The `path` parameter is `&str` (not `&Path`) to match the convention in
/// `tool_events.rs`. The path is formatted with `{:?}` debug formatting.
#[rustfmt::skip]
pub(crate) fn format_tool_policy_detail(tool_name: &str, path: &str, outcome: &ToolPolicyOutcome) -> String {
    format!(
        "tool={} path={:?} risk={} decision={} reason={}",
        tool_name,
        path,
        outcome.risk.as_str(),
        outcome.decision.as_str(),
        outcome.reason
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::registry::ToolRequest;

    #[test]
    fn evaluate_list_files_returns_allow_read_only_auto_allow() {
        let engine = ToolPolicyEngine::read_only();
        let request = ToolRequest::ListFiles {
            path: ".".to_string(),
        };
        let outcome = engine.evaluate(&request);
        assert_eq!(outcome.decision, ToolPolicyDecision::Allow);
        assert_eq!(outcome.risk, ToolRisk::ReadOnly);
        assert_eq!(outcome.reason, "read_only_auto_allow");
    }

    #[test]
    fn evaluate_read_file_returns_allow_read_only_auto_allow() {
        let engine = ToolPolicyEngine::read_only();
        let request = ToolRequest::ReadFile {
            path: "hello.txt".to_string(),
        };
        let outcome = engine.evaluate(&request);
        assert_eq!(outcome.decision, ToolPolicyDecision::Allow);
        assert_eq!(outcome.risk, ToolRisk::ReadOnly);
        assert_eq!(outcome.reason, "read_only_auto_allow");
    }

    #[test]
    fn format_tool_policy_detail_allow_outcome() {
        let outcome = ToolPolicyOutcome {
            decision: ToolPolicyDecision::Allow,
            risk: ToolRisk::ReadOnly,
            reason: "read_only_auto_allow".to_string(),
        };
        let detail = format_tool_policy_detail("list_files", ".", &outcome);
        assert_eq!(
            detail,
            r#"tool=list_files path="." risk=read_only decision=allow reason=read_only_auto_allow"#
        );
    }

    #[test]
    fn format_tool_policy_detail_deny_outcome_contains_decision_deny() {
        let outcome = ToolPolicyOutcome {
            decision: ToolPolicyDecision::Deny,
            risk: ToolRisk::ReadOnly,
            reason: "deny_all".to_string(),
        };
        let detail = format_tool_policy_detail("list_files", ".", &outcome);
        assert!(
            detail.contains("decision=deny"),
            "expected decision=deny in detail: {detail}"
        );
    }

    #[test]
    fn deny_all_engine_returns_deny_decision() {
        let engine = ToolPolicyEngine::deny_all();
        let request = ToolRequest::ListFiles {
            path: ".".to_string(),
        };
        let outcome = engine.evaluate(&request);
        assert_eq!(outcome.decision, ToolPolicyDecision::Deny);
    }
}
