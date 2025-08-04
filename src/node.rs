use std::path::{Path, PathBuf};

/// Represents a single node in our custom tree.
/// It holds the data and the structural links (parent/children).
#[derive(Debug)]
pub struct ArenaNode<T> {
    pub data: T,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
}

/// The arena allocator for our tree structure.
/// It owns all the nodes in a single vector.
#[derive(Debug)]
pub struct Tree<T> {
    pub nodes: Vec<ArenaNode<T>>,
}

impl<T> Tree<T> {
    /// Creates a new, empty tree.
    pub fn new() -> Self {
        Tree { nodes: Vec::new() }
    }

    /// Inserts a new node into the tree.
    /// Returns the ID (index) of the newly created node.
    pub fn insert(&mut self, data: T, parent_id: Option<usize>) -> usize {
        let new_node_id = self.nodes.len();

        // Create the new node with a link to its parent.
        let new_node = ArenaNode {
            data,
            parent: parent_id,
            children: Vec::new(), // No children yet
        };

        self.nodes.push(new_node);

        // If a parent exists, update it to link to its new child.
        if let Some(id) = parent_id {
            self.nodes[id].children.push(new_node_id);
        }

        new_node_id
    }

    /// Gets an immutable reference to a node by its ID.
    pub fn get(&self, id: usize) -> Option<&ArenaNode<T>> {
        self.nodes.get(id)
    }

    /// Gets a mutable reference to a node by its ID.
    pub fn get_mut(&mut self, id: usize) -> Option<&mut ArenaNode<T>> {
        self.nodes.get_mut(id)
    }
}

/// The data for each file or directory.
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
