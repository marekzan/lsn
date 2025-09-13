use color_eyre::Result;
use ratatui::{prelude::*, widgets::*};
use tokio::sync::mpsc::UnboundedSender;

use super::Component;
use crate::{action::AppAction, config::Config};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HomeAction {}

#[derive(Default)]
pub struct Home {
    command_tx: Option<UnboundedSender<AppAction>>,
    config: Config,
}

impl Home {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Component for Home {
    fn register_action_handler(&mut self, tx: UnboundedSender<AppAction>) -> Result<()> {
        self.command_tx = Some(tx);
        Ok(())
    }

    fn register_config_handler(&mut self, config: Config) -> Result<()> {
        self.config = config;
        Ok(())
    }

    fn update(&mut self, action: AppAction) -> Result<Option<AppAction>> {
        if let AppAction::Home(home_action) = action {
            match home_action {
                // handle HomeAction variants here
            }
        }
        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        frame.render_widget(Paragraph::new("hello world"), area);
        Ok(())
    }
}