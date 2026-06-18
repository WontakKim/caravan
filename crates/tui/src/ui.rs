use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

mod inspector;
mod prompt_bar;

/// Returns the header text, appending `| Context: pending` when
/// `app.pending_manual_tool_context` is `Some` and `| Context: none` otherwise,
/// and appending `| Request: pending` when `app.pending_model_tool_request` is
/// `Some` and `| Request: none` otherwise.
/// The `Context` indicator reflects ONLY `pending_manual_tool_context`;
/// `last_tool_output_candidate` alone (with pending still `None`) yields `| Context: none`.
fn header_text(app: &crate::app::App) -> String {
    let context_label = if app.pending_manual_tool_context.is_some() {
        "pending"
    } else {
        "none"
    };
    let request_label = if app.pending_model_tool_request.is_some() {
        "pending"
    } else {
        "none"
    };
    format!(
        "Caravan | TUI Shell | Status: Ready | Context: {} | Request: {}",
        context_label, request_label
    )
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
    let header = Paragraph::new(header_text(app))
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

    inspector::render(frame, app, inspector_area);

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

    prompt_bar::render(frame, app, cmd_area);
}

#[cfg(test)]
mod tests {
    use super::{header_text, log_skip};
    use crate::app::App;

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
    fn header_text_no_pending_shows_none() {
        let app = App::new();
        assert_eq!(
            header_text(&app),
            "Caravan | TUI Shell | Status: Ready | Context: none | Request: none"
        );
    }

    #[test]
    fn header_text_with_pending_shows_pending() {
        let mut app = App::new();
        app.pending_manual_tool_context = Some(
            kernel::manual_context::ManualToolContext::from_read_file("f.txt", "x"),
        );
        assert_eq!(
            header_text(&app),
            "Caravan | TUI Shell | Status: Ready | Context: pending | Request: none"
        );
    }

    #[test]
    fn header_text_last_candidate_only_stays_none() {
        let mut app = App::new();
        app.last_tool_output_candidate = Some(
            kernel::manual_context::ManualToolContext::from_read_file("f.txt", "x"),
        );
        // pending_manual_tool_context is still None, so header must show "none"
        let result = header_text(&app);
        assert_eq!(
            result,
            "Caravan | TUI Shell | Status: Ready | Context: none | Request: none"
        );
        assert!(
            !result.contains("Context: pending"),
            "result must not contain 'Context: pending' when only last_tool_output_candidate is set"
        );
    }

    #[test]
    fn header_text_with_model_tool_request_shows_request_pending() {
        use kernel::model_tool_request::{ModelToolRequest, ModelToolRequestKind};

        let mut app = App::new();
        app.pending_model_tool_request = Some(ModelToolRequest {
            kind: ModelToolRequestKind::ReadFile,
            path: "README.md".to_string(),
        });
        assert_eq!(
            header_text(&app),
            "Caravan | TUI Shell | Status: Ready | Context: none | Request: pending"
        );
    }
}
