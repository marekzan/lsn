use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct ArenaNode<T> {
    pub data: T,
    parent: Option<usize>,
    children: Vec<usize>,
}

#[derive(Debug)]
pub struct Tree<T> {
    nodes: Vec<ArenaNode<T>>,
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
}

#[derive(Debug, Clone)]
pub struct FsNode {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_open: bool,
}

pub struct FileSystem {
    pub tree: Tree<FsNode>,
    pub path_lookup: HashMap<PathBuf, usize>,
    ui_representation: Vec<String>,
}

impl FileSystem {
    pub fn new(root: String) -> Self {
        let mut tree = Tree::new();
        let mut path_lookup = HashMap::new();

        // TODO is there another way of doing less clone? maybe have the param type be PathBuf?
        let root_path = PathBuf::from(root.clone());
        let root_node_data = FsNode {
            name: root,
            path: root_path.clone(),
            is_dir: root_path.is_dir(),
            is_open: true,
        };

        let root_id = tree.insert(root_node_data, None);

        path_lookup.insert(root_path, root_id);

        Self {
            tree,
            path_lookup,
            ui_representation: Vec::new(),
        }
    }

    pub fn add_item(&mut self, parent_path: &Path, name: &str) -> Result<usize, &'static str> {
        let parent_id = match self.path_lookup.get(parent_path) {
            Some(&id) => id,
            None => return Err("Parent path does not exist."),
        };

        if !self.tree.get(parent_id).unwrap().data.is_dir {
            return Err("Parent is not a directory.");
        }

        let new_path = parent_path.join(name);
        let new_node_data = FsNode {
            name: name.to_string(),
            path: new_path.clone(),
            is_dir: new_path.is_dir(),
            is_open: false,
        };

        let new_id = self.tree.insert(new_node_data, Some(parent_id));

        self.path_lookup.insert(new_path, new_id);

        Ok(new_id)
    }

    pub fn get_by_path(&self, path: &Path) -> Option<&ArenaNode<FsNode>> {
        let node_id = self.path_lookup.get(path)?;
        self.tree.get(*node_id)
    }

    pub fn build_ui_view(&mut self, node_id: usize, prefix: String) {
        let node = self.tree.get(node_id).unwrap();

        self.ui_representation.push(node.data.name);

        if (node.data.is_dir && node.data.is_open) {
            let children_count = node.children.len();
            for (i, &child_id) in node.children.iter().enumerate() {
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

                self.print_tree(child_id, format!("{}{}", new_prefix, connector));
            }
        }
    }
}
