use color_eyre::Result;
use crossterm::event::KeyEvent;
use ratatui::prelude::Rect;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{debug, info};

use crate::{
    action::Action,
    components::{Component, fps::FpsCounter, home::Home},
    config::Config,
    terminal::{Terminal, events::TermEvent},
};

pub struct App {
    config: Config,
    tick_rate: f64,
    frame_rate: f64,
    ui_components: Vec<Box<dyn Component>>,
    should_quit: bool,
    mode: Mode,
    last_tick_key_events: Vec<KeyEvent>,
    action_sender: mpsc::UnboundedSender<Action>,
    action_receiver: mpsc::UnboundedReceiver<Action>,
}

#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Mode {
    #[default]
    Home,
}

impl App {
    pub fn new(tick_rate: f64, frame_rate: f64) -> Result<Self> {
        let (action_sender, action_receiver) = mpsc::unbounded_channel();
        Ok(Self {
            tick_rate,
            frame_rate,
            ui_components: vec![Box::new(Home::new()), Box::new(FpsCounter::default())],
            should_quit: false,
            config: Config::new()?,
            mode: Mode::Home,
            last_tick_key_events: Vec::new(),
            action_sender,
            action_receiver,
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        let mut terminal = Terminal::new(self.tick_rate, self.frame_rate)?;
        terminal.enter()?;

        for component in self.ui_components.iter_mut() {
            component.register_action_handler(self.action_sender.clone())?;
            component.register_config_handler(self.config.clone())?;
            component.init(terminal.size()?)?;
        }

        loop {
            self.handle_terminal_events(&mut terminal).await?;
            self.handle_actions(&mut terminal)?;
            if self.should_quit {
                terminal.stop()?;
                break;
            }
        }
        terminal.exit()?;
        Ok(())
    }

    async fn handle_terminal_events(&mut self, terminal: &mut Terminal) -> Result<()> {
        let Some(event) = terminal.next_event().await else {
            return Ok(());
        };
        let action_sender = self.action_sender.clone();
        match event {
            TermEvent::Quit => action_sender.send(Action::Quit)?,
            TermEvent::Tick => action_sender.send(Action::Tick)?,
            TermEvent::Render => action_sender.send(Action::Render)?,
            TermEvent::Resize(x, y) => action_sender.send(Action::Resize(x, y))?,
            TermEvent::Key(key) => self.handle_key_event(key)?,
            _ => {}
        }
        for component in self.ui_components.iter_mut() {
            if let Some(action) = component.handle_events(Some(event.clone()))? {
                action_sender.send(action)?;
            }
        }
        Ok(())
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> Result<()> {
        let action_sender = self.action_sender.clone();
        let Some(keymap) = self.config.keybindings.get(&self.mode) else {
            return Ok(());
        };
        match keymap.get(&vec![key]) {
            Some(action) => {
                info!("Got action: {action:?}");
                action_sender.send(action.clone())?;
            }
            _ => {
                // If the key was not handled as a single key action,
                // then consider it for multi-key combinations.
                self.last_tick_key_events.push(key);

                // Check for multi-key combinations
                if let Some(action) = keymap.get(&self.last_tick_key_events) {
                    info!("Got action: {action:?}");
                    action_sender.send(action.clone())?;
                }
            }
        }
        Ok(())
    }

    fn handle_actions(&mut self, tui: &mut Terminal) -> Result<()> {
        while let Ok(action) = self.action_receiver.try_recv() {
            if action != Action::Tick && action != Action::Render {
                debug!("{action:?}");
            }
            match action {
                Action::Tick => {
                    self.last_tick_key_events.drain(..);
                }
                Action::Quit => self.should_quit = true,
                Action::ClearScreen => tui.terminal.clear()?,
                Action::Resize(w, h) => self.handle_resize(tui, w, h)?,
                Action::Render => self.render(tui)?,
                _ => {}
            }
            for component in self.ui_components.iter_mut() {
                if let Some(action) = component.update(action.clone())? {
                    self.action_sender.send(action)?
                };
            }
        }
        Ok(())
    }

    fn handle_resize(&mut self, tui: &mut Terminal, w: u16, h: u16) -> Result<()> {
        tui.resize(Rect::new(0, 0, w, h))?;
        self.render(tui)?;
        Ok(())
    }

    fn render(&mut self, tui: &mut Terminal) -> Result<()> {
        tui.draw(|frame| {
            for component in self.ui_components.iter_mut() {
                if let Err(err) = component.draw(frame, frame.area()) {
                    let _ = self
                        .action_sender
                        .send(Action::Error(format!("Failed to draw: {:?}", err)));
                }
            }
        })?;
        Ok(())
    }
}
