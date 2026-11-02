use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
};

use crate::{
    arena::{Arena, Handle},
    fs::node::Node,
};

/// the model: everything we know about the file system, stored in an arena.
///
/// nodes reference each other through handles instead of `Box`/`Rc`, so we can
/// keep stable references to them (view list, cursor) and walk up to the
/// parent without fighting the borrow checker.
///
/// a tree can have multiple roots. a normal tab has exactly one, the pins tab
/// has one per pinned folder.
///
/// children are stored in the order the os returns them. sorting and filtering
/// is a concern of the view (see `view.rs`) so the model never has to change
/// when the user only changes how things are displayed.
#[derive(Default)]
pub struct FileTree {
    arena: Arena<Node>,
    roots: Vec<Handle>,
}

impl FileTree {
    pub fn new() -> Self {
        Self::default()
    }

    /// creates a tree with a single, already expanded root
    pub fn with_root(path: &Path) -> io::Result<Self> {
        let mut tree = Self::new();
        let root = tree.add_root(path)?;
        tree.expand(root);
        Ok(tree)
    }

    pub fn add_root(&mut self, path: &Path) -> io::Result<Handle> {
        let path = std::fs::canonicalize(path)?;
        let node = Node::from_path(&path, None)?;
        let handle = self.arena.insert(node);
        self.roots.push(handle);
        Ok(handle)
    }

    pub fn remove_root(&mut self, handle: Handle) {
        self.roots.retain(|r| *r != handle);
        self.remove_subtree(handle);
    }

    pub fn roots(&self) -> &[Handle] {
        &self.roots
    }

    pub fn get(&self, handle: Handle) -> Option<&Node> {
        self.arena.get(&handle)
    }

    pub fn path(&self, handle: Handle) -> Option<&Path> {
        self.get(handle).map(|n| n.path.as_path())
    }

    pub fn parent(&self, handle: Handle) -> Option<Handle> {
        self.get(handle).and_then(|n| n.parent)
    }

    /// loaded children of a node, empty if the node is a file or was not read yet
    pub fn children(&self, handle: Handle) -> &[Handle] {
        self.get(handle)
            .and_then(|n| n.children.as_deref())
            .unwrap_or(&[])
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.arena.len()
    }

    /// reads the directory behind `handle` and stores its entries as children.
    /// errors are stored on the node so the ui can show them.
    pub fn load_children(&mut self, handle: Handle) {
        let Some(node) = self.arena.get(&handle) else {
            return;
        };
        if !node.is_dir() {
            return;
        }

        let path = node.path.clone();
        match read_dir_sorted(&path) {
            Ok(paths) => {
                let children = paths
                    .iter()
                    .filter_map(|p| Node::from_path(p, Some(handle)).ok())
                    .map(|n| self.arena.insert(n))
                    .collect();
                let node = self.arena.get_mut(&handle).expect("checked above");
                node.children = Some(children);
                node.error = None;
            }
            Err(err) => {
                let node = self.arena.get_mut(&handle).expect("checked above");
                node.children = Some(Vec::new());
                node.error = Some(err.to_string());
            }
        }
    }

    pub fn expand(&mut self, handle: Handle) {
        let Some(node) = self.arena.get(&handle) else {
            return;
        };
        if !node.is_dir() {
            return;
        }
        if node.children.is_none() {
            self.load_children(handle);
        }
        if let Some(node) = self.arena.get_mut(&handle) {
            node.expanded = true;
        }
    }

    pub fn collapse(&mut self, handle: Handle) {
        if let Some(node) = self.arena.get_mut(&handle) {
            node.expanded = false;
        }
    }

    pub fn toggle(&mut self, handle: Handle) {
        match self.get(handle) {
            Some(n) if n.expanded => self.collapse(handle),
            Some(_) => self.expand(handle),
            None => {}
        }
    }

    /// collapses the node and all of its loaded descendants
    pub fn collapse_all(&mut self, handle: Handle) {
        let mut stack = vec![handle];
        while let Some(h) = stack.pop() {
            stack.extend_from_slice(self.children(h));
            self.collapse(h);
        }
    }

