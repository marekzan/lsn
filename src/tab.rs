use std::{
    collections::BTreeSet,
    io,
    path::{Path, PathBuf},
};

use crate::{
    arena::Handle,
    fs::{node::Node, tree::FileTree},
    view::{self, Filter, Sort, ViewContext, ViewItem},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabKind {
    /// a normal tab rooted at one directory
    Dir,
    /// the pinned folders, each one is a root of the tree
    Pins,
}

/// one tab: its own tree, its own view on it and its own cursor.
///
/// marks and the clipboard are not part of a tab, they are global so files
/// can be marked in one tab and pasted in another.
pub struct Tab {
    pub kind: TabKind,
    pub tree: FileTree,
    pub items: Vec<ViewItem>,
    pub cursor: usize,
    /// first visible row, kept up to date by the ui
    pub offset: usize,
    pub filter: Filter,
    pub sort: Sort,
    /// the view list has to be rebuilt before it is used next
    pub stale: bool,
}

impl Tab {
    pub fn new_dir(path: &Path) -> io::Result<Self> {
        Ok(Self::with_tree(TabKind::Dir, FileTree::with_root(path)?))
    }

    pub fn new_pins(pins: &[PathBuf]) -> Self {
        let mut tree = FileTree::new();
        for pin in pins {
            // pins that vanished from disk are skipped silently
            let _ = tree.add_root(pin);
        }
        Self::with_tree(TabKind::Pins, tree)
    }

    fn with_tree(kind: TabKind, tree: FileTree) -> Self {
        Self {
            kind,
            tree,
            items: Vec::new(),
            cursor: 0,
            offset: 0,
            filter: Filter::default(),
            sort: Sort::default(),
            stale: true,
        }
    }

    pub fn title(&self) -> String {
        match self.kind {
            TabKind::Pins => "★ pins".to_string(),
            TabKind::Dir => self
                .root_path()
                .map(crate::fs::node::display_name)
                .unwrap_or_default(),
        }
    }

    /// root of a normal tab
    pub fn root_path(&self) -> Option<&Path> {
        match self.kind {
            TabKind::Dir => self.tree.roots().first().and_then(|r| self.tree.path(*r)),
            TabKind::Pins => None,
        }
    }

    pub fn rebuild(&mut self, marked: &BTreeSet<PathBuf>) {
        let previous = self.current();
        let ctx = ViewContext {
            filter: &self.filter,
            sort: &self.sort,
            marked,
        };
        self.items = view::build(&self.tree, &ctx, self.kind == TabKind::Pins);
        self.stale = false;

        // keep the cursor on the same node if it is still visible
        if let Some(pos) = previous.and_then(|h| self.position(h)) {
            self.cursor = pos;
        }
        self.cursor = self.cursor.min(self.items.len().saturating_sub(1));
    }

    pub fn position(&self, handle: Handle) -> Option<usize> {
        self.items.iter().position(|i| i.handle == handle)
    }

    pub fn current(&self) -> Option<Handle> {
        self.items.get(self.cursor).map(|i| i.handle)
    }

    pub fn current_node(&self) -> Option<&Node> {
        self.current().and_then(|h| self.tree.get(h))
    }

    pub fn current_path(&self) -> Option<PathBuf> {
        self.current_node().map(|n| n.path.clone())
    }

    /// the directory something should be created in or pasted into:
    /// the directory under the cursor, or the parent of the file under it
    pub fn target_dir(&self) -> Option<PathBuf> {
        match self.current_node() {
            Some(node) if node.is_dir() => Some(node.path.clone()),
            Some(node) => node.path.parent().map(Path::to_path_buf),
            None => self.root_path().map(Path::to_path_buf),
        }
    }

    pub fn move_cursor(&mut self, delta: isize) {
        let last = self.items.len().saturating_sub(1) as isize;
        self.cursor = (self.cursor as isize + delta).clamp(0, last.max(0)) as usize;
    }

    pub fn select(&mut self, handle: Handle) {
        if let Some(pos) = self.position(handle) {
            self.cursor = pos;
        }
    }

    /// expands the way to `path` and puts the cursor on it
    pub fn reveal(&mut self, path: &Path, marked: &BTreeSet<PathBuf>) {
        if let Some(h) = self.tree.reveal(path) {
            self.rebuild(marked);
            self.select(h);
        }
    }

    /// makes `path` the new root of this tab. directories that were expanded
    /// before stay expanded if they are part of the new tree.
    pub fn set_root(&mut self, path: &Path, marked: &BTreeSet<PathBuf>) -> io::Result<()> {
        let old_root = self.root_path().map(Path::to_path_buf);
        let expanded = self.tree.expanded_paths();
        let mut tree = FileTree::with_root(path)?;
        for dir in expanded {
            if let Some(h) = tree.reveal(&dir) {
                tree.expand(h);
            }
        }
        self.tree = tree;
        self.cursor = 0;
        self.offset = 0;
        self.rebuild(marked);

        // after `cd ..` the cursor should be on the directory we came from
        if let Some(h) = old_root.and_then(|p| self.tree.find(&p)) {
            self.select(h);
        }
        Ok(())
    }
}
