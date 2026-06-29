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
use crate::tool::registry::ToolRequest;

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

/// A parsed representation of an `ApprovalRequest` event detail string.
///
/// The detail string is produced by `format_approval_request_detail` using Rust's
/// `{:?}` Debug format for the path, so the path may be quoted and may contain
/// spaces or `=` characters. Use [`ParsedApprovalRequest::parse_detail`] to
/// decode it safely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedApprovalRequest {
    pub tool: String,
    pub path: String,
    pub risk: String,
    pub reason: String,
}

impl ParsedApprovalRequest {
    /// Parses an `ApprovalRequest` event detail string into a [`ParsedApprovalRequest`].
    ///
    /// The expected format is:
    /// `tool=<tool> path=<debug-quoted-or-bare-path> risk=<risk> reason=<reason>`
    ///
    /// Uses a label-aware fixed-order scan rather than a naive whitespace split so
    /// that paths containing spaces or `=` (e.g. `path="dir/a b.txt"`) are handled
    /// correctly. Returns `None` for any malformed input; never panics.
    pub fn parse_detail(detail: &str) -> Option<Self> {
        // Require "tool=" prefix and read tool value up to " path=".
        let rest = detail.strip_prefix("tool=")?;
        let path_label_pos = rest.find(" path=")?;
        let tool = rest[..path_label_pos].to_string();
        let rest = &rest[path_label_pos + " path=".len()..];

        // Parse path value: quoted (Debug-style) or unquoted.
        let (path, rest) = if rest.starts_with('"') {
            parse_debug_quoted_path(&rest[1..])?
        } else {
            let pos = rest.find(" risk=")?;
            (rest[..pos].to_string(), &rest[pos..])
        };

        // Require " risk=" and read risk value up to " reason=".
        let rest = rest.strip_prefix(" risk=")?;
        let reason_pos = rest.find(" reason=")?;
        let risk = rest[..reason_pos].to_string();
        let reason = rest[reason_pos + " reason=".len()..].to_string();

        Some(ParsedApprovalRequest {
            tool,
            path,
            risk,
            reason,
        })
    }

    /// Converts this parsed request into a [`ToolRequest`], if the tool name is
    /// recognised. Returns `None` for unsupported tool names.
    ///
    /// This method is pure: it does not access the filesystem or canonicalize paths.
    pub fn to_tool_request(&self) -> Option<ToolRequest> {
        match self.tool.as_str() {
            "read_file" => Some(ToolRequest::ReadFile {
                path: self.path.clone(),
                offset: None,
                limit: None,
            }),
            "list_files" => Some(ToolRequest::ListFiles {
                path: self.path.clone(),
            }),
            _ => None,
        }
    }
}

/// A projection that ties a parsed approval request to its approval/decision
/// sequence numbers so a later step can resume it.
///
/// The `request_detail` field carries the original event detail string verbatim
/// so the `/approval status` display can stay faithful to the recorded event
/// rather than re-serialising parsed fields.
pub struct ApprovalResumePlan {
    pub request_seq: EventSeq,
    pub decision_seq: EventSeq,
    pub request_detail: String,
    pub request: ParsedApprovalRequest,
}

impl ApprovalResumePlan {
    /// Converts the inner [`ParsedApprovalRequest`] into a [`ToolRequest`].
    ///
    /// Delegates to [`ParsedApprovalRequest::to_tool_request`] and returns
    /// `None` for unsupported tool names.
    pub fn to_tool_request(&self) -> Option<ToolRequest> {
        self.request.to_tool_request()
    }

    /// Returns the `/tool` command string the operator should run to replay
    /// this request, or `None` if the tool is not supported.
    ///
    /// Uses the bare (unquoted) path from `self.request.path`.
    pub fn suggested_command(&self) -> Option<String> {
        match self.request.tool.as_str() {
            "read_file" => Some(format!("/tool read {}", self.request.path)),
            "list_files" => Some(format!("/tool list {}", self.request.path)),
            _ => None,
        }
    }

