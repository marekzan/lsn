use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct FsNode {
    pub path: PathBuf,
    pub kind: FsNodeKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FsNodeKind {
    Directory {
        children: Vec<PathBuf>,
        is_open: bool,
    },
    File,
}

#[derive(Debug, Default)]
pub struct StateManager {
    /// The flat database of all known files
    pub fs_nodes: HashMap<PathBuf, FsNode>,
    pub root: PathBuf,
}

impl StateManager {
    pub fn new(root: PathBuf) -> Self {
        let mut manager = Self {
            fs_nodes: HashMap::new(),
            root: root.clone(),
        };
        // Initialize root
        manager.add_path(&root);
        manager
    }

    fn add_path(&mut self, path: &PathBuf) {
        let kind = if path.is_dir() {
            FsNodeKind::Directory {
                children: Vec::new(),
                is_open: false,
            }
        } else {
            FsNodeKind::File
        };

        self.fs_nodes.insert(
            path.clone(),
            FsNode {
                path: path.clone(),
                kind,
            },
        );
    }

    /// Loads children for a specific path, updates the map, and sets children links
    pub fn load_dir(&mut self, path: &Path) -> std::io::Result<()> {
        let read_dir = fs::read_dir(path)?;
        let mut child_paths = Vec::new();

        for entry in read_dir.filter_map(|r| r.ok()) {
            let child_path = entry.path();
            self.add_path(&child_path);
            child_paths.push(child_path);
        }

        // Sort children immediately upon loading
        child_paths.sort();

        // Update the parent entry to know about these children
        if let Some(entry) = self.fs_nodes.get_mut(path) {
            if let FsNodeKind::Directory { children, .. } = &mut entry.kind {
                *children = child_paths;
            }
        }

        Ok(())
    }

    pub fn toggle_open(&mut self, path: &Path) {
        if let Some(entry) = self.fs_nodes.get_mut(path) {
            if let FsNodeKind::Directory { is_open, .. } = &mut entry.kind {
                *is_open = !*is_open;
            }
        }
    }

    pub fn set_open(&mut self, path: &Path, open: bool) {
        if let Some(entry) = self.fs_nodes.get_mut(path) {
            if let FsNodeKind::Directory { is_open, .. } = &mut entry.kind {
                *is_open = open;
            }
        }
    }

    pub fn get_entry(&self, path: &Path) -> Option<&FsNode> {
        self.fs_nodes.get(path)
    }
}
