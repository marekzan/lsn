use std::{cmp::Ordering, collections::BTreeSet, path::PathBuf};

use crate::{
    arena::Handle,
    fs::{node::Node, tree::FileTree},
};

/// the view-model: a flat list of the nodes that are currently visible.
///
/// it is rebuilt from the tree whenever the tree, the filter or the sort order
/// changes. everything the ui and the key handling needs (cursor, scrolling,
/// "node under cursor") works on this list in O(1) instead of walking the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewItem {
    pub handle: Handle,
    pub depth: usize,
    /// one entry per ancestor level: `true` if that ancestor still has
    /// siblings below it. used to draw the tree guides (`│`, `├─`, `└─`).
    pub guides: Vec<bool>,
    pub is_last: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filter {
    pub show_hidden: bool,
    pub show_dirs: bool,
    pub show_files: bool,
    pub show_symlinks: bool,
    /// only show marked entries (and the content of marked directories)
    pub only_marked: bool,
    /// substring search on the name, smartcase
    pub query: String,
}

impl Default for Filter {
    fn default() -> Self {
        Self {
            show_hidden: false,
            show_dirs: true,
            show_files: true,
            show_symlinks: true,
            only_marked: false,
            query: String::new(),
        }
    }
}

impl Filter {
    /// filters that also look into collapsed directories, so a match deep in
    /// the tree becomes visible together with its parents
    fn reveals(&self) -> bool {
        self.only_marked || !self.query.is_empty()
    }

    fn matches_query(&self, node: &Node) -> bool {
        if self.query.is_empty() {
            return true;
        }
        if self.query.chars().any(char::is_uppercase) {
            node.name.contains(&self.query)
        } else {
            node.name
                .to_lowercase()
                .contains(&self.query.to_lowercase())
        }
    }

    fn matches_kind(&self, node: &Node) -> bool {
        match node.kind {
            crate::fs::node::NodeKind::Dir => self.show_dirs,
            crate::fs::node::NodeKind::File => self.show_files,
            crate::fs::node::NodeKind::Symlink { .. } => self.show_symlinks,
        }
    }

    /// short description of the active filters for the status bar
    pub fn describe(&self) -> Option<String> {
        let mut parts = Vec::new();
        if self.show_hidden {
            parts.push("hidden".to_string());
        }
        let kinds: String = [
            (self.show_dirs, 'd'),
            (self.show_files, 'f'),
            (self.show_symlinks, 'l'),
        ]
        .iter()
        .filter(|(on, _)| *on)
        .map(|(_, c)| *c)
        .collect();
        if kinds.len() != 3 {
            parts.push(format!("only {kinds}"));
        }
        if self.only_marked {
            parts.push("marked".to_string());
        }
        if !self.query.is_empty() {
            parts.push(format!("/{}", self.query));
        }
        (!parts.is_empty()).then(|| parts.join(" "))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortBy {
    #[default]
    Name,
    Size,
    Modified,
    Extension,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sort {
    pub by: SortBy,
    pub dirs_first: bool,
    pub reverse: bool,
}

impl Default for Sort {
    fn default() -> Self {
        Self {
            by: SortBy::Name,
            dirs_first: true,
            reverse: false,
        }
    }
}

impl Sort {
    fn compare(&self, a: &Node, b: &Node) -> Ordering {
        if self.dirs_first {
            match (a.is_dir(), b.is_dir()) {
                (true, false) => return Ordering::Less,
                (false, true) => return Ordering::Greater,
                _ => {}
            }
        }
        let by_name = || natural_cmp(&a.name, &b.name);
        // the size of a directory entry itself means nothing to the user
        let size = |n: &Node| if n.is_dir() { 0 } else { n.meta.size };
        let ord = match self.by {
            SortBy::Name => by_name(),
            // biggest / newest first feels more natural for these
            SortBy::Size => size(b).cmp(&size(a)).then_with(by_name),
            SortBy::Modified => b.meta.modified.cmp(&a.meta.modified).then_with(by_name),
            SortBy::Extension => a
                .extension()
                .to_lowercase()
                .cmp(&b.extension().to_lowercase())
                .then_with(by_name),
        };
        if self.reverse { ord.reverse() } else { ord }
    }

    pub fn describe(&self) -> String {
        let by = match self.by {
            SortBy::Name => "name",
            SortBy::Size => "size",
            SortBy::Modified => "modified",
            SortBy::Extension => "ext",
        };
        format!("{by}{}", if self.reverse { " ↓" } else { "" })
    }
}

/// case insensitive compare that orders numbers by value (`file2` < `file10`)
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut a = a.chars().peekable();
    let mut b = b.chars().peekable();
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut s = String::new();
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        s.push(c);
                        it.next();
                    }
                    s
                };
                let (na, nb) = (take(&mut a), take(&mut b));
                let (ta, tb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
                let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(x), Some(y)) => {
                let ord = x.to_lowercase().cmp(y.to_lowercase());
                if ord != Ordering::Equal {
                    return ord;
                }
                a.next();
                b.next();
            }
        }
    }
}

