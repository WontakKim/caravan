use crate::manual_context::ManualToolContext;
use crate::transcript::{TranscriptMessage, TranscriptRole};

/// Number of most-recent transcript messages rendered in the prompt's
/// conversation window. This is a small fixed window — NOT long-term memory.
pub const DEFAULT_PROMPT_HISTORY_MESSAGES: usize = 6;

/// Compiles the prompt for the current user message, including a short recent
/// conversation window.
///
/// `history` is the prior conversation, already excluding the current user
/// message (see `ConversationTranscript::without_trailing_user_message`). Only
/// the last `DEFAULT_PROMPT_HISTORY_MESSAGES` messages are rendered; when that
/// window is empty the Conversation section shows `No prior conversation
/// context.`. The function renders whatever history it is given verbatim — it
/// does not de-duplicate by content.
pub fn compile_prompt_with_context(
    current_user_message: &str,
    history: &[TranscriptMessage],
    manual_tool_context: Option<&ManualToolContext>,
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

    let context = match manual_tool_context {
        Some(ctx) => format!(
            "Manual Tool Context:\nSource:\n  {}\nContent:\n{}",
            ctx.source_label(),
            ctx.content
        ),
        None => "No external tool context is available in this POC.".to_string(),
    };

    let tools_section = crate::tool::schema::ToolCatalog::readonly().render_prompt_section();

    format!(
        "System:\nYou are Caravan's local assistant.\n\nConversation:\n{}\n\nCurrent User:\n{}\n\nContext:\n{}\n\n{}\n\nOutput:\nRespond to the current user message.",
        conversation, current_user_message, context, tools_section
    )
}

