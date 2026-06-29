//! Policy engine that classifies a [`ToolRequest`] and produces a decision.
//!
//! The current policy auto-allows every read-only tool. `ToolPolicyDecision`
//! still defines a `Deny` variant for type completeness and future approval
//! gating; the `Deny` branch is exercised only by tests.

use crate::approval::ApprovalRequirement;
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
    pub approval_requirement: ApprovalRequirement,
}

/// Evaluates tool requests against a policy.
pub struct ToolPolicyEngine {
    deny_all: bool,
    manual_reason: Option<String>,
}

impl ToolPolicyEngine {
    /// Creates an engine that auto-allows all read-only tools.
    pub fn read_only() -> Self {
        ToolPolicyEngine {
            deny_all: false,
            manual_reason: None,
        }
    }

    /// Creates an engine that denies all requests (test-only).
    #[cfg(test)]
    pub(crate) fn deny_all() -> Self {
        ToolPolicyEngine {
            deny_all: true,
            manual_reason: None,
        }
    }

    /// Creates an engine that allows all requests and signals manual approval
    /// with the given reason (test-only).
    #[cfg(test)]
    pub(crate) fn manual_for_test(reason: impl Into<String>) -> Self {
        ToolPolicyEngine {
            deny_all: false,
            manual_reason: Some(reason.into()),
        }
    }

    /// Evaluates a tool request and returns the policy outcome.
    pub fn evaluate(&self, request: &ToolRequest) -> ToolPolicyOutcome {
        let risk = match request {
            ToolRequest::ListFiles { .. }
            | ToolRequest::ReadFile { .. }
            | ToolRequest::PreviewWrite { .. }
            | ToolRequest::SearchText { .. }
            | ToolRequest::GlobFiles { .. } => ToolRisk::ReadOnly,
            ToolRequest::PlanWrite { .. } => ToolRisk::WorkspaceWrite,
        };

        if self.deny_all {
            ToolPolicyOutcome {
                decision: ToolPolicyDecision::Deny,
                risk,
                reason: "deny_all".to_string(),
                approval_requirement: ApprovalRequirement::None,
            }
        } else {
            let reason = match risk {
                ToolRisk::ReadOnly => "read_only_auto_allow".to_string(),
                ToolRisk::WorkspaceWrite => "workspace_write_requires_approval".to_string(),
            };
            let approval_requirement = match &self.manual_reason {
                Some(r) => ApprovalRequirement::Manual { reason: r.clone() },
                None => match risk {
                    ToolRisk::WorkspaceWrite => ApprovalRequirement::Manual {
                        reason: "workspace_write_requires_approval".to_string(),
                    },
                    ToolRisk::ReadOnly => ApprovalRequirement::None,
                },
            };
            ToolPolicyOutcome {
                decision: ToolPolicyDecision::Allow,
                risk,
                reason,
                approval_requirement,
            }
        }
    }
}