    /// Returns the detail string for an `ApprovalResume` event.
    ///
    /// Builds an [`ApprovalResumeRecord`] from this plan's sequence numbers
    /// and parsed request fields, then returns `record.detail()`.
    pub fn resume_detail(&self) -> String {
        let record = ApprovalResumeRecord {
            request_seq: self.request_seq,
            decision_seq: self.decision_seq,
            tool: self.request.tool.clone(),
            path: self.request.path.clone(),
            risk: self.request.risk.clone(),
            reason: self.request.reason.clone(),
        };
        record.detail()
    }
}

/// A recorded approval resume event detail.
///
/// The detail string format is key=value (NOT JSON):
/// `request_seq=<u64> decision_seq=<u64> tool=<tool> path=<debug-quoted-path> risk=<risk> reason=<reason>`
///
/// The path is Debug-quoted (via `{:?}`) so paths containing spaces or `=` are
/// preserved correctly. Use [`ApprovalResumeRecord::parse_detail`] to decode safely.
pub struct ApprovalResumeRecord {
    pub request_seq: EventSeq,
    pub decision_seq: EventSeq,
    pub tool: String,
    pub path: String,
    pub risk: String,
    pub reason: String,
}

impl ApprovalResumeRecord {
    /// Formats this record as an EventLog detail string.
    ///
    /// Output: `request_seq={seq} decision_seq={seq} tool={tool} path={path:?} risk={risk} reason={reason}`
    ///
    /// The path is encoded with Rust Debug (`{:?}`) to produce the quoted form so it
    /// round-trips correctly through [`Self::parse_detail`].
    pub fn detail(&self) -> String {
        format!(
            "request_seq={} decision_seq={} tool={} path={:?} risk={} reason={}",
            self.request_seq, self.decision_seq, self.tool, self.path, self.risk, self.reason
        )
    }

    /// Parses an EventLog detail string into an [`ApprovalResumeRecord`].
    ///
    /// The expected format is:
    /// `request_seq=<u64> decision_seq=<u64> tool=<tool> path=<debug-quoted-or-bare-path> risk=<risk> reason=<reason>`
    ///
    /// Uses a label-aware fixed-order positional scan (not `splitn`/whitespace split)
    /// so that Debug-quoted paths containing spaces or `=` are handled correctly.
    /// Returns `None` for any malformed input; never panics.
    pub fn parse_detail(detail: &str) -> Option<Self> {
        // Strip "request_seq=" and read value up to " decision_seq=".
        let rest = detail.strip_prefix("request_seq=")?;
        let decision_seq_label_pos = rest.find(" decision_seq=")?;
        let request_seq: u64 = rest[..decision_seq_label_pos].parse().ok()?;
        let rest = &rest[decision_seq_label_pos + " decision_seq=".len()..];

        // Read decision_seq value up to " tool=".
        let tool_label_pos = rest.find(" tool=")?;
        let decision_seq: u64 = rest[..tool_label_pos].parse().ok()?;
        let rest = &rest[tool_label_pos + " tool=".len()..];

        // Read tool value up to " path=".
        let path_label_pos = rest.find(" path=")?;
        let tool = rest[..path_label_pos].to_string();
        let rest = &rest[path_label_pos + " path=".len()..];

        // Parse path value. `detail()` always Debug-quotes the path, so the value
        // MUST start with a quote. A bare (unquoted) path is malformed for this
        // event format and is rejected (returns None), keeping the projection's
        // "malformed detail → ignore" rule precise.
        if !rest.starts_with('"') {
            return None;
        }
        let (path, rest) = parse_debug_quoted_path(&rest[1..])?;

        // Require " risk=" immediately after the closing quote (this also rejects
        // any trailing junk between the closing quote and " risk="), then read the
        // risk value up to " reason=".
        let rest = rest.strip_prefix(" risk=")?;
        let reason_pos = rest.find(" reason=")?;
        let risk = rest[..reason_pos].to_string();
        let reason = rest[reason_pos + " reason=".len()..].to_string();

        Some(ApprovalResumeRecord {
            request_seq: EventSeq(request_seq),
            decision_seq: EventSeq(decision_seq),
            tool,
            path,
            risk,
            reason,
        })
    }
}

