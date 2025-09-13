use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub(crate) struct Node {
    pub path: PathBuf,
    pub kind: NodeKind,
    pub depth: usize,
}

#[derive(Debug, Clone)]
pub(crate) enum NodeKind {
    Directory {
        children: Option<Vec<Box<Node>>>,
        is_open: bool,
    },
    File,
}

impl Node {
    pub fn new(path: &Path, depth: usize) -> Self {
        let path_buf = path.to_path_buf();
        let kind = if path.is_dir() {
            NodeKind::Directory {
                children: None,
                is_open: false,
            }
        } else {
            NodeKind::File
        };
        Node {
            path: path_buf,
            kind,
            depth,
        }
    }

    pub(crate) fn find_node_by_path_mut(&mut self, target_path: &Path) -> Option<&mut Node> {
        if self.path == target_path {
            return Some(self);
        }

        if let NodeKind::Directory { children, .. } = &mut self.kind {
            if let Some(children) = children {
                for child in children {
                    if let Some(found) = child.find_node_by_path_mut(target_path) {
                        return Some(found);
                    }
                }
            }
        }
        None
    }

    pub(crate) fn find_node_by_path(&self, target_path: &Path) -> Option<&Node> {
        if self.path == target_path {
            return Some(self);
        }

        if let NodeKind::Directory { children, .. } = &self.kind {
            if let Some(children) = children {
                for child in children {
                    if let Some(found) = child.find_node_by_path(target_path) {
                        return Some(found);
                    }
                }
            }
        }
        None
    }
}
