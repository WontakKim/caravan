use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use unicode_width::UnicodeWidthStr;

use kernel::events::{AppEvent, EventKind};

/// Terminal display width (in columns) of the prompt input. Wide characters
/// such as Hangul/CJK occupy two columns, so the cursor must advance by the
/// rendered width rather than the scalar count; otherwise it lands inside the
/// typed text. Saturates to `u16::MAX` for pathologically long input.
fn input_display_width(input: &str) -> u16 {
    u16::try_from(UnicodeWidthStr::width(input)).unwrap_or(u16::MAX)
}

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

/// Compute how many leading events the Log panel should skip: tail the newest
/// events by default, but scroll up to keep the selected event visible when it
/// is older than the tail window.
fn log_skip(total: usize, log_height: usize, selected: Option<usize>) -> usize {
    let tail_skip = total.saturating_sub(log_height);
    match selected {
        Some(sel) if sel < tail_skip => sel,
        _ => tail_skip,
    }
}

pub fn draw(frame: &mut ratatui::Frame, app: &crate::app::App) {
    let area = frame.area();

    // Vertical layout: Header | Body | Log | Command Bar
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Fill(1),
            Constraint::Fill(1),
            Constraint::Length(3),
        ])
        .split(area);

    let header_area = vertical[0];
    let body_area = vertical[1];
    let log_area = vertical[2];
    let cmd_area = vertical[3];

    // Horizontal layout: Nav | Main | Inspector
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Percentage(55),
            Constraint::Percentage(25),
        ])
        .split(body_area);

    let nav_area = horizontal[0];
    let main_area = horizontal[1];
    let inspector_area = horizontal[2];

    // Header
    let header = Paragraph::new("Caravan | TUI Shell | Status: Ready")
        .block(Block::default().borders(Borders::ALL).title("Header"));
    frame.render_widget(header, header_area);

    // Nav
    let nav = Paragraph::new("Home\nAgents\nRuns\nTools")
        .block(Block::default().borders(Borders::ALL).title("Nav"));
    frame.render_widget(nav, nav_area);

    // Main — render the tail of app.log that fits the panel height
    let main_height = main_area.height.saturating_sub(2) as usize;
    let main_skip = app.log.len().saturating_sub(main_height);
    let main_text = app.log[main_skip..].join("\n");
    let main =
        Paragraph::new(main_text).block(Block::default().borders(Borders::ALL).title("Main"));
    frame.render_widget(main, main_area);

    // Inspector — render selected event detail, or fallback
    let text = inspector_text(app.selected_event.and_then(|i| app.event_log.get(i)));
    let inspector = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL).title("Inspector"))
        .wrap(Wrap { trim: false })
        .scroll((app.inspector_scroll, 0));
    frame.render_widget(inspector, inspector_area);

    // Log — render the EventLog, tailing newest events but scrolling up to keep
    // the selected event visible and highlighted.
    let log_height = log_area.height.saturating_sub(2) as usize;
    let events = app.event_log.events();
    let skip = log_skip(events.len(), log_height, app.selected_event);
    let lines: Vec<Line> = events[skip..]
        .iter()
        .enumerate()
        .map(|(relative_i, ev)| {
            let abs_i = skip + relative_i;
            let text = format!("{} {}", ev.seq, ev.kind.name());
            let span = Span::raw(text);
            let line = Line::from(span);
            if app.selected_event == Some(abs_i) {
                line.style(Style::default().add_modifier(Modifier::REVERSED))
            } else {
                line
            }
        })
        .collect();
    let log = Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("Log"));
    frame.render_widget(log, log_area);

    // Prompt Bar
    let cmd_text = format!("> {}", app.input);
    let cmd =
        Paragraph::new(cmd_text).block(Block::default().borders(Borders::ALL).title("Prompt"));
    frame.render_widget(cmd, cmd_area);

    // Place the cursor just after the "> " prompt. The offset is 3 columns from
    // cmd_area.x: 1 for the block's left border, plus 2 for the "> " prefix.
    // Saturating arithmetic guards against extreme input lengths / tiny terminals.
    let inner_max_x = cmd_area.x.saturating_add(cmd_area.width.saturating_sub(2));
    let typed = input_display_width(&app.input);
    let cursor_x = cmd_area
        .x
        .saturating_add(3)
        .saturating_add(typed)
        .min(inner_max_x);
    let cursor_y = cmd_area.y.saturating_add(1);
    frame.set_cursor_position((cursor_x, cursor_y));
}

#[cfg(test)]
mod tests {
    use kernel::events::{AppEvent, EventKind, EventSeq};

    use super::{input_display_width, inspector_text, log_skip};

    #[test]
    fn ascii_width_equals_char_count() {
        assert_eq!(input_display_width("hello"), 5);
        assert_eq!(input_display_width(""), 0);
    }

    #[test]
    fn hangul_chars_are_two_columns_each() {
        // Each Hangul syllable renders as two terminal columns, so "한글"
        // (2 scalars) occupies 4 columns. The cursor must advance by 4, not 2.
        assert_eq!("한글".chars().count(), 2);
        assert_eq!(input_display_width("한글"), 4);
    }

    #[test]
    fn mixed_ascii_and_hangul_width() {
        // "hi한" -> h(1) + i(1) + 한(2) = 4 columns.
        assert_eq!(input_display_width("hi한"), 4);
    }

    #[test]
    fn tails_when_no_selection() {
        // 10 events, window of 4 -> show the last 4 (skip 6).
        assert_eq!(log_skip(10, 4, None), 6);
    }

    #[test]
    fn tails_when_selection_already_in_window() {
        // index 8 is within the tail window [6, 10) -> still tail.
        assert_eq!(log_skip(10, 4, Some(8)), 6);
    }

    #[test]
    fn scrolls_up_to_keep_older_selection_visible() {
        // index 2 is older than the tail window -> scroll so it's at the top.
        assert_eq!(log_skip(10, 4, Some(2)), 2);
    }

    #[test]
    fn no_scroll_when_all_events_fit() {
        assert_eq!(log_skip(3, 10, Some(0)), 0);
        assert_eq!(log_skip(3, 10, None), 0);
    }

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
