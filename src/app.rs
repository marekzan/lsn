use color_eyre::{Result, eyre::Error};
use log::info;
use ratatui::{
    DefaultTerminal,
    crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind},
    widgets::ListState,
};
use std::{
    collections::HashMap,
    env,
    fs::read_dir,
    path::{Path, PathBuf},
};

use crate::{Node, node::NodeKind};

#[derive(Default, Debug)]
pub(crate) enum Sort {
    #[default]
    Directory,
    File,
    Alphabetical,
}

#[derive(Default, Debug)]
pub(crate) struct Filter {
    pub directories: bool,
    pub files: bool,
    pub dotfiles: bool,
}

#[derive(Default, Debug)]
pub(crate) enum InputMode {
    #[default]
    Normal,
    FilterKey,
}

#[derive(Debug)]
pub(crate) struct App {
    should_exit: bool,
    pub tree_representation: Node,
    pub data_representation: HashMap<PathBuf, Node>,
    pub ui_representation: Vec<PathBuf>,
    pub state: ListState,
    pub sort: Sort,
    pub filter: Filter,
    pub input_mode: InputMode,
}

impl App {
    pub(crate) fn new() -> Result<Self, Error> {
        let cwd = env::current_dir()?;
        let content = Node::new(&cwd, 0);
        let mut app = Self {
            tree_representation: content.clone(),
            data_representation: HashMap::new(),
            ui_representation: vec![],
            should_exit: false,
            state: ListState::default(),
            sort: Sort::default(),
            filter: Filter::default(),
            input_mode: InputMode::default(),
        };
        app.data_representation.insert(cwd, content);
        app.state.select(Some(0));
        info!("{:?}", app);
        Ok(app)
    }

    pub fn run(mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        self.toggle_folder();
        self.update_view_items();

        while !self.should_exit {
            terminal.draw(|frame| frame.render_widget(&mut self, frame.area()))?;
            if let Event::Key(key) = event::read()? {
                self.handle_key(key);
            };
        }
        Ok(())
    }

    fn update_view_items(&mut self) {
        info!("Updating view items");
        self.ui_representation = flatten_tree_for_view(&self.tree_representation, &self.filter);
    }

    fn get_selected_path(&self) -> Option<&PathBuf> {
        self.state
            .selected()
            .and_then(|i| self.ui_representation.get(i))
    }

    fn close_parent(&mut self) {
        if let Some(child_path) = self.get_selected_path().cloned() {
            if let Some(parent_path) = child_path.parent() {
                if parent_path.as_os_str().is_empty() {
                    return;
                }

                if let Some(parent_node) =
                    self.tree_representation.find_node_by_path_mut(parent_path)
                {
                    if let NodeKind::Directory { is_open, .. } = &mut parent_node.kind {
                        *is_open = false;
                    }
                }

                self.update_view_items();
                if let Some(parent_index) =
                    self.ui_representation.iter().position(|p| p == parent_path)
                {
                    self.state.select(Some(parent_index));
                }
            }
        }
    }

    fn toggle_folder(&mut self) {
        if let Some(selected_path) = self.get_selected_path().cloned()
            && let Some(node) = self
                .tree_representation
                .find_node_by_path_mut(&selected_path)
        {
            if let NodeKind::Directory { is_open, children } = &mut node.kind {
                if children.is_none() {
                    let mut entries = match read_dir(&node.path) {
                        Ok(entries) => entries
                            .filter_map(Result::ok)
                            .map(|entry| Box::new(Node::new(&entry.path(), node.depth + 1)))
                            .collect(),
                        Err(_) => vec![],
                    };
                    sort_children(&mut entries, &self.sort);
                    *children = Some(entries);
                }

                *is_open = !*is_open;
                self.update_view_items();
            }
        }
    }

    fn toggle_directory_filter(&mut self) {
        self.filter.directories = !self.filter.directories;
        self.update_view_items();
    }

    fn toggle_file_filter(&mut self) {
        self.filter.files = !self.filter.files;
        self.update_view_items();
    }

    fn toggle_dotfile_filter(&mut self) {
        self.filter.dotfiles = !self.filter.dotfiles;
        self.update_view_items();
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }

        match self.input_mode {
            InputMode::Normal => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => self.should_exit = true,
                KeyCode::Char('h') | KeyCode::Left => self.close_parent(),
                KeyCode::Char('j') | KeyCode::Down => self.state.select_next(),
                KeyCode::Char('k') | KeyCode::Up => self.state.select_previous(),
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

fn flatten_tree_for_view(root_node: &Node, filter: &Filter) -> Vec<PathBuf> {
    let mut view_items = Vec::new();
    build_view_recursive(root_node, &mut view_items, filter);
    view_items
}

fn build_view_recursive(node: &Node, view_items: &mut Vec<PathBuf>, filter: &Filter) {
    view_items.push(node.path.clone());

    if let NodeKind::Directory { children, is_open } = &node.kind
        && *is_open
        && let Some(children) = children
    {
        for child in children {
            let mut should_display = true;
            let file_name = child.path.file_name().unwrap_or_default().to_string_lossy();

            if filter.dotfiles && file_name.starts_with('.') {
                should_display = false;
            }
            if filter.files && matches!(child.kind, NodeKind::File) {
                should_display = false;
            }
            if filter.directories
                && matches!(child.kind, NodeKind::Directory { is_open, .. } if is_open == false)
            {
                should_display = false;
            }

            if should_display {
                build_view_recursive(child, view_items, filter);
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
