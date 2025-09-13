//! This module contains the terminal events.

use std::time::Duration;

use crossterm::event::{Event as CrosstermEvent, EventStream, KeyEvent, KeyEventKind, MouseEvent};
use futures::{FutureExt, StreamExt};
use tokio::{sync::mpsc::UnboundedSender, time::interval};
use tokio_util::sync::CancellationToken;

use serde::{Deserialize, Serialize};

use crate::terminal::Terminal;

/// Terminal events.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TermEvent {
    Init,
    Quit,
    Error,
    Closed,
    Tick,
    Render,
    FocusGained,
    FocusLost,
    Paste(String),
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize(u16, u16),
}

impl Terminal {
    pub async fn event_loop(
        event_sender: UnboundedSender<TermEvent>,
        cancellation_token: CancellationToken,
        tick_rate: f64,
        frame_rate: f64,
    ) {
        let mut event_stream = EventStream::new();
        let mut tick_interval = interval(Duration::from_secs_f64(1.0 / tick_rate));
        let mut render_interval = interval(Duration::from_secs_f64(1.0 / frame_rate));

        // send a marker event to check if the channel is open
        event_sender
            .send(TermEvent::Init)
            .expect("failed to send init event");

        loop {
            let event = tokio::select! {
                _ = cancellation_token.cancelled() => {
                    break;
                }
                _ = tick_interval.tick() => TermEvent::Tick,
                _ = render_interval.tick() => TermEvent::Render,
                crossterm_event = event_stream.next().fuse() => match crossterm_event {
                    Some(Ok(event)) => match event {
                        CrosstermEvent::Key(key) if key.kind == KeyEventKind::Press => TermEvent::Key(key),
                        CrosstermEvent::Mouse(mouse) => TermEvent::Mouse(mouse),
                        CrosstermEvent::Resize(x, y) => TermEvent::Resize(x, y),
                        CrosstermEvent::FocusLost => TermEvent::FocusLost,
                        CrosstermEvent::FocusGained => TermEvent::FocusGained,
                        CrosstermEvent::Paste(s) => TermEvent::Paste(s),
                        _ => continue,
                    }
                    Some(Err(_)) => TermEvent::Error,
                    None => break,
                },
            };
            if event_sender.send(event).is_err() {
                // the receiver has been dropped, so there's no point in continuing the loop
                break;
            }
        }
        // if the loop endet we send a cancel signal to our tasks
        cancellation_token.cancel();
    }
}
