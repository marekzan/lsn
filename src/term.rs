//! terminal setup for both display modes.
//!
//! inline: the ui takes `height` lines below the prompt and is cleared on
//! exit, like atuin or fzf. fullscreen: the alternate screen is used.
//!
//! the ui always talks to the tty. when stdout is redirected (`cd "$(lsn)"`)
//! fd 1 is pointed at `/dev/tty` while the ui runs and restored afterwards,
//! so the picked paths end up in the pipe and the escape codes don't.

use std::io::{self, Stdout, Write};

use crossterm::{
    cursor, execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{Terminal, TerminalOptions, Viewport, backend::CrosstermBackend};

pub type Tui = Terminal<CrosstermBackend<Stdout>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Display {
    Inline(u16),
    Fullscreen,
}

pub struct Session {
    pub terminal: Tui,
    display: Display,
    redirect: Option<StdoutRedirect>,
}

impl Session {
    pub fn start(display: Display) -> io::Result<Self> {
        let redirect = StdoutRedirect::to_tty_if_needed()?;
        terminal::enable_raw_mode()?;

        let viewport = match display {
            Display::Fullscreen => {
                execute!(io::stdout(), EnterAlternateScreen)?;
                Viewport::Fullscreen
            }
            Display::Inline(height) => {
                let (_, rows) = terminal::size()?;
                Viewport::Inline(height.clamp(3, rows.max(3)))
            }
        };
        let terminal = Terminal::with_options(
            CrosstermBackend::new(io::stdout()),
            TerminalOptions { viewport },
        )?;
        install_panic_hook(display);

        Ok(Self {
            terminal,
            display,
            redirect,
        })
    }

    /// restores the terminal; the inline viewport is wiped so no trace is left
    pub fn stop(self) -> io::Result<()> {
        let Self {
            mut terminal,
            display,
            redirect,
        } = self;
        if display != Display::Fullscreen {
            let area = terminal.get_frame().area();
            terminal.clear()?;
            terminal.set_cursor_position((0, area.y))?;
        }
        // the terminal shows the cursor on drop, that must happen while fd 1
        // still points at the tty
        terminal.show_cursor()?;
        drop(terminal);
        restore(display)?;
        io::stdout().flush()?;
        drop(redirect);
        Ok(())
    }
}

fn restore(display: Display) -> io::Result<()> {
    terminal::disable_raw_mode()?;
    if display == Display::Fullscreen {
        execute!(io::stdout(), LeaveAlternateScreen)?;
    }
    execute!(io::stdout(), cursor::Show)
}

/// leaves the terminal usable when we crash
fn install_panic_hook(display: Display) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = restore(display);
        if display != Display::Fullscreen {
            println!();
        }
        previous(info);
    }));
}

/// points fd 1 at the controlling terminal until dropped
struct StdoutRedirect {
    saved: libc::c_int,
}

impl StdoutRedirect {
    fn to_tty_if_needed() -> io::Result<Option<Self>> {
        // SAFETY: plain fd juggling, every fd we open we also close
        unsafe {
            if libc::isatty(libc::STDOUT_FILENO) == 1 {
                return Ok(None);
            }
            let tty = libc::open(c"/dev/tty".as_ptr(), libc::O_RDWR);
            if tty < 0 {
                return Err(io::Error::last_os_error());
            }
            let saved = libc::dup(libc::STDOUT_FILENO);
            if saved < 0 || libc::dup2(tty, libc::STDOUT_FILENO) < 0 {
                let err = io::Error::last_os_error();
                libc::close(tty);
                return Err(err);
            }
            libc::close(tty);
            Ok(Some(Self { saved }))
        }
    }
}

impl Drop for StdoutRedirect {
    fn drop(&mut self) {
        let _ = io::stdout().flush();
        // SAFETY: `saved` is a valid fd we dup'ed ourselves
        unsafe {
            libc::dup2(self.saved, libc::STDOUT_FILENO);
            libc::close(self.saved);
        }
    }
}
