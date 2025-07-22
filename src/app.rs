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

pub(crate) struct App {
    should_exit: bool,
    pub content: Node,
    pub state: ListState,
    pub sort: Sort,
}

impl App {
    pub fn new() -> Result<Self> {
        /* let current_path */
        let content = Node::new(Path::new("."))?;
        let mut app = Self {
            content,
            should_exit: false,
            state: ListState::default(),
            sort: Sort::default(),
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
                .get_node_by_index(selected_index, &self.sort)
                .map(|node| node.path.clone());

            if let Some(child_path) = child_path
                && let Some(parent_path) = child_path.parent()
                && let Some(parent_node) = self.content.find_node_by_path(parent_path)
                && let NodeKind::Directory { is_open, .. } = &mut parent_node.kind
            {
                *is_open = false;

                let new_list = flatten_tree_for_list(&self.content, &self.sort);
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
            && let Some(node) = self.content.get_node_by_index(selected_index, &self.sort)
            && let NodeKind::Directory { is_open, children } = &mut node.kind
        {
            if children.is_none() {
                let entries = match read_dir(&node.path) {
                    Ok(entries) => entries
                        .filter_map(|entry| entry.ok())
                        .filter_map(|entry| Node::new(&entry.path()).ok())
                        .map(Box::new)
                        .collect(),
                    Err(_) => vec![],
                };
                *children = Some(entries);
            }

            *is_open = !*is_open;
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.should_exit = true,
            KeyCode::Char('h') | KeyCode::Left => self.close_parent(),
            KeyCode::Char('j') | KeyCode::Down => self.state.select_next(),
            KeyCode::Char('k') | KeyCode::Up => self.state.select_previous(),
            KeyCode::Char('g') | KeyCode::Home => self.state.select_first(),
            KeyCode::Char('G') | KeyCode::End => self.state.select_last(),
            KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter => {
                self.toggle_folder();
            }
            _ => {}
        }
    }
}