    /// re-reads a directory from disk while keeping the handles (and with them
    /// the expanded state) of entries that still exist. loaded descendants are
    /// refreshed as well.
    pub fn refresh(&mut self, handle: Handle) {
        let Some(node) = self.arena.get(&handle) else {
            return;
        };
        let path = node.path.clone();

        // update the metadata of the node itself, it might have been chmod'ed
        if let Ok(fresh) = Node::from_path(&path, node.parent) {
            let node = self.arena.get_mut(&handle).expect("checked above");
            if node.kind != fresh.kind {
                // a directory became a file (or the other way around)
                let old_children = node.children.take().unwrap_or_default();
                node.expanded = false;
                node.kind = fresh.kind;
                for child in old_children {
                    self.remove_subtree(child);
                }
            }
            let node = self.arena.get_mut(&handle).expect("checked above");
            node.meta = fresh.meta;
        }

        let Some(old_children) = self.get(handle).and_then(|n| n.children.clone()) else {
            // never loaded, nothing to keep in sync
            return;
        };

        let entries = match read_dir_sorted(&path) {
            Ok(entries) => entries,
            Err(err) => {
                for child in old_children {
                    self.remove_subtree(child);
                }
                let node = self.arena.get_mut(&handle).expect("checked above");
                node.children = Some(Vec::new());
                node.error = Some(err.to_string());
                return;
            }
        };

        let mut existing: HashMap<PathBuf, Handle> = old_children
            .iter()
            .filter_map(|h| self.get(*h).map(|n| (n.path.clone(), *h)))
            .collect();

        let mut children = Vec::with_capacity(entries.len());
        for entry in entries {
            if let Some(h) = existing.remove(&entry) {
                children.push(h);
                self.refresh(h);
            } else if let Ok(node) = Node::from_path(&entry, Some(handle)) {
                children.push(self.arena.insert(node));
            }
        }

        // whatever is left vanished from disk
        for (_, h) in existing {
            self.remove_subtree(h);
        }

        let node = self.arena.get_mut(&handle).expect("checked above");
        node.children = Some(children);
        node.error = None;
    }

    pub fn refresh_all(&mut self) {
        for root in self.roots.clone() {
            self.refresh(root);
        }
    }

    /// removes the node and everything below it from the arena.
    /// the caller is responsible for unlinking it from its parent.
    fn remove_subtree(&mut self, handle: Handle) {
        let mut stack = vec![handle];
        while let Some(h) = stack.pop() {
            if let Some(node) = self.arena.remove(h) {
                stack.extend(node.children.unwrap_or_default());
            }
        }
    }

    /// reads not yet loaded directories breadth first, so a search can find
    /// entries the user never opened. symlinks are not followed and hidden
    /// directories are skipped unless asked for. stops after `budget` new
    /// nodes and returns `false` in that case.
    pub fn load_deep(&mut self, budget: usize, include_hidden: bool) -> bool {
        let start = self.arena.len();
        let mut queue: std::collections::VecDeque<Handle> = self.roots.iter().copied().collect();
        while let Some(h) = queue.pop_front() {
            if self.arena.len() - start >= budget {
                return false;
            }
            let Some(node) = self.get(h) else { continue };
            let is_root = node.parent.is_none();
            if node.kind != crate::fs::node::NodeKind::Dir
                || (!include_hidden && node.is_hidden() && !is_root)
            {
                continue;
            }
            if node.children.is_none() {
                self.load_children(h);
            }
            queue.extend(self.children(h).iter().copied());
        }
        true
    }

    /// paths of all expanded directories, parents before children.
    /// used to carry the expanded state over into a new tree (e.g. `cd ..`).
    pub fn expanded_paths(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let mut stack: Vec<Handle> = self.roots.iter().rev().copied().collect();
        while let Some(h) = stack.pop() {
            let Some(node) = self.get(h) else { continue };
            if node.expanded {
                out.push(node.path.clone());
                stack.extend(self.children(h).iter().rev());
            }
        }
        out
    }

    /// finds an already loaded node by its path
    pub fn find(&self, path: &Path) -> Option<Handle> {
        self.roots.iter().find_map(|root| {
            let root_path = self.path(*root)?;
            let rest = path.strip_prefix(root_path).ok()?;
            let mut current = *root;
            for component in rest.components() {
                current = *self.children(current).iter().find(|c| {
                    self.get(**c)
                        .is_some_and(|n| component.as_os_str() == n.name.as_str())
                })?;
            }
            Some(current)
        })
    }

    /// like `find`, but loads and expands every directory on the way so the
    /// node becomes visible
    pub fn reveal(&mut self, path: &Path) -> Option<Handle> {
        let root = self.roots.iter().copied().find(|root| {
            self.path(*root)
                .is_some_and(|root_path| path.starts_with(root_path))
        })?;
        let rest = path.strip_prefix(self.path(root)?).ok()?.to_path_buf();

        let mut current = root;
        for component in rest.components() {
            self.expand(current);
            current = *self.children(current).iter().find(|c| {
                self.get(**c)
                    .is_some_and(|n| component.as_os_str() == n.name.as_str())
            })?;
        }
        Some(current)
    }
}

