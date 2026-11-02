use std::{io::Write, time::Instant};

use clap::Parser;
use color_eyre::{Result, eyre::WrapErr};
use crossterm::event::{self, Event, KeyEventKind};

use crate::{
    app::App,
    cli::Cli,
    pins::Pins,
    term::{Display, Session},
};

mod action;
mod app;
mod arena;
mod cli;
mod fs;
mod input;
mod jump;
mod keymap;
mod pins;
mod tab;
mod term;
mod ui;
mod view;

fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Cli::parse();

    let mut app = App::new(&args.dirs, Pins::load()).wrap_err("could not open directory")?;
    let display = if args.fullscreen {
        Display::Fullscreen
    } else {
        Display::Inline(args.height)
    };

    let mut session = Session::start(display)?;
    let result = run(&mut session, &mut app);
    session.stop()?;
    result?;

    // only now stdout is the real stdout again
    let mut stdout = std::io::stdout().lock();
    for path in &app.output {
        writeln!(stdout, "{}", path.display())?;
    }
    Ok(())
}

/// the event loop: draw, wait for a key, update. the ui is only redrawn when
/// something happened (a key, a resize or a deadline of the app).
fn run(session: &mut Session, app: &mut App) -> Result<()> {
    loop {
        session.terminal.draw(|frame| ui::draw(frame, app))?;

        // some modes want to act after a pause (jump), wait for a key only
        // until then
        if let Some(deadline) = app.deadline() {
            let timeout = deadline.saturating_duration_since(Instant::now());
            if !event::poll(timeout)? {
                app.tick(Instant::now());
                continue;
            }
        }

        match event::read()? {
            Event::Key(key) if key.kind != KeyEventKind::Release => app.handle_key(key),
            Event::Resize(..) => session.terminal.autoresize()?,
            _ => continue,
        }
        if app.should_quit {
            return Ok(());
        }
    }
}
