mod app;
mod commands;
mod events;
mod input;
mod model;
mod model_config;
mod model_gateway;
mod model_openai_compatible;
mod model_openai_config;
mod model_openai_request;
mod model_openai_types;
mod model_registry;
mod model_runtime_config;
mod model_types;
mod prompt;
mod runner;
mod storage;
mod ui;

use std::time::Duration;

use crossterm::event::{Event, poll, read};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

/// Attempt all cleanup steps without short-circuiting between them.
/// Returns the first error encountered, or `Ok(())`.
fn restore_terminal() -> std::io::Result<()> {
    let show = crossterm::execute!(
        std::io::stdout(),
        crossterm::cursor::Show,
        LeaveAlternateScreen
    );
    let disable = disable_raw_mode();
    if show.is_err() { show } else { disable }
}

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

        if app.should_exit {
            break;
        }
    }
    Ok(())
}

fn main() -> std::io::Result<()> {
    // Install panic hook that restores the terminal before the default hook runs.
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::cursor::Show,
            LeaveAlternateScreen
        );
        let _ = disable_raw_mode();
        prev_hook(info);
    }));

    let runtime_config = match model_runtime_config::ModelRuntimeConfig::from_process_env() {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("caravan: invalid model runtime config: {e}");
            std::process::exit(1);
        }
    };
    let gateway = model_gateway::ModelGateway::from_runtime_config(runtime_config);

    let mut app = app::App::with_store_and_gateway(storage::EventStore::new(".caravan"), gateway);

    // Terminal setup: clean up on any early failure after raw mode is enabled.
    let mut stdout = std::io::stdout();
    enable_raw_mode()?;

    if let Err(e) = crossterm::execute!(stdout, EnterAlternateScreen) {
        let _ = restore_terminal();
        return Err(e);
    }

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = match Terminal::new(backend) {
        Ok(t) => t,
        Err(e) => {
            let _ = restore_terminal();
            return Err(e);
        }
    };

    // Run the event loop; capture the result so teardown is unconditional.
    let result = run_app(&mut terminal, &mut app);

    // Unconditional teardown: show cursor, leave alternate screen, disable raw mode.
    let restore = restore_terminal();

    // Return the run_app error first (if any), otherwise the restore error.
    if result.is_err() { result } else { restore }
}
