use serde::{Deserialize, Serialize};
use strum::Display;

use crate::components::home::HomeAction;

#[derive(Debug, Clone, PartialEq, Eq, Display, Serialize, Deserialize)]
pub enum AppAction {
    Tick,
    Render,
    Resize(u16, u16),
    Suspend,
    Resume,
    Quit,
    ClearScreen,
    Error(String),
    Help,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    App(AppAction),
    Home(HomeAction),
}

impl From<AppAction> for Action {
    fn from(action: AppAction) -> Self {
        Action::App(action)
    }
}

impl From<HomeAction> for Action {
    fn from(action: HomeAction) -> Self {
        Action::Home(action)
    }
}
