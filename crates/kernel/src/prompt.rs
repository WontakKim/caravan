use crate::manual_context::ManualToolContext;
use crate::project_memory::ProjectMemory;
use crate::transcript::{TranscriptMessage, TranscriptRole};
use crate::workspace_reference::WorkspaceReferences;

/// Number of most-recent transcript messages rendered in the prompt's
/// conversation window. This is a small fixed window — NOT long-term memory.
pub const DEFAULT_PROMPT_HISTORY_MESSAGES: usize = 6;

/// Compiles the prompt for the current user message, including a short recent
/// conversation window and optional project memory.
///
/// `history` is the prior conversation, already excluding the current user
/// message (see `ConversationTranscript::without_trailing_user_message`). Only
/// the last `DEFAULT_PROMPT_HISTORY_MESSAGES` messages are rendered; when that
/// window is empty the Conversation section shows `No prior conversation
/// context.`. The function renders whatever history it is given verbatim — it
/// does not de-duplicate by content.
///
/// `project_memory` is rendered in the `Project Memory:` section. When `None`,
/// the fallback from `ProjectMemory::missing()` is used.
///
/// `referenced_context` renders a `Referenced Workspace Context:` sub-block
/// inside `Workspace Context:`, before `manual_tool_context`'s `Attached
/// Workspace Context:` sub-block. A `Some` value whose
/// `render_prompt_section()` is empty (no items and nothing omitted) is
/// treated the same as `None`.
pub fn compile_prompt_with_context(
    current_user_message: &str,
    history: &[TranscriptMessage],
    referenced_context: Option<&WorkspaceReferences>,
    manual_tool_context: Option<&ManualToolContext>,
    project_memory: Option<&ProjectMemory>,
) -> String {
    let window_start = history
        .len()
        .saturating_sub(DEFAULT_PROMPT_HISTORY_MESSAGES);
    let window = &history[window_start..];

    let conversation = if window.is_empty() {
        "No prior conversation context.".to_string()
    } else {
        window
            .iter()
            .map(|message| {
                let role = match message.role {
                    TranscriptRole::User => "User",
                    TranscriptRole::Assistant => "Assistant",
                };
                format!("{}: {}", role, message.content)
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let memory_content = match project_memory {
        Some(pm) => pm.content.clone(),
        None => ProjectMemory::missing().content,
    };

    let referenced = referenced_context
        .map(|r| r.render_prompt_section())
        .filter(|s| !s.is_empty());
    let attached = manual_tool_context.map(|ctx| {
        format!(
            "Attached Workspace Context:\nSource:\n  {}\nContent:\n{}",
            ctx.source_label(),
            ctx.content
        )
    });

    let workspace_context = match (referenced, attached) {
        (None, None) => "No external tool context is attached.".to_string(),
        (Some(referenced), None) => referenced,
        (None, Some(attached)) => attached,
        (Some(referenced), Some(attached)) => format!("{}\n\n{}", referenced, attached),
    };

    format!(
        "System:\n\
Caravan is a local terminal coding assistant inspired by Claude Code. \
It helps understand, edit, and reason about the current project. \
It operates inside a local workspace. \
It must not claim it changed files unless a tool or command actually changed them. \
When file or command execution is required it explains the next concrete step.\n\n\
Project Memory:\n{}\n\n\
Conversation:\n{}\n\n\
Current User:\n{}\n\n\
Workspace Context:\n{}\n\n\
Operating Rules:\n\
- Treat normal text as the user's task.\n\
- Do not invent file contents.\n\
- Do not claim tool execution happened unless present in the event trace.\n\
- Ask for explicit user action before any mutation.\n\
- Automatic write or shell execution is not available in this baseline.\n\
- Experimental harness commands may exist but are not part of the default agent contract.\n\
- Use the provided read-only tools when workspace inspection, file discovery, or text search is necessary; they can inspect whole files or a bounded line range (offset and limit).\n\
- You may request up to two read-only workspace tools per turn, one per response.\n\
- Do not invent tool results.\n\
- After receiving the second tool result, or earlier if sufficient, answer without requesting another tool.\n\
- Only read-only workspace tools are available in this baseline.\n\
- When using Workspace Evidence from tools or attached Workspace Context, cite file paths and line numbers when available.\n\
- For file snippets, cite references like `path:line` or `path:line-line`.\n\
- For search results, cite the reported `path:line` matches.\n\
- For directory or glob results, cite the relevant path.\n\
- Do not cite files, lines, or contents that were not present in Workspace Evidence or Project Memory.\n\
- If the available evidence is insufficient, say what needs to be inspected next.\n\
- Treat the attached Workspace Context as Workspace Evidence.\n\n\
Output:\n\
Respond to the current user message.",
        memory_content, conversation, current_user_message, workspace_context
    )
}

/// Compiles the prompt for `message` with no prior conversation context.
///
/// This is the empty-history case of `compile_prompt_with_context`, delegating
/// to it so there is one prompt-template source of truth.
pub fn compile_prompt(message: &str) -> String {
    compile_prompt_with_context(message, &[], None, None, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::EventSeq;

    fn msg(role: TranscriptRole, content: &str, seq: u64) -> TranscriptMessage {
        TranscriptMessage {
            role,
            content: content.to_string(),
            seq: EventSeq(seq),
        }
    }

    /// Builds a `WorkspaceReferences` with a single resolved file item, using
    /// the T-1 public API directly (no filesystem I/O needed for these tests).
    fn referenced_context_with_file(raw: &str, content: &str) -> WorkspaceReferences {
        use crate::workspace_reference::{
            ResolvedWorkspaceReference, WorkspaceReference, WorkspaceReferenceKind,
        };

        WorkspaceReferences {
            items: vec![ResolvedWorkspaceReference {
                reference: WorkspaceReference {
                    raw: raw.to_string(),
                    path: raw.to_string(),
                    range: None,
                },
                kind: WorkspaceReferenceKind::File,
                content: content.to_string(),
                truncated: false,
            }],
            omitted: 0,
        }
    }

    #[test]
    fn compile_prompt_exact_template() {
        let missing_memory = ProjectMemory::missing().content;
        let expected = format!(
            "System:\n\
Caravan is a local terminal coding assistant inspired by Claude Code. \
It helps understand, edit, and reason about the current project. \
It operates inside a local workspace. \
It must not claim it changed files unless a tool or command actually changed them. \
When file or command execution is required it explains the next concrete step.\n\n\
Project Memory:\n{}\n\n\
Conversation:\nNo prior conversation context.\n\n\
Current User:\nhello\n\n\
Workspace Context:\nNo external tool context is attached.\n\n\
Operating Rules:\n\
- Treat normal text as the user's task.\n\
- Do not invent file contents.\n\
- Do not claim tool execution happened unless present in the event trace.\n\
- Ask for explicit user action before any mutation.\n\
- Automatic write or shell execution is not available in this baseline.\n\
- Experimental harness commands may exist but are not part of the default agent contract.\n\
- Use the provided read-only tools when workspace inspection, file discovery, or text search is necessary; they can inspect whole files or a bounded line range (offset and limit).\n\
- You may request up to two read-only workspace tools per turn, one per response.\n\
- Do not invent tool results.\n\
- After receiving the second tool result, or earlier if sufficient, answer without requesting another tool.\n\
- Only read-only workspace tools are available in this baseline.\n\
- When using Workspace Evidence from tools or attached Workspace Context, cite file paths and line numbers when available.\n\
- For file snippets, cite references like `path:line` or `path:line-line`.\n\
- For search results, cite the reported `path:line` matches.\n\
- For directory or glob results, cite the relevant path.\n\
- Do not cite files, lines, or contents that were not present in Workspace Evidence or Project Memory.\n\
- If the available evidence is insufficient, say what needs to be inspected next.\n\
- Treat the attached Workspace Context as Workspace Evidence.\n\n\
Output:\n\
Respond to the current user message.",
            missing_memory
        );
        assert_eq!(compile_prompt("hello"), expected);
    }

    #[test]
    fn compile_prompt_contains_required_sections() {
        let result = compile_prompt("hello");
        assert!(result.contains("System:"));
        assert!(result.contains("Project Memory:"));
        assert!(result.contains("Conversation:"));
        assert!(result.contains("Current User:"));
        assert!(result.contains("Workspace Context:"));
        assert!(result.contains("Operating Rules:"));
        assert!(result.contains("Output:"));
        assert!(result.contains("hello"));

        // Verify section order.
        let system_pos = result.find("System:").expect("System: must exist");
        let memory_pos = result
            .find("Project Memory:")
            .expect("Project Memory: must exist");
        let conv_pos = result
            .find("Conversation:")
            .expect("Conversation: must exist");
        let user_pos = result
            .find("Current User:")
            .expect("Current User: must exist");
        let ws_pos = result
            .find("Workspace Context:")
            .expect("Workspace Context: must exist");
        let rules_pos = result
            .find("Operating Rules:")
            .expect("Operating Rules: must exist");
        let output_pos = result.find("Output:").expect("Output: must exist");
        assert!(system_pos < memory_pos);
        assert!(memory_pos < conv_pos);
        assert!(conv_pos < user_pos);
        assert!(user_pos < ws_pos);
        assert!(ws_pos < rules_pos);
        assert!(rules_pos < output_pos);

        // Harness protocol must be absent.
        let absent_1 = ["CARAVAN", "_TOOL_REQUEST"].concat();
        assert!(!result.contains(&absent_1));
        let absent_2 = ["Available", " Tools:"].concat();
        assert!(!result.contains(&absent_2));
    }

    #[test]
    fn compile_prompt_delegates_to_empty_history() {
        assert_eq!(
            compile_prompt("hello"),
            compile_prompt_with_context("hello", &[], None, None, None)
        );
    }

    #[test]
    fn compile_prompt_with_context_empty_history_has_no_prior_marker() {
        let result = compile_prompt_with_context("hi", &[], None, None, None);
        assert!(result.contains("Conversation:"));
        assert!(result.contains("No prior conversation context."));
    }

    #[test]
    fn compile_prompt_with_context_renders_prior_user_and_assistant() {
        let history = vec![
            msg(TranscriptRole::User, "hi", 1),
            msg(TranscriptRole::Assistant, "hello back", 2),
        ];
        let result = compile_prompt_with_context("next", &history, None, None, None);
        assert!(result.contains("User: hi"));
        assert!(result.contains("Assistant: hello back"));
    }

    #[test]
    fn compile_prompt_with_context_current_in_current_user_only() {
        // History does NOT contain the current message; the compiler renders the
        // provided history verbatim and places the current message under
        // Current User. (Excluding the real current message is the runner's job.)
        // Use a unique token that won't collide with template prose.
        let history = vec![
            msg(TranscriptRole::User, "earlier", 1),
            msg(TranscriptRole::Assistant, "earlier reply", 2),
        ];
        let unique_msg = "xUNIQUE_ACTIVE_MSG_x";
        let result = compile_prompt_with_context(unique_msg, &history, None, None, None);
        assert!(result.contains(&format!("Current User:\n{}", unique_msg)));

        // Everything before the Current User: section is conversation context.
        let conversation = result
            .split("\n\nCurrent User:")
            .next()
            .expect("conversation section should exist");
        assert!(conversation.contains("User: earlier"));
        assert!(conversation.contains("Assistant: earlier reply"));
        assert!(!conversation.contains(unique_msg));
    }

    #[test]
    fn compile_prompt_with_context_caps_history_to_six() {
        let history: Vec<TranscriptMessage> = (0..8)
            .map(|i| {
                let role = if i % 2 == 0 {
                    TranscriptRole::User
                } else {
                    TranscriptRole::Assistant
                };
                msg(role, &format!("m{i}"), i + 1)
            })
            .collect();
        let result = compile_prompt_with_context("now", &history, None, None, None);
        // Only the last 6 (m2..m7) are rendered; the oldest two are dropped.
        assert!(!result.contains("m0"));
        assert!(!result.contains("m1"));
        assert!(result.contains("m2"));
        assert!(result.contains("m7"));
    }

    #[test]
    fn compile_prompt_with_context_some_manual_tool_context_renders_in_context_section() {
        use crate::manual_context::ManualToolContext;

        let ctx = ManualToolContext::from_read_file("notes.txt", "file body content");
        let compiled = compile_prompt_with_context("tell me about it", &[], None, Some(&ctx), None);

        // Positive assertions: required labels and content are present.
        assert!(compiled.contains("Attached Workspace Context:"));
        assert!(compiled.contains(
            "Source:\n  tool=read_file path=\"notes.txt\" risk=read_only truncated=false"
        ));
        assert!(compiled.contains("Content:"));
        assert!(compiled.contains("file body content"));

        // All of the above must appear after the Workspace Context: marker.
        let context_pos = compiled
            .find("Workspace Context:")
            .expect("Workspace Context: section must exist");
        let manual_pos = compiled
            .find("Attached Workspace Context:")
            .expect("Attached Workspace Context: must be present");
        let source_pos = compiled.find("Source:").expect("Source: must be present");
        let content_label_pos = compiled.find("Content:").expect("Content: must be present");
        let content_body_pos = compiled
            .find("file body content")
            .expect("bounded content must be present");
        assert!(
            manual_pos > context_pos,
            "Attached Workspace Context: must appear after Workspace Context:"
        );
        assert!(
            source_pos > context_pos,
            "Source: must appear after Workspace Context:"
        );
        assert!(
            content_label_pos > context_pos,
            "Content: label must appear after Workspace Context:"
        );
        assert!(
            content_body_pos > context_pos,
            "bounded content must appear after Workspace Context:"
        );

        // The bounded content must NOT appear in the Conversation or Current User sections.
        let before_context = compiled
            .split("\n\nWorkspace Context:")
            .next()
            .expect("Workspace Context: section must exist");
        assert!(
            !before_context.contains("file body content"),
            "bounded content must not appear before the Workspace Context: section"
        );

        // Runtime guard: the rendered prompt must not contain a byte count.
        let forbidden = format!("{}=", "bytes");
        assert!(
            !compiled.contains(&forbidden),
            "prompt must not contain a byte count (found '{forbidden}')"
        );
    }

    #[test]
    fn compile_prompt_with_context_none_manual_tool_context_uses_fallback_literal() {
        let result = compile_prompt_with_context("hello", &[], None, None, None);
        assert!(result.contains("No external tool context is attached."));
        assert!(!result.contains("Attached Workspace Context:"));
    }

    #[test]
    fn compile_prompt_with_context_none_has_project_memory_fallback() {
        let result = compile_prompt_with_context("hello", &[], None, None, None);
        assert!(result.contains("Project Memory:"));
        assert!(result.contains("No CLAUDE.md project memory found."));
        assert!(!result.contains("Attached Workspace Context:"));
    }

    #[test]
    fn compile_prompt_with_context_some_project_memory_renders_content() {
        let pm = ProjectMemory {
            source: crate::project_memory::ProjectMemorySource::File {
                path: "CLAUDE.md".to_string(),
            },
            content: "# My Project\nBuild with cargo.".to_string(),
            truncated: false,
        };
        let result = compile_prompt_with_context("hello", &[], None, None, Some(&pm));
        assert!(result.contains("Project Memory:"));
        assert!(result.contains("# My Project"));
        assert!(result.contains("Build with cargo."));
        // Fallback must not appear when real memory is provided.
        assert!(!result.contains("No CLAUDE.md project memory found."));
    }

    #[test]
    fn compile_prompt_with_context_some_has_manual_and_workspace_context_section() {
        use crate::manual_context::ManualToolContext;

        let ctx = ManualToolContext::from_read_file("notes.txt", "attached file body");
        let compiled = compile_prompt_with_context("tell me", &[], None, Some(&ctx), None);

        assert!(compiled.contains("Attached Workspace Context:"));
        assert!(compiled.contains("Workspace Context:"));

        // The Workspace Context section must contain the attached file content.
        let ws_start = compiled
            .find("\n\nWorkspace Context:")
            .expect("\n\nWorkspace Context: must exist");
        let output_start = compiled
            .find("\n\nOutput:")
            .expect("\n\nOutput: must exist");
        let ws_slice = &compiled[ws_start..output_start];
        assert!(
            ws_slice.contains("attached file body"),
            "Workspace Context section must contain attached file content"
        );
    }

    #[test]
    fn compile_prompt_with_context_some_referenced_context_renders_without_manual_context() {
        let refs = referenced_context_with_file("notes.txt", "referenced file content");
        let compiled = compile_prompt_with_context("tell me", &[], Some(&refs), None, None);

        assert!(compiled.contains("Referenced Workspace Context:"));
        assert!(!compiled.contains("Attached Workspace Context:"));
        assert!(!compiled.contains("No external tool context is attached."));
    }

    #[test]
    fn compile_prompt_with_context_some_referenced_and_manual_orders_referenced_first() {
        use crate::manual_context::ManualToolContext;

        let refs = referenced_context_with_file("notes.txt", "referenced file content");
        let ctx = ManualToolContext::from_read_file("attached.txt", "attached file content");
        let compiled = compile_prompt_with_context("tell me", &[], Some(&refs), Some(&ctx), None);

        let ws_pos = compiled
            .find("Workspace Context:")
            .expect("Workspace Context: must exist");
        let referenced_pos = compiled
            .find("Referenced Workspace Context:")
            .expect("Referenced Workspace Context: must exist");
        let attached_pos = compiled
            .find("Attached Workspace Context:")
            .expect("Attached Workspace Context: must exist");

        assert!(ws_pos < referenced_pos);
        assert!(referenced_pos < attached_pos);
    }

    #[test]
    fn compile_prompt_with_context_referenced_content_confined_to_workspace_context_section() {
        let sentinel = "xUNIQUE_REFERENCED_SENTINEL_x";
        let refs = referenced_context_with_file("notes.txt", sentinel);
        let compiled = compile_prompt_with_context("tell me", &[], Some(&refs), None, None);

        let ws_pos = compiled
            .find("Workspace Context:")
            .expect("Workspace Context: must exist");
        let (before_ws, from_ws) = compiled.split_at(ws_pos);

        assert!(from_ws.contains(sentinel));
        assert!(!before_ws.contains(sentinel));

        // Also confirm it does not leak into Current User: or Conversation:
        // specifically, in case a future template reorder moves those
        // sections after Workspace Context:.
        let current_user_slice = compiled
            .split("\n\nCurrent User:\n")
            .nth(1)
            .and_then(|rest| rest.split("\n\nWorkspace Context:").next())
            .expect("Current User: section must exist");
        assert!(!current_user_slice.contains(sentinel));

        let conversation_slice = compiled
            .split("\n\nConversation:\n")
            .nth(1)
            .and_then(|rest| rest.split("\n\nCurrent User:").next())
            .expect("Conversation: section must exist");
        assert!(!conversation_slice.contains(sentinel));
    }

    #[test]
    fn compile_prompt_with_context_none_referenced_and_manual_uses_fallback_literal() {
        let result = compile_prompt_with_context("hello", &[], None, None, None);
        assert!(result.contains("No external tool context is attached."));
        assert!(!result.contains("Referenced Workspace Context:"));
        assert!(!result.contains("Attached Workspace Context:"));
    }

    #[test]
    fn compile_prompt_with_context_some_empty_referenced_context_equals_none() {
        let empty_refs = WorkspaceReferences {
            items: Vec::new(),
            omitted: 0,
        };

        let with_empty = compile_prompt_with_context("hello", &[], Some(&empty_refs), None, None);
        let with_none = compile_prompt_with_context("hello", &[], None, None, None);

        assert_eq!(with_empty, with_none);
    }
}
