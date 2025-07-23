use std::{fs::read_dir, path::Path};

use color_eyre::Result;
use ratatui::{
    DefaultTerminal,
    crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind},
    widgets::ListState,
};

use crate::{Node, flatten_tree_for_list, node::NodeKind};

#[derive(Default)]
pub enum Sort {
    #[default]
    Directory,
    File,
    Alphabetical,
}

#[derive(Default)]
pub struct Filter {
    pub directories: bool,
    pub files: bool,
    pub dotfiles: bool,
}

#[derive(Default)]
pub enum InputMode {
    #[default]
    Normal,
    FilterKey,
}

pub(crate) struct App {
    should_exit: bool,
    pub content: Node,
    pub state: ListState,
    pub sort: Sort,
    pub filter: Filter,
    pub input_mode: InputMode,
}

impl App {
    pub fn new() -> Result<Self> {
        let content = Node::new(Path::new("."))?;
        let mut app = Self {
            content,
            should_exit: false,
            state: ListState::default(),
            sort: Sort::default(),
            filter: Filter::default(),
            input_mode: InputMode::default(),
        };
        app.state.select(Some(1));
        Ok(app)
    }

    pub fn run(mut self, mut terminal: DefaultTerminal) -> Result<()> {
        while !self.should_exit {
            terminal.draw(|frame| frame.render_widget(&mut self, frame.area()))?;
            if let Event::Key(key) = event::read()? {
                self.handle_key(key);
            };
        }
        Ok(())
    }

    fn close_parent(&mut self) {
        if let Some(selected_index) = self.state.selected() {
            let child_path = self
                .content
                .get_node_by_index(selected_index)
                .map(|node| node.path.clone());

            if let Some(child_path) = child_path
                && let Some(parent_path) = child_path.parent()
                && let Some(parent_node) = self.content.find_node_by_path(parent_path)
                && let NodeKind::Directory { is_open, .. } = &mut parent_node.kind
            {
                *is_open = false;

                let new_list = flatten_tree_for_list(&self.content, &self.filter);
                let parent_new_index = new_list.iter().position(|line| {
                    let name = parent_path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy();
                    line.contains(name.as_ref())
                });

                if let Some(index) = parent_new_index {
                    self.state.select(Some(index))
                }
            }
        }
    }

    fn toggle_folder(&mut self) {
        if let Some(selected_index) = self.state.selected()
            && let Some(node) = self.content.get_node_by_index(selected_index)
            && let NodeKind::Directory { is_open, children } = &mut node.kind
        {
            if children.is_none() {
                let mut entries = match read_dir(&node.path) {
                    Ok(entries) => entries
                        .filter_map(|entry| entry.ok())
                        .filter_map(|entry| Node::new(&entry.path()).ok())
                        .map(Box::new)
                        .collect(),
                    Err(_) => vec![],
                };
                sort_children(&mut entries, &self.sort);
                *children = Some(entries);
            }

            *is_open = !*is_open;
        }
    }

    fn toggle_directory_filter(&mut self) {
        self.filter.directories = !self.filter.directories;
    }

    fn toggle_file_filter(&mut self) {
        self.filter.files = !self.filter.files;
    }

    fn toggle_dotfile_filter(&mut self) {
        self.filter.dotfiles = !self.filter.dotfiles;
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }

        match self.input_mode {
            InputMode::Normal => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => self.should_exit = true,
                KeyCode::Char('h') => self.close_parent(),
                KeyCode::Char('j') => self.state.select_next(),
                KeyCode::Char('k') => self.state.select_previous(),
                KeyCode::Char('g') => self.state.select_first(),
                KeyCode::Char('G') => self.state.select_last(),
                KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter => {
                    self.toggle_folder();
                }
                KeyCode::Char('f') => self.input_mode = InputMode::FilterKey,
                _ => {}
            },
            InputMode::FilterKey => {
                match key.code {
                    KeyCode::Char('d') => self.toggle_directory_filter(),
                    KeyCode::Char('f') => self.toggle_file_filter(),
                    KeyCode::Char('.') => self.toggle_dotfile_filter(),
                    _ => {}
                }
                self.input_mode = InputMode::Normal;
            }
        }
    }
}

fn sort_children(children: &mut Vec<Box<Node>>, sort: &Sort) {
    children.sort_by(|a, b| match sort {
        Sort::Directory => {
            let a_is_dir = matches!(a.kind, NodeKind::Directory { .. });
            let b_is_dir = matches!(b.kind, NodeKind::Directory { .. });
            b_is_dir.cmp(&a_is_dir).then_with(|| a.path.cmp(&b.path))
        }
        Sort::File => {
            let a_is_dir = matches!(a.kind, NodeKind::Directory { .. });
            let b_is_dir = matches!(b.kind, NodeKind::Directory { .. });
            a_is_dir.cmp(&b_is_dir).then_with(|| a.path.cmp(&b.path))
        }
        Sort::Alphabetical => a.path.cmp(&b.path),
    });
}