/// Compiles the prompt for `message` with no prior conversation context.
///
/// This is the empty-history case of `compile_prompt_with_context`, delegating
/// to it so there is one prompt-template source of truth.
pub fn compile_prompt(message: &str) -> String {
    compile_prompt_with_context(message, &[], None)
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

    #[test]
    fn compile_prompt_exact_template() {
        let tools_section = crate::tool::schema::ToolCatalog::readonly().render_prompt_section();
        let expected = format!(
            "System:\nYou are Caravan's local assistant.\n\nConversation:\nNo prior conversation context.\n\nCurrent User:\nhello\n\nContext:\nNo external tool context is available in this POC.\n\n{}\n\nOutput:\nRespond to the current user message.",
            tools_section
        );
        assert_eq!(compile_prompt("hello"), expected);
    }

    #[test]
    fn compile_prompt_contains_required_sections() {
        let result = compile_prompt("hello");
        assert!(result.contains("System:"));
        assert!(result.contains("Conversation:"));
        assert!(result.contains("Current User:"));
        assert!(result.contains("Context:"));
        assert!(result.contains("Available Tools:"));
        assert!(result.contains("Output:"));
        assert!(result.contains("hello"));

        let context_pos = result.find("Context:").expect("Context: must exist");
        let tools_pos = result
            .find("Available Tools:")
            .expect("Available Tools: must exist");
        let output_pos = result.find("Output:").expect("Output: must exist");
        assert!(
            tools_pos > context_pos,
            "Available Tools: must appear after Context:"
        );
        assert!(
            tools_pos < output_pos,
            "Available Tools: must appear before Output:"
        );
    }

    #[test]
    fn compile_prompt_delegates_to_empty_history() {
        assert_eq!(
            compile_prompt("hello"),
            compile_prompt_with_context("hello", &[], None)
        );
    }

    #[test]
    fn compile_prompt_with_context_empty_history_has_no_prior_marker() {
        let result = compile_prompt_with_context("hi", &[], None);
        assert!(result.contains("Conversation:"));
        assert!(result.contains("No prior conversation context."));
    }

    #[test]
    fn compile_prompt_with_context_renders_prior_user_and_assistant() {
        let history = vec![
            msg(TranscriptRole::User, "hi", 1),
            msg(TranscriptRole::Assistant, "hello back", 2),
        ];
        let result = compile_prompt_with_context("next", &history, None);
        assert!(result.contains("User: hi"));
        assert!(result.contains("Assistant: hello back"));
    }

    #[test]
    fn compile_prompt_with_context_current_in_current_user_only() {
        // History does NOT contain the current message; the compiler renders the
        // provided history verbatim and places the current message under
        // Current User. (Excluding the real current message is the runner's job.)
        let history = vec![
            msg(TranscriptRole::User, "earlier", 1),
            msg(TranscriptRole::Assistant, "earlier reply", 2),
        ];
        let result = compile_prompt_with_context("current", &history, None);
        assert!(result.contains("Current User:\ncurrent"));

        let conversation = result
            .split("\n\nCurrent User:")
            .next()
            .expect("conversation section should exist");
        assert!(conversation.contains("User: earlier"));
        assert!(conversation.contains("Assistant: earlier reply"));
        assert!(!conversation.contains("current"));
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
        let result = compile_prompt_with_context("now", &history, None);
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
        let compiled = compile_prompt_with_context("tell me about it", &[], Some(&ctx));

        // Positive assertions: required labels and content are present.
        assert!(compiled.contains("Manual Tool Context:"));
        assert!(compiled.contains(
            "Source:\n  tool=read_file path=\"notes.txt\" risk=read_only truncated=false"
        ));
        assert!(compiled.contains("Content:"));
        assert!(compiled.contains("file body content"));

        // All of the above must appear after the Context: marker.
        let context_pos = compiled
            .find("Context:")
            .expect("Context: section must exist");
        let manual_pos = compiled
            .find("Manual Tool Context:")
            .expect("Manual Tool Context: must be present");
        let source_pos = compiled.find("Source:").expect("Source: must be present");
        let content_label_pos = compiled.find("Content:").expect("Content: must be present");
        let content_body_pos = compiled
            .find("file body content")
            .expect("bounded content must be present");
        assert!(
            manual_pos > context_pos,
            "Manual Tool Context: must appear after Context:"
        );
        assert!(
            source_pos > context_pos,
            "Source: must appear after Context:"
        );
        assert!(
            content_label_pos > context_pos,
            "Content: label must appear after Context:"
        );
        assert!(
            content_body_pos > context_pos,
            "bounded content must appear after Context:"
        );

        // The bounded content must NOT appear in the Conversation or Current User sections.
        let before_context = compiled
            .split("\n\nContext:")
            .next()
            .expect("Context: section must exist");
        assert!(
            !before_context.contains("file body content"),
            "bounded content must not appear before the Context: section"
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
        let result = compile_prompt_with_context("hello", &[], None);
        assert!(result.contains("No external tool context is available in this POC."));
        assert!(!result.contains("Manual Tool Context:"));
    }

    #[test]
    fn compile_prompt_with_context_none_has_available_tools_and_fallback() {
        let result = compile_prompt_with_context("hello", &[], None);
        assert!(result.contains("Available Tools:"));
        assert!(result.contains("No external tool context is available in this POC."));
    }

    #[test]
    fn compile_prompt_with_context_some_has_both_manual_and_available_tools_sections() {
        use crate::manual_context::ManualToolContext;

        let ctx = ManualToolContext::from_read_file("notes.txt", "attached file body");
        let compiled = compile_prompt_with_context("tell me", &[], Some(&ctx));

        assert!(compiled.contains("Manual Tool Context:"));
        assert!(compiled.contains("Available Tools:"));

        // The Available Tools section slice must NOT contain the attached file content.
        let tools_start = compiled
            .find("\n\nAvailable Tools:")
            .expect("\n\nAvailable Tools: must exist");
        let tools_end = compiled
            .find("\n\nOutput:")
            .expect("\n\nOutput: must exist");
        let tools_slice = &compiled[tools_start..tools_end];
        assert!(
            !tools_slice.contains("attached file body"),
            "Available Tools section must not contain attached file content"
        );
    }
}
