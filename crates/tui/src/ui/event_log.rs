use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

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

pub(super) fn render(frame: &mut ratatui::Frame, app: &crate::app::App, area: Rect) {
    let log_height = area.height.saturating_sub(2) as usize;
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
    frame.render_widget(log, area);
}

#[cfg(test)]
mod tests {
    use super::log_skip;

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
}
