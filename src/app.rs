use color_eyre::Result;
use log::info;
use ratatui::{
    DefaultTerminal,
    crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind},
    widgets::ListState,
};
use std::{
    fs::read_dir,
    path::{Path, PathBuf},
};

use crate::{Node, node::NodeKind};

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
    pub view_items: Vec<PathBuf>,
}

impl App {
    pub fn new() -> Self {
        let content = Node::new(Path::new("."), 0);
        let mut app = Self {
            content,
            should_exit: false,
            state: ListState::default(),
            sort: Sort::default(),
            filter: Filter::default(),
            input_mode: InputMode::default(),
            view_items: vec![],
        };
        app.state.select(Some(0));
        app
    }

    pub fn run(mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        // Initiales Laden der View-Items
        self.update_view_items();

        while !self.should_exit {
            terminal.draw(|frame| frame.render_widget(&mut self, frame.area()))?;
            info!("draw");
            if let Event::Key(key) = event::read()? {
                self.handle_key(key);
            };
        }
        Ok(())
    }

    /// NEU: Diese Funktion aktualisiert die logische Liste der sichtbaren Elemente.
    /// Sie wird nur aufgerufen, wenn sich der Zustand ändert (z.B. Ordner öffnen).
    fn update_view_items(&mut self) {
        info!("Updating view items");
        self.view_items = flatten_tree_for_view(&self.content, &self.filter);
    }

    /// Holt sich den Pfad des aktuell ausgewählten Elements.
    fn get_selected_path(&self) -> Option<&PathBuf> {
        self.state.selected().and_then(|i| self.view_items.get(i))
    }

    fn close_parent(&mut self) {
        if let Some(child_path) = self.get_selected_path().cloned() {
            if let Some(parent_path) = child_path.parent() {
                if parent_path.as_os_str().is_empty() {
                    return; // Verhindert das Schließen des Wurzelverzeichnisses
                }

                // Den Elternknoten im Baum finden und schließen
                if let Some(parent_node) = self.content.find_node_by_path_mut(parent_path) {
                    if let NodeKind::Directory { is_open, .. } = &mut parent_node.kind {
                        *is_open = false;
                    }
                }

                // View aktualisieren und Auswahl auf den Elternteil setzen
                self.update_view_items();
                if let Some(parent_index) = self.view_items.iter().position(|p| p == parent_path) {
                    self.state.select(Some(parent_index));
                }
            }
        }
    }

    fn toggle_folder(&mut self) {
        if let Some(selected_path) = self.get_selected_path().cloned() {
            if let Some(node) = self.content.find_node_by_path_mut(&selected_path) {
                if let NodeKind::Directory { is_open, children } = &mut node.kind {
                    // Kinder laden, falls noch nicht geschehen
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

/// NEU: Diese Funktion durchläuft den Baum und erstellt eine flache Liste von Pfaden.
fn flatten_tree_for_view(root_node: &Node, filter: &Filter) -> Vec<PathBuf> {
    let mut view_items = Vec::new();
    build_view_recursive(root_node, &mut view_items, filter);
    view_items
}

fn build_view_recursive(node: &Node, view_items: &mut Vec<PathBuf>, filter: &Filter) {
    view_items.push(node.path.clone());

    if let NodeKind::Directory { children, is_open } = &node.kind {
        if *is_open {
            if let Some(children) = children {
                for child in children {
                    // Filterlogik anwenden
                    let mut should_display = true;
                    let file_name_str =
                        child.path.file_name().unwrap_or_default().to_string_lossy();

                    if filter.dotfiles && file_name_str.starts_with('.') {
                        should_display = false;
                    }
                    if filter.files && matches!(child.kind, NodeKind::File) {
                        should_display = false;
                    }
                    if filter.directories && matches!(child.kind, NodeKind::Directory { .. }) {
                        should_display = false;
                    }

                    if should_display {
                        build_view_recursive(child, view_items, filter);
                    }
                }
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