/// paths of all entries of a directory
fn read_dir_sorted(path: &Path) -> io::Result<Vec<PathBuf>> {
    let mut entries = std::fs::read_dir(path)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect::<Vec<_>>();
    // a stable order keeps the view stable even when the user sorts by
    // something that has a lot of ties (size, extension)
    entries.sort();
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use pretty_assertions::assert_eq;
    use tempfile::TempDir;

    use super::*;

    fn names(tree: &FileTree, handle: Handle) -> Vec<String> {
        tree.children(handle)
            .iter()
            .map(|h| tree.get(*h).unwrap().name.clone())
            .collect()
    }

    fn fixture() -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("a/b")).unwrap();
        fs::write(dir.path().join("a/b/c.txt"), "c").unwrap();
        fs::write(dir.path().join("a/x.rs"), "x").unwrap();
        fs::write(dir.path().join("z.md"), "z").unwrap();
        dir
    }

    #[test]
    fn loads_lazily() {
        let dir = fixture();
        let tree = FileTree::with_root(dir.path()).unwrap();
        let root = tree.roots()[0];
        assert_eq!(names(&tree, root), vec!["a", "z.md"]);

        let a = tree.children(root)[0];
        assert!(tree.get(a).unwrap().children.is_none());
        // root + a + z.md
        assert_eq!(tree.len(), 3);
    }

    #[test]
    fn find_and_reveal() {
        let dir = fixture();
        let mut tree = FileTree::with_root(dir.path()).unwrap();
        let root_path = tree.path(tree.roots()[0]).unwrap().to_path_buf();
        let target = root_path.join("a/b/c.txt");

        assert_eq!(tree.find(&target), None);
        let h = tree.reveal(&target).unwrap();
        assert_eq!(tree.find(&target), Some(h));
        assert_eq!(tree.get(h).unwrap().name, "c.txt");

        let b = tree.parent(h).unwrap();
        assert!(tree.get(b).unwrap().expanded);
    }

    #[test]
    fn refresh_keeps_handles_and_state() {
        let dir = fixture();
        let mut tree = FileTree::with_root(dir.path()).unwrap();
        let root = tree.roots()[0];
        let root_path = tree.path(root).unwrap().to_path_buf();
        let a = tree.reveal(&root_path.join("a")).unwrap();
        tree.expand(a);

        fs::write(root_path.join("new.txt"), "").unwrap();
        fs::remove_file(root_path.join("z.md")).unwrap();
        fs::write(root_path.join("a/y.rs"), "").unwrap();
        tree.refresh(root);

        assert_eq!(names(&tree, root), vec!["a", "new.txt"]);
        assert_eq!(tree.find(&root_path.join("a")), Some(a));
        assert!(tree.get(a).unwrap().expanded);
        assert_eq!(names(&tree, a), vec!["b", "x.rs", "y.rs"]);
        assert_eq!(tree.find(&root_path.join("z.md")), None);
    }

    #[test]
    fn refresh_frees_removed_subtrees() {
        let dir = fixture();
        let mut tree = FileTree::with_root(dir.path()).unwrap();
        let root = tree.roots()[0];
        let root_path = tree.path(root).unwrap().to_path_buf();
        tree.reveal(&root_path.join("a/b/c.txt")).unwrap();
        // root, a, z.md, b, x.rs, c.txt
        assert_eq!(tree.len(), 6);

        fs::remove_dir_all(root_path.join("a")).unwrap();
        tree.refresh(root);
        assert_eq!(tree.len(), 2);
    }

    #[test]
    fn unreadable_dir_stores_error() {
        let dir = fixture();
        let mut tree = FileTree::with_root(dir.path()).unwrap();
        let root = tree.roots()[0];
        let a = tree.children(root)[0];
        fs::remove_dir_all(dir.path().join("a")).unwrap();

        tree.expand(a);
        let node = tree.get(a).unwrap();
        assert!(node.error.is_some());
        assert_eq!(node.children.as_deref(), Some(&[][..]));
    }

    #[test]
    fn multiple_roots() {
        let one = fixture();
        let two = fixture();
        let mut tree = FileTree::new();
        tree.add_root(one.path()).unwrap();
        let r2 = tree.add_root(two.path()).unwrap();
        let target = std::fs::canonicalize(two.path()).unwrap().join("a/x.rs");

        let h = tree.reveal(&target).unwrap();
        let a = tree.parent(h).unwrap();
        assert_eq!(tree.parent(a), Some(r2));

        tree.remove_root(r2);
        assert_eq!(tree.roots().len(), 1);
        assert_eq!(tree.find(&target), None);
    }
}