/// Consumes a Rust Debug-quoted string from `s` (the content after the opening `"`).
///
/// Decodes `\"` → `"` and `\\` → `\`. Returns the decoded string and the remaining
/// slice (starting with the character after the closing `"`). Returns `None` if the
/// closing `"` is never found or an unrecognised escape sequence is encountered.
fn parse_debug_quoted_path(s: &str) -> Option<(String, &str)> {
    let mut result = String::new();
    let mut chars = s.char_indices();
    loop {
        let (i, c) = chars.next()?;
        match c {
            '"' => return Some((result, &s[i + 1..])),
            '\\' => {
                let (_, escaped) = chars.next()?;
                match escaped {
                    '"' => result.push('"'),
                    '\\' => result.push('\\'),
                    _ => return None,
                }
            }
            _ => result.push(c),
        }
    }
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

    // --- ParsedApprovalRequest tests ---

    #[test]
    fn parsed_approval_request_canonical_read_file() {
        let detail =
            r#"tool=read_file path="README.md" risk=read_only reason=test_manual_approval"#;
        let parsed = ParsedApprovalRequest::parse_detail(detail).expect("should parse");
        assert_eq!(parsed.tool, "read_file");
        assert_eq!(parsed.path, "README.md");
        assert_eq!(parsed.risk, "read_only");
        assert_eq!(parsed.reason, "test_manual_approval");
    }

    #[test]
    fn parsed_approval_request_canonical_list_files() {
        let detail = r#"tool=list_files path="." risk=read_only reason=browse"#;
        let parsed = ParsedApprovalRequest::parse_detail(detail).expect("should parse");
        assert_eq!(parsed.tool, "list_files");
        assert_eq!(parsed.path, ".");
        assert_eq!(parsed.risk, "read_only");
        assert_eq!(parsed.reason, "browse");
    }

    #[test]
    fn parsed_approval_request_missing_tool_returns_none() {
        let detail = r#"path="README.md" risk=read_only reason=test"#;
        assert!(ParsedApprovalRequest::parse_detail(detail).is_none());
    }

    #[test]
    fn parsed_approval_request_missing_path_returns_none() {
        let detail = "tool=read_file risk=read_only reason=test";
        assert!(ParsedApprovalRequest::parse_detail(detail).is_none());
    }

    #[test]
    fn parsed_approval_request_missing_risk_returns_none() {
        let detail = r#"tool=read_file path="README.md" reason=test"#;
        assert!(ParsedApprovalRequest::parse_detail(detail).is_none());
    }

    #[test]
    fn parsed_approval_request_missing_reason_returns_none() {
        let detail = r#"tool=read_file path="README.md" risk=read_only"#;
        assert!(ParsedApprovalRequest::parse_detail(detail).is_none());
    }

    #[test]
    fn parsed_approval_request_malformed_input_returns_none() {
        assert!(ParsedApprovalRequest::parse_detail("").is_none());
        assert!(ParsedApprovalRequest::parse_detail("not valid at all").is_none());
        assert!(ParsedApprovalRequest::parse_detail("tool=x").is_none());
    }

    #[test]
    fn parsed_approval_request_debug_quoted_path_stripped_to_bare() {
        // Path value is Debug-quoted: quotes are removed, bare path is returned.
        let detail = r#"tool=read_file path="src/main.rs" risk=read_only reason=check"#;
        let parsed = ParsedApprovalRequest::parse_detail(detail).expect("should parse");
        assert_eq!(parsed.path, "src/main.rs");
    }

    #[test]
    fn parsed_approval_request_quoted_path_with_space() {
        let detail = r#"tool=read_file path="dir/a b.txt" risk=read_only reason=check"#;
        let parsed = ParsedApprovalRequest::parse_detail(detail).expect("should parse");
        assert_eq!(parsed.path, "dir/a b.txt");
    }

    #[test]
    fn parsed_approval_request_quoted_path_with_equals() {
        let detail = r#"tool=read_file path="a=b.txt" risk=read_only reason=check"#;
        let parsed = ParsedApprovalRequest::parse_detail(detail).expect("should parse");
        assert_eq!(parsed.path, "a=b.txt");
    }

    #[test]
    fn parsed_approval_request_quoted_path_with_escaped_quote() {
        // The raw detail string contains: path="dir/quote\"file.txt"
        // which decodes to the path: dir/quote"file.txt
        let detail = "tool=read_file path=\"dir/quote\\\"file.txt\" risk=read_only reason=check";
        let parsed = ParsedApprovalRequest::parse_detail(detail).expect("should parse");
        assert_eq!(parsed.path, "dir/quote\"file.txt");
    }

    #[test]
    fn parsed_approval_request_empty_path() {
        let detail = r#"tool=read_file path="" risk=read_only reason=check"#;
        let parsed = ParsedApprovalRequest::parse_detail(detail).expect("should parse");
        assert_eq!(parsed.path, "");
    }

    #[test]
    fn parsed_approval_request_reason_with_spaces_captured_in_full() {
        let detail =
            r#"tool=read_file path="file.txt" risk=read_only reason=this is the full reason"#;
        let parsed = ParsedApprovalRequest::parse_detail(detail).expect("should parse");
        assert_eq!(parsed.reason, "this is the full reason");
    }

    #[test]
    fn parsed_approval_request_to_tool_request_read_file() {
        let detail = r#"tool=read_file path="README.md" risk=read_only reason=test"#;
        let parsed = ParsedApprovalRequest::parse_detail(detail).expect("should parse");
        let request = parsed.to_tool_request().expect("should convert");
        assert_eq!(
            request,
            ToolRequest::ReadFile {
                path: "README.md".to_string(),
                offset: None,
                limit: None,
            }
        );
    }

    #[test]
    fn parsed_approval_request_to_tool_request_list_files() {
        let detail = r#"tool=list_files path="." risk=read_only reason=browse"#;
        let parsed = ParsedApprovalRequest::parse_detail(detail).expect("should parse");
        let request = parsed.to_tool_request().expect("should convert");
        assert_eq!(
            request,
            ToolRequest::ListFiles {
                path: ".".to_string()
            }
        );
    }

    #[test]
    fn parsed_approval_request_to_tool_request_unsupported_tool_returns_none() {
        let detail = r#"tool=write_file path="output.txt" risk=high reason=test"#;
        let parsed = ParsedApprovalRequest::parse_detail(detail).expect("should parse");
        assert!(parsed.to_tool_request().is_none());
    }

    // --- ApprovalResumePlan tests ---

    fn make_approval_resume_plan(tool: &str, path: &str) -> ApprovalResumePlan {
        let detail = format!("tool={} path=\"{}\" risk=read_only reason=test", tool, path);
        let request = ParsedApprovalRequest::parse_detail(&detail).expect("should parse");
        ApprovalResumePlan {
            request_seq: EventSeq(10),
            decision_seq: EventSeq(11),
            request_detail: detail,
            request,
        }
    }

    #[test]
    fn approval_resume_plan_to_tool_request_delegates_read_file() {
        let plan = make_approval_resume_plan("read_file", "README.md");
        let request = plan.to_tool_request().expect("should convert");
        assert_eq!(
            request,
            ToolRequest::ReadFile {
                path: "README.md".to_string(),
                offset: None,
                limit: None,
            }
        );
    }

    #[test]
    fn approval_resume_plan_suggested_command_read_file() {
        let plan = make_approval_resume_plan("read_file", "src/main.rs");
        assert_eq!(
            plan.suggested_command(),
            Some("/tool read src/main.rs".to_string())
        );
    }

    #[test]
    fn approval_resume_plan_suggested_command_list_files() {
        let plan = make_approval_resume_plan("list_files", ".");
        assert_eq!(plan.suggested_command(), Some("/tool list .".to_string()));
    }

    #[test]
    fn approval_resume_plan_suggested_command_unsupported_returns_none() {
        let plan = make_approval_resume_plan("write_file", "output.txt");
        assert!(plan.suggested_command().is_none());
    }

    // --- ApprovalResumeRecord tests ---

    fn make_approval_resume_record(path: &str) -> ApprovalResumeRecord {
        ApprovalResumeRecord {
            request_seq: EventSeq(12),
            decision_seq: EventSeq(15),
            tool: "read_file".to_string(),
            path: path.to_string(),
            risk: "read_only".to_string(),
            reason: "test_manual_approval".to_string(),
        }
    }

    #[test]
    fn approval_resume_record_detail_exact_string() {
        let record = make_approval_resume_record("README.md");
        assert_eq!(
            record.detail(),
            r#"request_seq=12 decision_seq=15 tool=read_file path="README.md" risk=read_only reason=test_manual_approval"#
        );
    }

    #[test]
    fn approval_resume_record_detail_parse_detail_round_trip() {
        let original = make_approval_resume_record("src/main.rs");
        let detail = original.detail();
        let parsed = ApprovalResumeRecord::parse_detail(&detail).expect("round-trip should parse");
        assert_eq!(parsed.request_seq, original.request_seq);
        assert_eq!(parsed.decision_seq, original.decision_seq);
        assert_eq!(parsed.tool, original.tool);
        assert_eq!(parsed.path, original.path);
        assert_eq!(parsed.risk, original.risk);
        assert_eq!(parsed.reason, original.reason);
    }

    #[test]
    fn approval_resume_record_path_with_spaces_round_trip() {
        let original = make_approval_resume_record("dir/a b.txt");
        let detail = original.detail();
        let parsed =
            ApprovalResumeRecord::parse_detail(&detail).expect("path-with-spaces should parse");
        assert_eq!(parsed.path, "dir/a b.txt");
    }

    #[test]
    fn approval_resume_record_parse_detail_malformed_returns_none() {
        assert!(ApprovalResumeRecord::parse_detail("").is_none());
        assert!(ApprovalResumeRecord::parse_detail("not valid at all").is_none());
        assert!(
            ApprovalResumeRecord::parse_detail(
                "request_seq=abc decision_seq=1 tool=read_file path=\"f\" risk=low reason=x"
            )
            .is_none()
        );
        assert!(
            ApprovalResumeRecord::parse_detail(
                "request_seq=1 decision_seq=abc tool=read_file path=\"f\" risk=low reason=x"
            )
            .is_none()
        );
        assert!(ApprovalResumeRecord::parse_detail("request_seq=1 decision_seq=2").is_none());
    }

    #[test]
    fn approval_resume_record_resume_detail_round_trip_all_fields_match() {
        let plan = ApprovalResumePlan {
            request_seq: EventSeq(12),
            decision_seq: EventSeq(15),
            request_detail: String::new(),
            request: ParsedApprovalRequest {
                tool: "read_file".to_string(),
                path: "README.md".to_string(),
                risk: "read_only".to_string(),
                reason: "test_manual_approval".to_string(),
            },
        };
        let detail = plan.resume_detail();
        let parsed =
            ApprovalResumeRecord::parse_detail(&detail).expect("resume_detail should parse");
        assert_eq!(parsed.request_seq, EventSeq(12));
        assert_eq!(parsed.decision_seq, EventSeq(15));
        assert_eq!(parsed.tool, "read_file");
        assert_eq!(parsed.path, "README.md");
        assert_eq!(parsed.risk, "read_only");
        assert_eq!(parsed.reason, "test_manual_approval");
    }

    #[test]
    fn approval_resume_record_parse_detail_rejects_bare_path() {
        // detail() always Debug-quotes the path, so a bare path is malformed.
        assert!(
            ApprovalResumeRecord::parse_detail(
                "request_seq=1 decision_seq=2 tool=read_file path=notes.txt risk=low reason=ok"
            )
            .is_none()
        );
    }

    #[test]
    fn approval_resume_record_parse_detail_rejects_trailing_junk_after_quote() {
        assert!(
            ApprovalResumeRecord::parse_detail(
                r#"request_seq=1 decision_seq=2 tool=read_file path="notes.txt"x risk=low reason=ok"#
            )
            .is_none()
        );
    }

    #[test]
    fn approval_resume_record_parse_detail_rejects_unterminated_quote() {
        assert!(
            ApprovalResumeRecord::parse_detail(
                r#"request_seq=1 decision_seq=2 tool=read_file path="unterminated risk=low reason=ok"#
            )
            .is_none()
        );
    }

    #[test]
    fn approval_resume_record_debug_escaped_path_round_trip() {
        // Paths containing a quote or backslash must round-trip via Debug escaping.
        for raw in [r#"dir/"quoted"/notes.txt"#, r"dir\notes.txt"] {
            let original = make_approval_resume_record(raw);
            let detail = original.detail();
            let parsed = ApprovalResumeRecord::parse_detail(&detail)
                .expect("debug-escaped path should round-trip");
            assert_eq!(parsed.path, raw);
        }
    }
}
