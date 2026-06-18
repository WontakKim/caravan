use kernel::events::{AppEvent, EventKind};
use ratatui::{
    layout::Rect,
    widgets::{Block, Borders, Paragraph, Wrap},
};

/// Returns the Inspector panel text for the given selected event.
///
/// For `None`, returns a placeholder. For known event kinds
/// (`PromptCompile`, `AssistantMessage`, `ModelError`, `ModelUsage`,
/// `ModelOutputChunk`), renders the detail under a readable label.
/// Every other event kind uses the standard `seq:`/`kind:`/`message:` format.
fn inspector_text(selected: Option<&AppEvent>) -> String {
    match selected {
        None => "No event selected".to_string(),
        Some(ev) => {
            let labeled = |label: &str| {
                format!(
                    "seq: {}\nkind: {}\n\n{}:\n{}",
                    ev.seq,
                    ev.kind.name(),
                    label,
                    ev.detail
                )
            };
            match ev.kind {
                EventKind::PromptCompile => labeled("Prompt preview"),
                EventKind::AssistantMessage => labeled("Assistant Message"),
                EventKind::ModelError => labeled("Error"),
                EventKind::ModelUsage => labeled("Usage"),
                EventKind::ModelOutputChunk => labeled("Chunk"),
                EventKind::ToolCall => labeled("Tool Call"),
                EventKind::ToolResult => labeled("Tool Result"),
                EventKind::ToolError => labeled("Tool Error"),
                EventKind::ToolPolicy => labeled("Tool Policy"),
                EventKind::ToolContextAttach => labeled("Tool Context Attach"),
                EventKind::ToolContextClear => labeled("Tool Context Clear"),
                EventKind::ModelToolRequest => labeled("Model Tool Request"),
                _ => format!(
                    "seq: {}\nkind: {}\nmessage: {}",
                    ev.seq,
                    ev.kind.name(),
                    ev.detail
                ),
            }
        }
    }
}

pub(super) fn render(frame: &mut ratatui::Frame, app: &crate::app::App, area: Rect) {
    let text = inspector_text(app.selected_event.and_then(|i| app.event_log.get(i)));
    let inspector = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL).title("Inspector"))
        .wrap(Wrap { trim: false })
        .scroll((app.inspector_scroll, 0));
    frame.render_widget(inspector, area);
}

#[cfg(test)]
mod tests {
    use kernel::events::{AppEvent, EventKind, EventSeq};

    use super::inspector_text;

    #[test]
    fn inspector_text_none_returns_placeholder() {
        assert_eq!(inspector_text(None), "No event selected");
    }

    #[test]
    fn inspector_text_prompt_compiled_contains_prompt_preview_label() {
        let ev = AppEvent {
            seq: EventSeq(5),
            kind: EventKind::PromptCompile,
            detail: "System: You are helpful.\nUser: Hello".to_string(),
        };
        let result = inspector_text(Some(&ev));
        assert!(
            result.contains("Prompt preview"),
            "should contain 'Prompt preview'"
        );
        // Section header must begin its own line (not glued to the label).
        assert!(
            result.contains("\nSystem:"),
            "System: should start its own line"
        );
    }

    #[test]
    fn inspector_text_non_prompt_compiled_uses_message_format() {
        let ev = AppEvent {
            seq: EventSeq(3),
            kind: EventKind::AppStart,
            detail: "Caravan started.".to_string(),
        };
        let result = inspector_text(Some(&ev));
        assert!(
            result.contains("message:"),
            "non-PromptCompile events should use message: label"
        );
        assert!(result.contains("seq:"), "should include seq:");
        assert!(result.contains("kind:"), "should include kind:");
    }

    #[test]
    fn inspector_text_assistant_message_uses_assistant_message_label() {
        let ev = AppEvent {
            seq: EventSeq(10),
            kind: EventKind::AssistantMessage,
            detail: "Sure, I can help with that.".to_string(),
        };
        let result = inspector_text(Some(&ev));
        assert!(
            result.contains("Assistant Message:"),
            "AssistantMessage events should use 'Assistant Message:' label"
        );
        assert!(
            result.contains("Sure, I can help with that."),
            "result should contain raw detail"
        );
    }

    #[test]
    fn inspector_text_model_error_uses_error_label() {
        let ev = AppEvent {
            seq: EventSeq(7),
            kind: EventKind::ModelError,
            detail: "Rate limit exceeded.".to_string(),
        };
        let result = inspector_text(Some(&ev));
        assert!(
            result.contains("Error:"),
            "ModelError events should use 'Error:' label"
        );
    }

