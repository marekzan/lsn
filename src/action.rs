use serde::{Deserialize, Serialize};
use strum::Display;

use crate::components::home::HomeAction;

#[derive(Debug, Clone, PartialEq, Eq, Display, Serialize, Deserialize)]
pub enum GlobalAction {
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
pub enum AppAction {
    Global(GlobalAction),
    Home(HomeAction),
}

impl From<GlobalAction> for AppAction {
    fn from(action: GlobalAction) -> Self {
        AppAction::Global(action)
    }
}

impl From<HomeAction> for AppAction {
    fn from(action: HomeAction) -> Self {
        AppAction::Home(action)
    }
}