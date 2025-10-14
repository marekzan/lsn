pub mod events;

use std::{
    io::{Stdout, stdout},
    ops::{Deref, DerefMut},
    time::Duration,
};

use color_eyre::Result;
use crossterm::{
    cursor,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::backend::CrosstermBackend;
use tokio::{
    sync::mpsc::{self, UnboundedReceiver, UnboundedSender},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;
use tracing::error;

use crate::terminal::events::TermEvent;

pub struct Terminal {
    pub terminal: ratatui::Terminal<CrosstermBackend<Stdout>>,
    pub task: JoinHandle<()>,
    pub cancellation_token: CancellationToken,
    pub term_event_receiver: UnboundedReceiver<TermEvent>,
    pub term_event_sender: UnboundedSender<TermEvent>,
    pub frame_rate: f64,
    pub tick_rate: f64,
    pub fullscreen: bool,
}

impl Terminal {
    pub fn new(
        tick_rate: f64,
        frame_rate: f64,
        fullscreen: bool,
        inline_height: u16,
    ) -> Result<Self> {
        let (sender, receiver) = mpsc::unbounded_channel();

        Ok(Self {
            terminal: configure_terminal(fullscreen, inline_height)?,
            task: tokio::spawn(async {}),
            cancellation_token: CancellationToken::new(),
            term_event_receiver: receiver,
            term_event_sender: sender,
            tick_rate,
            frame_rate,
            fullscreen,
        })
    }

    pub fn enter(&mut self) -> Result<()> {
        crossterm::terminal::enable_raw_mode()?;
        if self.fullscreen {
            crossterm::execute!(stdout(), EnterAlternateScreen, cursor::Hide)?;
        }
        self.start_event_loop();
        Ok(())
    }

    pub fn exit(&mut self) -> Result<()> {
        self.stop_event_loop()?;
        if crossterm::terminal::is_raw_mode_enabled()? {
            self.flush()?;
            if self.fullscreen {
                crossterm::execute!(stdout(), LeaveAlternateScreen, cursor::Show)?;
            }
            crossterm::terminal::disable_raw_mode()?;
        }
        Ok(())
    }

    pub fn start_event_loop(&mut self) {
        self.cancellation_token.cancel();
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

    pub async fn next_event(&mut self) -> Option<TermEvent> {
        self.term_event_receiver.recv().await
    }

    pub fn stop_event_loop(&self) -> Result<()> {
        self.cancellation_token.cancel();
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
}

impl Deref for Terminal {
    type Target = ratatui::Terminal<CrosstermBackend<Stdout>>;

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

fn configure_terminal(
    fullscreen: bool,
    inline_height: u16,
) -> Result<ratatui::Terminal<CrosstermBackend<Stdout>>> {
    if fullscreen {
        let terminal = ratatui::Terminal::new(CrosstermBackend::new(stdout()))?;
        Ok(terminal)
    } else {
        let terminal = ratatui::init_with_options(ratatui::TerminalOptions {
            viewport: ratatui::Viewport::Inline(inline_height),
        });
        Ok(terminal)
    }
}
