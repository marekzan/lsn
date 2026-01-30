use color_eyre::Result;
use crossterm::event::KeyEvent;
use ratatui::prelude::Rect;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::debug;

use crate::{
    action::{Action, AppAction},
    arena::Arena,
    cli::Cli,
    components::{Component, fps::FpsCounter, home::Home},
    config::Config,
    terminal::{Terminal, events::TermEvent},
};

pub struct App {
    config: Config,
    arena: Arena<String>,
    tick_rate: f64,
    frame_rate: f64,
    fullscreen: bool,
    inline_height: u16,
    ui_components: Vec<Box<dyn Component>>,
    should_quit: bool,
    mode: Mode,
    last_tick_key_events: Vec<KeyEvent>,
    action_tx: mpsc::UnboundedSender<Action>,
    action_rx: mpsc::UnboundedReceiver<Action>,
}

#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Mode {
    #[default]
    Home,
}

impl App {
    pub fn new(args: Cli) -> Result<Self> {
        let (action_tx, action_rx) = mpsc::unbounded_channel();

        return Ok(Self {
            arena: Arena::new(),
            tick_rate: args.tick_rate,
            frame_rate: args.frame_rate,
            fullscreen: args.fullscreen,
            inline_height: args.inline_height,
            ui_components: vec![Box::new(Home::new()), Box::new(FpsCounter::default())],
            should_quit: false,
            config: Config::new()?,
            mode: Mode::Home,
            last_tick_key_events: Vec::new(),
            action_tx,
            action_rx,
        });
    }

    pub async fn run(&mut self) -> Result<()> {
        let mut terminal = Terminal::new(
            self.tick_rate,
            self.frame_rate,
            self.fullscreen,
            self.inline_height,
        )?;
        terminal.enter()?;

        for component in self.ui_components.iter_mut() {
            component.register_action_handler(self.action_tx.clone())?;
            component.register_config_handler(self.config.clone())?;
            component.init(terminal.size()?)?;
        }

        loop {
            self.handle_terminal_events(&mut terminal).await?;
            self.handle_actions(&mut terminal)?;
            if self.should_quit {
                // TODO: do we need this here when it is also called in `terminal.exit()`?
                terminal.stop_event_loop()?;
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

        let action_tx = self.action_tx.clone();

        match event {
            TermEvent::Quit => action_tx.send(AppAction::Quit.into())?,
            TermEvent::Tick => action_tx.send(AppAction::Tick.into())?,
            TermEvent::Render => action_tx.send(AppAction::Render.into())?,
            TermEvent::Resize(x, y) => action_tx.send(AppAction::Resize(x, y).into())?,
            TermEvent::Key(key) => self.handle_key_event(key)?,
            _ => {}
        }

        for component in self.ui_components.iter_mut() {
            if let Some(action) = component.handle_events(Some(event.clone()))? {
                action_tx.send(action)?;
            }
        }

        Ok(())
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> Result<()> {
        let action_tx = self.action_tx.clone();

        let Some(keymap) = self.config.keybindings.get(&self.mode) else {
            return Ok(());
        };

        match keymap.get(&vec![key]) {
            Some(action) => {
                action_tx.send(action.clone().into())?;
            }
            _ => {
                // If the key was not handled as a single key action,
                // then consider it for multi-key combinations.
                self.last_tick_key_events.push(key);

                // Check for multi-key combinations
                if let Some(action) = keymap.get(&self.last_tick_key_events) {
                    action_tx.send(action.clone().into())?;
                }
            }
        }
        Ok(())
    }

    fn handle_actions(&mut self, terminal: &mut Terminal) -> Result<()> {
        while let Ok(action) = self.action_rx.try_recv() {
            if let Action::App(app_action) = &action {
                if *app_action != AppAction::Tick && *app_action != AppAction::Render {
                    debug!("Emitted action: {:?}", app_action);
                }

                match app_action {
                    AppAction::Tick => {
                        self.last_tick_key_events.drain(..);
                    }
                    AppAction::Quit => self.should_quit = true,
                    AppAction::ClearScreen => terminal.terminal.clear()?,
                    AppAction::Resize(w, h) => self.handle_resize(terminal, *w, *h)?,
                    AppAction::Render => self.render(terminal)?,
                    _ => {}
                }
            }

            for component in self.ui_components.iter_mut() {
                if let Some(action) = component.update(action.clone())? {
                    self.action_tx.send(action)?
                };
            }
        }
        Ok(())
    }

    fn handle_resize(&mut self, terminal: &mut Terminal, w: u16, h: u16) -> Result<()> {
        terminal.resize(Rect::new(0, 0, w, h))?;
        self.render(terminal)?;
        Ok(())
    }

    fn render(&mut self, terminal: &mut Terminal) -> Result<()> {
        terminal.draw(|frame| {
            for component in self.ui_components.iter_mut() {
                if let Err(err) = component.draw(frame, frame.area()) {
                    let _ = self
                        .action_tx
                        .send(AppAction::Error(format!("Failed to draw: {:?}", err)).into());
                }
            }
        })?;
        Ok(())
    }
}