/// everything outside of the tree that decides what is visible
pub struct ViewContext<'a> {
    pub filter: &'a Filter,
    pub sort: &'a Sort,
    pub marked: &'a BTreeSet<PathBuf>,
}

/// flattens the visible part of the tree.
///
/// `include_roots`: a normal tab shows the content of its root, the pins tab
/// shows the pinned folders themselves.
pub fn build(tree: &FileTree, ctx: &ViewContext, include_roots: bool) -> Vec<ViewItem> {
    let mut out = Vec::new();
    let mut guides = Vec::new();
    let roots: Vec<Handle> = tree.roots().to_vec();
    if include_roots {
        walk_siblings(tree, ctx, &roots, false, &mut guides, &mut out);
    } else {
        for root in roots {
            let marked = tree
                .get(root)
                .is_some_and(|n| is_marked_or_inside(ctx.marked, &n.path));
            let children = tree.children(root).to_vec();
            walk_siblings(tree, ctx, &children, marked, &mut guides, &mut out);
        }
    }
    out
}

fn is_marked_or_inside(marked: &BTreeSet<PathBuf>, path: &std::path::Path) -> bool {
    path.ancestors().any(|p| marked.contains(p))
}

/// emits the visible nodes of one level (and below) and returns whether
/// anything was emitted
fn walk_siblings(
    tree: &FileTree,
    ctx: &ViewContext,
    siblings: &[Handle],
    parent_marked: bool,
    guides: &mut Vec<bool>,
    out: &mut Vec<ViewItem>,
) -> bool {
    let mut sorted: Vec<(Handle, &Node)> = siblings
        .iter()
        .filter_map(|h| tree.get(*h).map(|n| (*h, n)))
        .filter(|(_, n)| ctx.filter.show_hidden || !n.is_hidden())
        .collect();
    sorted.sort_by(|(_, a), (_, b)| ctx.sort.compare(a, b));

    // every sibling renders into its own buffer first, because we only know
    // which one is the last visible sibling after all of them were filtered
    let mut emitted: Vec<Vec<ViewItem>> = Vec::new();
    for (handle, node) in sorted {
        let marked = parent_marked || ctx.marked.contains(&node.path);
        let mut own = vec![ViewItem {
            handle,
            depth: guides.len(),
            guides: guides.clone(),
            is_last: false,
        }];

        // placeholder, the real value is patched in below once we know
        // whether this node is the last visible sibling
        guides.push(true);
        let descend = node.is_dir() && (node.expanded || ctx.filter.reveals());
        let has_children =
            descend && walk_siblings(tree, ctx, tree.children(handle), marked, guides, &mut own);
        guides.pop();

        let matches_self = ctx.filter.matches_kind(node)
            && ctx.filter.matches_query(node)
            && (!ctx.filter.only_marked || marked);

        // directories without a match themselves stay as context for their
        // matching descendants
        if matches_self || has_children {
            emitted.push(own);
        }
    }

    let count = emitted.len();
    let level = guides.len();
    for (i, mut items) in emitted.into_iter().enumerate() {
        let last = i + 1 == count;
        items[0].is_last = last;
        // children of a last sibling don't need a `│` at this level
        for child in items.iter_mut().skip(1) {
            child.guides[level] = !last;
        }
        out.extend(items);
    }
    count > 0
}

#[cfg(test)]
mod tests {
    use std::fs;

    use pretty_assertions::assert_eq;
    use tempfile::TempDir;

    use super::*;

    fn fixture() -> (TempDir, FileTree) {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src/deep")).unwrap();
        fs::write(dir.path().join("src/deep/needle.rs"), "").unwrap();
        fs::write(dir.path().join("src/main.rs"), "").unwrap();
        fs::write(dir.path().join("file10.txt"), "1234567890").unwrap();
        fs::write(dir.path().join("file2.txt"), "12").unwrap();
        fs::write(dir.path().join(".hidden"), "").unwrap();
        let tree = FileTree::with_root(dir.path()).unwrap();
        (dir, tree)
    }

