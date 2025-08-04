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

use crate::node::{FsNode, NodeKind, Tree};

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
    pub tree: Tree<FsNode>,
    pub path_to_id: HashMap<PathBuf, usize>,
    pub ui_representation: Vec<PathBuf>,
    pub state: ListState,
    pub sort: Sort,
    pub filter: Filter,
    pub input_mode: InputMode,
}

impl App {
    pub(crate) fn new() -> Result<Self, Error> {
        let cwd = env::current_dir()?;
        let mut tree = Tree::new();
        let mut lookup = HashMap::new();

        let root_node_data = FsNode::new(&cwd, 0);
        let root_id = tree.insert(root_node_data, None);
        lookup.insert(cwd, root_id);

        let mut app = Self {
            tree,
            path_to_id: lookup,
            ui_representation: vec![],
            should_exit: false,
            state: ListState::default(),
            sort: Sort::default(),
            filter: Filter::default(),
            input_mode: InputMode::default(),
        };
        app.state.select(Some(0));
        info!("{:?}", app);
        Ok(app)
    }

    pub fn run(mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        self.update_view_items();
        // Initially open the root folder to show its contents
        self.toggle_folder();

        while !self.should_exit {
            terminal.draw(|frame| frame.render_widget(&mut self, frame.area()))?;
            info!("drawing");
            if let Event::Key(key) = event::read()? {
                self.handle_key(key);
            };
        }
        Ok(())
    }

    fn update_view_items(&mut self) {
        info!("Updating view items");
        self.ui_representation = flatten_tree_for_view(&self.tree, &self.filter);
    }

    fn get_selected_path(&self) -> Option<&PathBuf> {
        self.state
            .selected()
            .and_then(|i| self.ui_representation.get(i))
    }

    /// Corrected function to avoid double mutable borrows.
    fn close_parent(&mut self) {
        // --- Phase 1: Immutable Read ---
        // Get all the info we need without holding onto long-lived borrows.
        let parent_info = self.get_selected_path().and_then(|child_path| {
            self.path_to_id.get(child_path).and_then(|&child_id| {
                self.tree.get(child_id)?.parent.and_then(|parent_id| {
                    let parent_path = self.tree.get(parent_id)?.data.path.clone();
                    Some((parent_id, parent_path)) // Return the IDs and paths we need
                })
            })
        });

        // --- Phase 2: Mutable Write ---
        // The immutable borrows from Phase 1 are now dropped. We can safely borrow mutably.
        if let Some((parent_id, parent_path)) = parent_info {
            // Get the mutable reference to the parent node.
            if let Some(parent_node) = self.tree.get_mut(parent_id) {
                if let NodeKind::Directory { is_open, .. } = &mut parent_node.data.kind {
                    *is_open = false;
                }
            } else {
                return;
            }

            // Perform other mutable operations.
            self.update_view_items();

            // Find the new position and update the state.
            if let Some(parent_index) = self
                .ui_representation
                .iter()
                .position(|p| p == &parent_path)
            {
                self.state.select(Some(parent_index));
            }
        }
    }

    /// Corrected function to avoid conflicting mutable borrows.
    fn toggle_folder(&mut self) {
        // --- Phase 1: Immutable read to get basic info ---
        let selected_info = self
            .get_selected_path()
            .and_then(|p| self.path_to_id.get(p).map(|&id| (id, p.clone())));

        if let Some((node_id, path)) = selected_info {
            let mut needs_loading = false;
            let mut is_directory = false;

            // --- Phase 2: Check if children need loading in a small scope ---
            if let Some(node) = self.tree.get(node_id) {
                if let NodeKind::Directory {
                    children_loaded, ..
                } = node.data.kind
                {
                    is_directory = true;
                    if !children_loaded {
                        needs_loading = true;
                    }
                }
            }

            // --- Phase 3: Load children if needed (this part mutates the tree) ---
            if needs_loading {
                let parent_depth = self.tree.get(node_id).unwrap().data.depth;
                let mut entries: Vec<(PathBuf, FsNode)> = match read_dir(&path) {
                    Ok(entries) => entries
                        .filter_map(Result::ok)
                        .map(|entry| {
                            let path = entry.path();
                            (path.clone(), FsNode::new(&path, parent_depth + 1))
                        })
                        .collect(),
                    Err(_) => vec![],
                };
                sort_children(&mut entries, &self.sort);

                for (child_path, fs_node) in entries {
                    let child_id = self.tree.insert(fs_node, Some(node_id));
                    self.path_to_id.insert(child_path, child_id);
                }

                if let Some(node) = self.tree.get_mut(node_id) {
                    if let NodeKind::Directory {
                        children_loaded, ..
                    } = &mut node.data.kind
                    {
                        *children_loaded = true;
                    }
                }
            }

            // --- Phase 4: Toggle open state and update UI ---
            if is_directory {
                if let Some(node) = self.tree.get_mut(node_id) {
                    if let NodeKind::Directory { is_open, .. } = &mut node.data.kind {
                        *is_open = !*is_open;
                    }
                }
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

fn flatten_tree_for_view(tree: &Tree<FsNode>, filter: &Filter) -> Vec<PathBuf> {
    let mut view_items = Vec::new();
    if !tree.nodes.is_empty() {
        build_view_recursive(tree, 0, &mut view_items, filter);
    }
    view_items
}

fn build_view_recursive(
    tree: &Tree<FsNode>,
    node_id: usize,
    view_items: &mut Vec<PathBuf>,
    filter: &Filter,
) {
    let node = tree.get(node_id).unwrap();
    view_items.push(node.data.path.clone());

    if let NodeKind::Directory { is_open, .. } = &node.data.kind {
        if *is_open {
            for &child_id in &node.children {
                let child_node = tree.get(child_id).unwrap();
                let mut should_display = true;
                let file_name = child_node
                    .data
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy();

                if filter.dotfiles && file_name.starts_with('.') {
                    should_display = false;
                }
                if filter.files && matches!(child_node.data.kind, NodeKind::File) {
                    should_display = false;
                }
                if filter.directories {
                    if let NodeKind::Directory { is_open, .. } = child_node.data.kind {
                        if !is_open {
                            should_display = false;
                        }
                    }
                }

                if should_display {
                    build_view_recursive(tree, child_id, view_items, filter);
                }
            }
        }
    }
}

fn sort_children(children: &mut Vec<(PathBuf, FsNode)>, sort: &Sort) {
    children.sort_by(|(_, a), (_, b)| match sort {
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
