use crate::ViewItem;
use color_eyre::{Result, eyre::Error};
use ratatui::{
    DefaultTerminal,
    crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind},
    widgets::ListState,
};

#[derive(Default, Debug, Clone, Copy)]
pub enum Sort {
    #[default]
    Directory,
    File,
    Alphabetical,
}

#[derive(Default, Debug, Clone, Copy)]
pub struct Filter {
    pub directories: bool,
    pub files: bool,
    pub dotfiles: bool,
}

#[derive(Default, Debug)]
pub enum InputMode {
    #[default]
    Normal,
    FilterKey,
}

#[derive(Debug)]
pub enum Action {
    Quit,
    ToggleFolder,
    CloseNearest,
    NavigateUp,
    NavigateDown,
    NavigateTop,
    NavigateBottom,
    ToggleFilter(FilterType),
}

#[derive(Debug)]
pub enum FilterType {
    Directory,
    File,
    Dotfile,
}

#[derive(Debug)]
pub struct Ui {
    pub state: ListState,
    pub sort: Sort,
    pub filter: Filter,
    pub input_mode: InputMode,
}

impl Ui {
    pub fn new() -> Result<Self, Error> {
        let mut app = Self {
            state: ListState::default(),
            sort: Sort::default(),
            filter: Filter::default(),
            input_mode: InputMode::default(),
        };
        app.state.select(Some(0));
        Ok(app)
    }

    pub fn draw(&mut self, terminal: &mut DefaultTerminal, items: &[ViewItem]) -> Result<()> {
        terminal.draw(|frame| {
            crate::ui::render(self, items, frame.area(), frame.buffer_mut());
        })?;
        Ok(())
    }

    pub fn handle_input(&mut self) -> Result<Option<Action>> {
        if let Event::Key(key) = event::read()? {
            return Ok(self.handle_key(key));
        }
        Ok(None)
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        if key.kind != KeyEventKind::Press {
            return None;
        }

        match self.input_mode {
            InputMode::Normal => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => Some(Action::Quit),
                KeyCode::Char('h') | KeyCode::Left => Some(Action::CloseNearest),
                KeyCode::Char('j') | KeyCode::Down => {
                    self.state.select_next();
                    Some(Action::NavigateDown)
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.state.select_previous();
                    Some(Action::NavigateUp)
                }
                KeyCode::Char('g') => {
                    self.state.select_first();
                    Some(Action::NavigateTop)
                }
                KeyCode::Char('G') => {
                    self.state.select_last();
                    Some(Action::NavigateBottom)
                }
                KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter => Some(Action::ToggleFolder),
                KeyCode::Char('f') => {
                    self.input_mode = InputMode::FilterKey;
                    None
                }
                _ => None,
            },
            InputMode::FilterKey => {
                let action = match key.code {
                    KeyCode::Char('d') => {
                        self.filter.directories = !self.filter.directories;
                        Some(Action::ToggleFilter(FilterType::Directory))
                    }
                    KeyCode::Char('f') => {
                        self.filter.files = !self.filter.files;
                        Some(Action::ToggleFilter(FilterType::File))
                    }
                    KeyCode::Char('.') => {
                        self.filter.dotfiles = !self.filter.dotfiles;
                        Some(Action::ToggleFilter(FilterType::Dotfile))
                    }
                    _ => None,
                };
                self.input_mode = InputMode::Normal;
                action
            }
        }
    }
}
