use color_eyre::Result;
use std::path::{Path, PathBuf};

use crate::app::Sort;

pub(crate) struct Node {
    pub path: PathBuf,
    pub kind: NodeKind,
}

pub(crate) enum NodeKind {
    Directory {
        children: Option<Vec<Box<Node>>>,
        is_open: bool,
    },
    File,
}

impl Node {
    pub fn new(path: &Path) -> Result<Self> {
        let path_buf = path.to_path_buf();
        let kind = if path.is_dir() {
            NodeKind::Directory {
                children: None,
                is_open: false,
            }
        } else {
            NodeKind::File
        };
        Ok(Node {
            path: path_buf,
            kind,
        })
    }

    /// Finds a node by its index in the flattened list and returns a mutable reference.
    pub fn get_node_by_index(&mut self, target_index: usize, sort: &Sort) -> Option<&mut Node> {
        let mut current_index = 0;
        self.find_node_recursive(target_index, &mut current_index, sort)
    }

    fn find_node_recursive<'a>(
        &'a mut self,
        target_index: usize,
        current_index: &mut usize,
        sort: &Sort,
    ) -> Option<&'a mut Node> {
        if *current_index == target_index {
            return Some(self);
        }

        *current_index += 1;

        if let NodeKind::Directory { children, is_open } = &mut self.kind
            && let Some(children) = children
        {
            if *is_open {
                let mut sorted_children = children.iter_mut().collect::<Vec<_>>();

                // Apply the exact same sorting logic as the view function
                sorted_children.sort_by(|a, b| match sort {
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
                for child in sorted_children {
                    if let Some(found_node) =
                        child.find_node_recursive(target_index, current_index, sort)
                    {
                        return Some(found_node);
                    }
                }
            }
        }
        None
    }

    pub(crate) fn find_node_by_path(&mut self, target_path: &Path) -> Option<&mut Node> {
        if self.path == target_path {
            return Some(self);
        }

        if let NodeKind::Directory { children, .. } = &mut self.kind
            && let Some(children) = children
        {
            for child in children {
                if let Some(found) = child.find_node_by_path(target_path) {
                    return Some(found);
                }
            }
        }
        None
    }
}
