use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct ArenaNode<T> {
    pub data: T,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
}

#[derive(Debug)]
pub struct Tree<T> {
    pub nodes: Vec<ArenaNode<T>>,
}

impl<T> Tree<T> {
    pub fn new() -> Self {
        Tree { nodes: Vec::new() }
    }

    pub fn insert(&mut self, data: T, parent_id: Option<usize>) -> usize {
        let new_node_id = self.nodes.len();

        let new_node = ArenaNode {
            data,
            parent: parent_id,
            children: Vec::new(),
        };

        self.nodes.push(new_node);

        if let Some(id) = parent_id {
            self.nodes[id].children.push(new_node_id);
        }

        new_node_id
    }

    pub fn get(&self, id: usize) -> Option<&ArenaNode<T>> {
        self.nodes.get(id)
    }

    pub fn get_mut(&mut self, id: usize) -> Option<&mut ArenaNode<T>> {
        self.nodes.get_mut(id)
    }
}

#[derive(Debug, Clone)]
pub struct FsNode {
    pub path: PathBuf,
    pub kind: NodeKind,
    pub depth: usize,
}

#[derive(Debug, Clone)]
pub enum NodeKind {
    Directory {
        children_loaded: bool,
        is_open: bool,
    },
    File,
}

impl FsNode {
    pub fn new(path: &Path, depth: usize) -> Self {
        let path_buf = path.to_path_buf();
        let kind = if path.is_dir() {
            NodeKind::Directory {
                children_loaded: false,
                is_open: false,
            }
        } else {
            NodeKind::File
        };
        FsNode {
            path: path_buf,
            kind,
            depth,
        }
    }
}
