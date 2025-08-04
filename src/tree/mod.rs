use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Represents a single node in our custom tree.
/// It holds the data and the structural links (parent/children).
#[derive(Debug)]
pub struct ArenaNode<T> {
    pub data: T,
    parent: Option<usize>,
    children: Vec<usize>,
}

/// The arena allocator for our tree structure.
/// It owns all the nodes in a single vector.
#[derive(Debug)]
pub struct Tree<T> {
    nodes: Vec<ArenaNode<T>>,
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
}

// --- FileSystem Implementation (Using our Custom Tree) ---

/// The data for each file or directory.
#[derive(Debug, Clone)]
pub struct FsNode {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_open: bool,
}

/// A struct that combines our custom tree for structure and a HashMap for O(1) lookup.
pub struct FileSystem {
    /// The arena-based tree holding all FsNode data.
    pub tree: Tree<FsNode>,
    /// The fast lookup index. Maps a full path to its NodeId (which is a usize).
    pub path_lookup: HashMap<PathBuf, usize>,
    /// The ui represenation of the filesystem
    ui_representation: Vec<String>,
}

impl FileSystem {
    /// Creates a new, empty FileSystem with a root directory.
    pub fn new(root: String) -> Self {
        let mut tree = Tree::new();
        let mut path_lookup = HashMap::new();

        // Create the root node
        // TODO is there another way of doing less clone? maybe have the param type be PathBuf?
        let root_path = PathBuf::from(root.clone());
        let root_node_data = FsNode {
            name: root,
            path: root_path.clone(),
            is_dir: root_path.is_dir(),
            is_open: true,
        };

        // Insert into the tree as the root (no parent) and get its ID
        let root_id = tree.insert(root_node_data, None);

        // Add the root to our lookup index
        path_lookup.insert(root_path, root_id);

        Self {
            tree,
            path_lookup,
            ui_representation: Vec::new(),
        }
    }

    /// Adds a new item to the filesystem under a given parent.
    pub fn add_item(&mut self, parent_path: &Path, name: &str) -> Result<usize, &'static str> {
        // 1. Find the parent's NodeId using the O(1) lookup.
        let parent_id = match self.path_lookup.get(parent_path) {
            Some(&id) => id,
            None => return Err("Parent path does not exist."),
        };

        // Ensure the parent is actually a directory
        if !self.tree.get(parent_id).unwrap().data.is_dir {
            return Err("Parent is not a directory.");
        }

        // 2. Create the new node's data
        let new_path = parent_path.join(name);
        let new_node_data = FsNode {
            name: name.to_string(),
            path: new_path.clone(),
            is_dir: new_path.is_dir(),
            is_open: false,
        };

        // 3. Insert the new node into the tree under its parent
        let new_id = self.tree.insert(new_node_data, Some(parent_id));

        // 4. CRITICAL: Add the new node to our lookup index to maintain synchronization.
        self.path_lookup.insert(new_path, new_id);

        Ok(new_id)
    }

    /// Gets a node's data using its path with O(1) complexity.
    pub fn get_by_path(&self, path: &Path) -> Option<&ArenaNode<FsNode>> {
        // Step 1: O(1) lookup in the HashMap to get the NodeId.
        let node_id = self.path_lookup.get(path)?;
        // Step 2: O(1) lookup in the tree's arena (Vec) using the NodeId.
        self.tree.get(*node_id)
    }

    /// Traverses the tree from a given node ID and prints its structure.
    pub fn build_ui_view(&mut self, node_id: usize, prefix: String) {
        // Get the current node
        let node = self.tree.get(node_id).unwrap();

        // Push the current node's name with the prefix to the ui representation vector
        self.ui_representation.push(node.data.name);

        if (node.data.is_dir && node.data.is_open) {
            // Prepare the prefix for the children
            let children_count = node.children.len();
            for (i, &child_id) in node.children.iter().enumerate() {
                // Determine if this is the last child to use the correct box-drawing character
                let new_prefix = if i == children_count - 1 {
                    format!("{}    ", prefix.replace("├──", "│  ").replace("└──", "   "))
                } else {
                    format!("{}│   ", prefix.replace("├──", "│  ").replace("└──", "   "))
                };

                let connector = if i == children_count - 1 {
                    "└── "
                } else {
                    "├── "
                };

                // Recursively call for each child
                self.print_tree(child_id, format!("{}{}", new_prefix, connector));
            }
        }
    }
}