/// Formats a canonical detail string for a tool policy event.
///
/// The `path` parameter is `&str` (not `&Path`) to match the convention in
/// `tool_events.rs`. The path is formatted with `{:?}` debug formatting.
/// For range reads (`offset` or `limit` is `Some`), `offset={}` and `limit={}`
/// are inserted between `path` and `risk`.
#[rustfmt::skip]
pub(crate) fn format_tool_policy_detail(
    tool_name: &str,
    path: &str,
    outcome: &ToolPolicyOutcome,
    offset: Option<usize>,
    limit: Option<usize>,
) -> String {
    use crate::tool::registry::{DEFAULT_READ_RANGE_LIMIT_LINES};
    match (offset, limit) {
        (None, None) => format!(
            "tool={} path={:?} risk={} decision={} reason={}",
            tool_name,
            path,
            outcome.risk.as_str(),
            outcome.decision.as_str(),
            outcome.reason
        ),
        _ => format!(
            "tool={} path={:?} offset={} limit={} risk={} decision={} reason={}",
            tool_name,
            path,
            offset.unwrap_or(1),
            limit.unwrap_or(DEFAULT_READ_RANGE_LIMIT_LINES),
            outcome.risk.as_str(),
            outcome.decision.as_str(),
            outcome.reason
        ),
    }
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
            offset: None,
            limit: None,
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
            approval_requirement: ApprovalRequirement::None,
        };
        let detail = format_tool_policy_detail("list_files", ".", &outcome, None, None);
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
            approval_requirement: ApprovalRequirement::None,
        };
        let detail = format_tool_policy_detail("list_files", ".", &outcome, None, None);
        assert!(
            detail.contains("decision=deny"),
            "expected decision=deny in detail: {detail}"
        );
    }

    #[test]
    fn format_tool_policy_detail_range_read_includes_offset_and_limit() {
        let outcome = ToolPolicyOutcome {
            decision: ToolPolicyDecision::Allow,
            risk: ToolRisk::ReadOnly,
            reason: "read_only_auto_allow".to_string(),
            approval_requirement: ApprovalRequirement::None,
        };
        let detail =
            format_tool_policy_detail("read_file", "src/lib.rs", &outcome, Some(5), Some(20));
        assert!(
            detail.contains("offset=5"),
            "expected offset=5 in detail: {detail}"
        );
        assert!(
            detail.contains("limit=20"),
            "expected limit=20 in detail: {detail}"
        );
        assert!(
            detail.contains("risk=read_only"),
            "expected risk=read_only in detail: {detail}"
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

    #[test]
    fn read_only_engine_produces_approval_requirement_none() {
        let engine = ToolPolicyEngine::read_only();
        let request = ToolRequest::ListFiles {
            path: ".".to_string(),
        };
        let outcome = engine.evaluate(&request);
        assert_eq!(outcome.approval_requirement, ApprovalRequirement::None);
    }

    #[test]
    fn manual_for_test_engine_produces_approval_requirement_manual_with_reason() {
        let engine = ToolPolicyEngine::manual_for_test("needs review");
        let request = ToolRequest::ListFiles {
            path: ".".to_string(),
        };
        let outcome = engine.evaluate(&request);
        assert_eq!(
            outcome.approval_requirement,
            ApprovalRequirement::Manual {
                reason: "needs review".to_string(),
            }
        );
    }

    #[test]
    fn evaluate_plan_write_returns_allow_workspace_write_with_manual_approval() {
        let engine = ToolPolicyEngine::read_only();
        let request = ToolRequest::PlanWrite {
            path: "README.md".to_string(),
        };
        let outcome = engine.evaluate(&request);
        assert_eq!(outcome.decision, ToolPolicyDecision::Allow);
        assert_eq!(outcome.risk, ToolRisk::WorkspaceWrite);
        assert_eq!(outcome.reason, "workspace_write_requires_approval");
        assert_eq!(
            outcome.approval_requirement,
            ApprovalRequirement::Manual {
                reason: "workspace_write_requires_approval".to_string(),
            }
        );
    }

    #[test]
    fn tool_risk_as_str_workspace_write() {
        assert_eq!(ToolRisk::WorkspaceWrite.as_str(), "workspace_write");
    }

    #[test]
    fn tool_risk_as_str_read_only() {
        assert_eq!(ToolRisk::ReadOnly.as_str(), "read_only");
    }

    #[test]
    fn evaluate_preview_write_returns_allow_read_only_auto_allow_no_approval() {
        let engine = ToolPolicyEngine::read_only();
        let request = ToolRequest::PreviewWrite {
            path: "README.md".to_string(),
            content: "proposed content".to_string(),
        };
        let outcome = engine.evaluate(&request);
        assert_eq!(outcome.decision, ToolPolicyDecision::Allow);
        assert_eq!(outcome.risk, ToolRisk::ReadOnly);
        assert_eq!(outcome.reason, "read_only_auto_allow");
        assert_eq!(outcome.approval_requirement, ApprovalRequirement::None);
    }
}