    fn render(tree: &FileTree, items: &[ViewItem]) -> Vec<String> {
        items
            .iter()
            .map(|i| {
                let mut s: String = i
                    .guides
                    .iter()
                    .map(|g| if *g { "│ " } else { "  " })
                    .collect();
                s.push_str(if i.is_last { "└ " } else { "├ " });
                s.push_str(&tree.get(i.handle).unwrap().name);
                s
            })
            .collect()
    }

    fn view(
        tree: &FileTree,
        filter: &Filter,
        sort: &Sort,
        marked: &BTreeSet<PathBuf>,
    ) -> Vec<String> {
        let ctx = ViewContext {
            filter,
            sort,
            marked,
        };
        render(tree, &build(tree, &ctx, false))
    }

    #[test]
    fn default_view_is_sorted_and_hides_dotfiles() {
        let (_dir, tree) = fixture();
        let lines = view(
            &tree,
            &Filter::default(),
            &Sort::default(),
            &BTreeSet::new(),
        );
        assert_eq!(lines, vec!["├ src", "├ file2.txt", "└ file10.txt"]);
    }

    #[test]
    fn expanded_dirs_show_children_with_guides() {
        let (dir, mut tree) = fixture();
        let root = tree.path(tree.roots()[0]).unwrap().to_path_buf();
        tree.reveal(&root.join("src/deep/needle.rs"));
        let lines = view(
            &tree,
            &Filter::default(),
            &Sort::default(),
            &BTreeSet::new(),
        );
        assert_eq!(
            lines,
            vec![
                "├ src",
                "│ ├ deep",
                "│ │ └ needle.rs",
                "│ └ main.rs",
                "├ file2.txt",
                "└ file10.txt",
            ]
        );
        drop(dir);
    }

    #[test]
    fn query_reveals_loaded_matches_in_collapsed_dirs() {
        let (_dir, mut tree) = fixture();
        let root = tree.path(tree.roots()[0]).unwrap().to_path_buf();
        let deep = tree.reveal(&root.join("src/deep/needle.rs")).unwrap();
        let src = tree.parent(tree.parent(deep).unwrap()).unwrap();
        tree.collapse_all(src);

        let filter = Filter {
            query: "NEEDLE".to_lowercase(),
            ..Filter::default()
        };
        let lines = view(&tree, &filter, &Sort::default(), &BTreeSet::new());
        assert_eq!(lines, vec!["└ src", "  └ deep", "    └ needle.rs"]);
    }

    #[test]
    fn smartcase() {
        let (_dir, tree) = fixture();
        let filter = Filter {
            query: "File".into(),
            ..Filter::default()
        };
        assert!(view(&tree, &filter, &Sort::default(), &BTreeSet::new()).is_empty());
    }

    #[test]
    fn kind_filter_keeps_parents_as_context() {
        let (_dir, mut tree) = fixture();
        let root = tree.path(tree.roots()[0]).unwrap().to_path_buf();
        tree.reveal(&root.join("src/main.rs"));
        let filter = Filter {
            show_files: false,
            ..Filter::default()
        };
        let lines = view(&tree, &filter, &Sort::default(), &BTreeSet::new());
        // src/deep is a dir and src is shown, files are gone
        assert_eq!(lines, vec!["└ src", "  └ deep"]);
    }

    #[test]
    fn only_marked() {
        let (_dir, mut tree) = fixture();
        let root = tree.path(tree.roots()[0]).unwrap().to_path_buf();
        tree.reveal(&root.join("src/main.rs"));
        let marked = BTreeSet::from([root.join("src/deep"), root.join("file2.txt")]);
        let filter = Filter {
            only_marked: true,
            ..Filter::default()
        };
        let lines = view(&tree, &filter, &Sort::default(), &marked);
        assert_eq!(lines, vec!["├ src", "│ └ deep", "└ file2.txt"]);
    }

    #[test]
    fn sort_by_size_and_reverse() {
        let (_dir, tree) = fixture();
        let sort = Sort {
            by: SortBy::Size,
            dirs_first: false,
            reverse: false,
        };
        let lines = view(&tree, &Filter::default(), &sort, &BTreeSet::new());
        assert_eq!(lines[0], "├ file10.txt");

        let sort = Sort {
            by: SortBy::Name,
            dirs_first: false,
            reverse: true,
        };
        let lines = view(&tree, &Filter::default(), &sort, &BTreeSet::new());
        assert_eq!(lines, vec!["├ src", "├ file10.txt", "└ file2.txt"]);
    }

    #[test]
    fn natural_order() {
        assert_eq!(natural_cmp("a2", "a10"), Ordering::Less);
        assert_eq!(natural_cmp("A", "b"), Ordering::Less);
        assert_eq!(natural_cmp("x01", "x1"), Ordering::Equal);
    }
}
