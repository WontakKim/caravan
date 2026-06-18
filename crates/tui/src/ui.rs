use ratatui::{
    layout::{Constraint, Direction, Layout},
    widgets::{Block, Borders, Paragraph},
};

mod event_log;
mod header;
mod inspector;
mod prompt_bar;

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
    header::render(frame, app, header_area);

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

    event_log::render(frame, app, log_area);

    prompt_bar::render(frame, app, cmd_area);
}
