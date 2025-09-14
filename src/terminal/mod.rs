pub mod events;

use std::{
    io::{Stdout, stdout},
    ops::{Deref, DerefMut},
    time::Duration,
};

use color_eyre::Result;
use crossterm::{
    cursor,
    event::{DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture},
    terminal::{EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::backend::CrosstermBackend as Backend;
use tokio::{
    sync::mpsc::{self, UnboundedReceiver, UnboundedSender},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;
use tracing::error;

use crate::terminal::events::TermEvent;

pub struct Terminal {
    pub terminal: ratatui::Terminal<Backend<Stdout>>,
    pub task: JoinHandle<()>,
    pub cancellation_token: CancellationToken,
    pub term_event_receiver: UnboundedReceiver<TermEvent>,
    pub term_event_sender: UnboundedSender<TermEvent>,
    pub frame_rate: f64,
    pub tick_rate: f64,
    pub mouse: bool,
    pub paste: bool,
}

impl Terminal {
    /// Constructs a new instance of [`Terminal`].
    pub fn new(tick_rate: f64, frame_rate: f64) -> Result<Self> {
        let (sender, receiver) = mpsc::unbounded_channel();
        Ok(Self {
            terminal: ratatui::Terminal::new(Backend::new(stdout()))?,
            task: tokio::spawn(async {}),
            cancellation_token: CancellationToken::new(),
            term_event_receiver: receiver,
            term_event_sender: sender,
            tick_rate,
            frame_rate,
            mouse: false,
            paste: false,
        })
    }

    /// Starts the terminal event loop.
    pub fn start(&mut self) {
        self.cancellation_token.cancel(); // Cancel any existing task
        self.cancellation_token = CancellationToken::new();
        let event_loop = Self::event_loop(
            self.term_event_sender.clone(),
            self.cancellation_token.clone(),
            self.tick_rate,
            self.frame_rate,
        );
        self.task = tokio::spawn(async {
            event_loop.await;
        });
    }

    /// Stops the terminal event loop.
    pub fn stop(&self) -> Result<()> {
        self.cancellation_token.cancel(); // Cancel any existing task
        let mut counter = 0;
        while !self.task.is_finished() {
            std::thread::sleep(Duration::from_millis(1));
            counter += 1;
            if counter > 50 {
                self.task.abort();
            }
            if counter > 100 {
                error!("Failed to abort task in 100 milliseconds for unknown reason");
                break;
            }
        }
        Ok(())
    }

    /// Enters the alternate screen, enables raw mode, and hides the cursor.
    pub fn enter(&mut self) -> Result<()> {
        crossterm::terminal::enable_raw_mode()?;
        crossterm::execute!(stdout(), EnterAlternateScreen, cursor::Hide)?;
        if self.mouse {
            crossterm::execute!(stdout(), EnableMouseCapture)?;
        }
        if self.paste {
            crossterm::execute!(stdout(), EnableBracketedPaste)?;
        }
        self.start();
        Ok(())
    }

    /// Exits the alternate screen, disables raw mode, and shows the cursor.
    pub fn exit(&mut self) -> Result<()> {
        self.stop()?;
        if crossterm::terminal::is_raw_mode_enabled()? {
            self.flush()?;
            if self.paste {
                crossterm::execute!(stdout(), DisableBracketedPaste)?;
            }
            if self.mouse {
                crossterm::execute!(stdout(), DisableMouseCapture)?;
            }
            crossterm::execute!(stdout(), LeaveAlternateScreen, cursor::Show)?;
            crossterm::terminal::disable_raw_mode()?;
        }
        Ok(())
    }

    /// Suspends the application.
    pub fn suspend(&mut self) -> Result<()> {
        self.exit()?;
        #[cfg(not(windows))]
        signal_hook::low_level::raise(signal_hook::consts::signal::SIGTSTP)?;
        Ok(())
    }

    /// Resumes the application.
    pub fn resume(&mut self) -> Result<()> {
        self.enter()?;
        Ok(())
    }

    /// Returns the next terminal event.
    pub async fn next_event(&mut self) -> Option<TermEvent> {
        self.term_event_receiver.recv().await
    }
}

impl Deref for Terminal {
    type Target = ratatui::Terminal<Backend<Stdout>>;

    fn deref(&self) -> &Self::Target {
        &self.terminal
    }
}

impl DerefMut for Terminal {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.terminal
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        if let Err(e) = self.exit() {
            eprintln!("Error exiting terminal: {:?}", e);
        };
    }
}