    #[test]
    fn inspector_text_model_usage_uses_usage_label() {
        let ev = AppEvent {
            seq: EventSeq(8),
            kind: EventKind::ModelUsage,
            detail: "input=100 output=50".to_string(),
        };
        let result = inspector_text(Some(&ev));
        assert!(
            result.contains("Usage:"),
            "ModelUsage events should use 'Usage:' label"
        );
    }

    #[test]
    fn inspector_text_model_output_chunk_uses_chunk_label() {
        let ev = AppEvent {
            seq: EventSeq(9),
            kind: EventKind::ModelOutputChunk,
            detail: "Hello, ".to_string(),
        };
        let result = inspector_text(Some(&ev));
        assert!(
            result.contains("Chunk:"),
            "ModelOutputChunk events should use 'Chunk:' label"
        );
    }

    #[test]
    fn inspector_text_tool_call_uses_tool_call_label() {
        let ev = AppEvent {
            seq: EventSeq(11),
            kind: EventKind::ToolCall,
            detail: "tool=read_file path=\"secret.txt\"".to_string(),
        };
        let result = inspector_text(Some(&ev));
        assert!(
            result.contains("Tool Call:"),
            "ToolCall events should use 'Tool Call:' label"
        );
    }

    #[test]
    fn inspector_text_tool_result_uses_tool_result_label() {
        let ev = AppEvent {
            seq: EventSeq(12),
            kind: EventKind::ToolResult,
            detail: "tool=read_file entries=3 bytes=512".to_string(),
        };
        let result = inspector_text(Some(&ev));
        assert!(
            result.contains("Tool Result:"),
            "ToolResult events should use 'Tool Result:' label"
        );
    }

    #[test]
    fn inspector_text_tool_error_uses_tool_error_label() {
        let ev = AppEvent {
            seq: EventSeq(13),
            kind: EventKind::ToolError,
            detail: "tool=read_file error=permission denied".to_string(),
        };
        let result = inspector_text(Some(&ev));
        assert!(
            result.contains("Tool Error:"),
            "ToolError events should use 'Tool Error:' label"
        );
    }

    #[test]
    fn inspector_text_tool_policy_uses_tool_policy_label() {
        let ev = AppEvent {
            seq: EventSeq(18),
            kind: EventKind::ToolPolicy,
            detail: "allow".to_string(),
        };
        let result = inspector_text(Some(&ev));
        assert!(
            result.contains("Tool Policy:"),
            "ToolPolicy events should use 'Tool Policy:' label"
        );
    }

    #[test]
    fn inspector_text_tool_context_attach_uses_label() {
        let ev = AppEvent {
            seq: EventSeq(15),
            kind: EventKind::ToolContextAttach,
            detail: "source=tool=read_file path=\"README.md\" bytes=42 truncated=false".to_string(),
        };
        let result = inspector_text(Some(&ev));
        assert!(
            result.contains("Tool Context Attach:"),
            "ToolContextAttach events should use 'Tool Context Attach:' label"
        );
        assert!(
            result.contains("source="),
            "result should contain the attach summary"
        );
        assert!(
            !result.contains("raw file body"),
            "result must not expose raw file body content"
        );
    }

    #[test]
    fn inspector_text_model_tool_request_uses_model_tool_request_label() {
        let ev = AppEvent {
            seq: EventSeq(17),
            kind: EventKind::ModelToolRequest,
            detail: "detected CARAVAN_TOOL_REQUEST block".to_string(),
        };
        let result = inspector_text(Some(&ev));
        assert!(
            result.contains("Model Tool Request:"),
            "ModelToolRequest events should use 'Model Tool Request:' label"
        );
        assert!(
            result.contains("detected CARAVAN_TOOL_REQUEST block"),
            "result should contain the event detail"
        );
    }

    #[test]
    fn inspector_text_tool_context_clear_uses_label() {
        let ev = AppEvent {
            seq: EventSeq(16),
            kind: EventKind::ToolContextClear,
            detail: "Tool context cleared".to_string(),
        };
        let result = inspector_text(Some(&ev));
        assert!(
            result.contains("Tool Context Clear:"),
            "ToolContextClear events should use 'Tool Context Clear:' label"
        );
    }

    #[test]
    fn inspector_text_tool_result_does_not_expose_full_file_content() {
        let ev = AppEvent {
            seq: EventSeq(14),
            kind: EventKind::ToolResult,
            detail: "tool=read_file path=\"secret.txt\" bytes=1234".to_string(),
        };
        let result = inspector_text(Some(&ev));
        assert!(
            result.contains("bytes=1234"),
            "rendered output should contain the summary substring from ev.detail"
        );
        assert!(
            !result.contains("TOP SECRET FILE BODY"),
            "rendered output must not contain full file content sentinel"
        );
    }
}
