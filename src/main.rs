mod app;
mod input;
mod ui;

use std::time::Duration;

use crossterm::event::{Event, poll, read};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut app::App,
) -> std::io::Result<()> {
    loop {
        terminal.draw(|frame| ui::draw(frame, app))?;

        if poll(Duration::from_millis(50))? {
            if let Event::Key(key) = read()? {
                input::handle_key(app, key);
            }
        }

        if app.should_quit {
            break;
        }
    }
    Ok(())
}

fn main() -> std::io::Result<()> {
    // Install panic hook that restores the terminal before the default hook runs.
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = crossterm::execute!(std::io::stdout(), LeaveAlternateScreen);
        prev_hook(info);
    }));

    let mut app = app::App::new();

    // Terminal setup.
    let mut stdout = std::io::stdout();
    enable_raw_mode()?;
    crossterm::execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Run the event loop; capture the result so teardown is unconditional.
    let result = run_app(&mut terminal, &mut app);

    // Unconditional teardown on the same stdout stream.
    crossterm::execute!(std::io::stdout(), LeaveAlternateScreen)?;
    disable_raw_mode()?;

    result
}
