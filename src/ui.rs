use ratatui::{
    layout::{Constraint, Direction, Layout},
    widgets::{Block, Borders, Paragraph},
};

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

    // Main
    let main = Paragraph::new("Welcome to Caravan\nType /help to see available commands.")
        .block(Block::default().borders(Borders::ALL).title("Main"));
    frame.render_widget(main, main_area);

    // Inspector
    let inspector = Paragraph::new("No item selected")
        .block(Block::default().borders(Borders::ALL).title("Inspector"));
    frame.render_widget(inspector, inspector_area);

    // Log — render the tail of app.log that fits the panel height
    let log_height = log_area.height.saturating_sub(2) as usize;
    let skip = app.log.len().saturating_sub(log_height);
    let log_text = app.log[skip..].join("\n");
    let log = Paragraph::new(log_text).block(Block::default().borders(Borders::ALL).title("Log"));
    frame.render_widget(log, log_area);

    // Command Bar
    let cmd_text = format!("> {}", app.input);
    let cmd =
        Paragraph::new(cmd_text).block(Block::default().borders(Borders::ALL).title("Command"));
    frame.render_widget(cmd, cmd_area);

    // Place the cursor just after the "> " prompt, clamped inside the inner width.
    let inner_max_x = cmd_area.x + cmd_area.width.saturating_sub(2);
    let cursor_x = (cmd_area.x + 2 + app.input.chars().count() as u16).min(inner_max_x);
    let cursor_y = cmd_area.y + 1;
    frame.set_cursor_position((cursor_x, cursor_y));
}
