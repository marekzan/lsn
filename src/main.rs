mod app;
mod node;
mod ui;

use std::io;

use app::App;
use color_eyre::Result;
use log::info;
use node::{Node, NodeKind};
use ratatui::crossterm::{
    cursor::{MoveUp, Show},
    execute,
    terminal::{Clear, ClearType, disable_raw_mode},
};

fn main() -> Result<()> {
    #[cfg(debug_assertions)]
    init_debug_logger();

    color_eyre::install()?;

    let tui_height = 50;

    let mut terminal = ratatui::init_with_options(ratatui::TerminalOptions {
        viewport: ratatui::Viewport::Inline(tui_height),
    });

    terminal.clear()?;

    let app_result = App::new().run(&mut terminal);
    // ratatui::restore();
    terminal.clear()?;

    // 1. Disable raw mode to restore normal keyboard input
    disable_raw_mode()?;

    // 2. Execute commands to clean up the terminal
    execute!(
        io::stdout(),
        // Make the cursor visible again
        Show,
        // Clear the entire screen
        MoveUp(tui_height),
        Clear(ClearType::FromCursorDown)
    )?;
    app_result
}

fn init_debug_logger() {
    use simplelog::{Config, WriteLogger};
    use std::fs::File;

    log_panics::init();

    WriteLogger::init(
        log::LevelFilter::Debug,
        Config::default(),
        File::create("debug.log").unwrap(),
    )
    .unwrap();

    info!("debug logger initialized")
}
