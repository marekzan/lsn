use crate::{arena::Handle, input::Input, tab::Tab, view};

/// quick navigation, level by level.
///
/// one level of the tree is shown flat and filtered by what the user types.
/// `enter` on a directory shows its content as the next level, `enter` on a
/// file ends the jump with the cursor on it.
///
/// the jump only looks at the tree, it never changes the real view. only when
/// it ends the result is revealed in the tab.
pub struct Jump {
    /// the directory whose content is shown. `None` is the level of the roots
    /// in the pins tab.
    pub dir: Option<Handle>,
    pub input: Input,
    pub matches: Vec<Handle>,
    pub cursor: usize,
}

/// what the app has to do after a jump step
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    /// still jumping
    Continue,
    /// the jump ended on this node (a file, or the directory we stopped in)
    Done(Option<Handle>),
}

/// where a jump starts
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
    /// the top level of the tab
    Root,
    /// the level of the entry under the cursor
    Here,
}

impl Jump {
    pub fn new(tab: &mut Tab, start: Start) -> Self {
        // the top level of a normal tab is the content of its root, the top
        // level of the pins tab are the roots themselves
        let top = tab
            .tree
            .roots()
            .first()
            .copied()
            .filter(|_| tab.root_path().is_some());
        let dir = match (start, tab.current()) {
            (Start::Here, Some(h)) => tab.tree.parent(h),
            _ => top,
        };
        let mut jump = Self {
            dir,
            input: Input::default(),
            matches: Vec::new(),
            cursor: 0,
        };
        jump.update(tab);
        jump
    }

    /// entries of the current level that match the input, sorted like the tab
    fn candidates(&self, tab: &mut Tab, query: &str) -> Vec<Handle> {
        let level = match self.dir {
            Some(dir) => {
                if tab.tree.get(dir).is_some_and(|n| n.children.is_none()) {
                    tab.tree.load_children(dir);
                }
                tab.tree.children(dir).to_vec()
            }
            None => tab.tree.roots().to_vec(),
        };
        let mut matches: Vec<Handle> = level
            .into_iter()
            .filter(|h| {
                tab.tree.get(*h).is_some_and(|n| {
                    (tab.filter.show_hidden || !n.is_hidden() || query.starts_with('.'))
                        && (query.is_empty() || view::match_range(query, &n.name).is_some())
                })
            })
            .collect();
        matches.sort_by(|a, b| {
            let (a, b) = (tab.tree.get(*a).unwrap(), tab.tree.get(*b).unwrap());
            tab.sort.compare(a, b)
        });
        matches
    }

    pub fn update(&mut self, tab: &mut Tab) {
        let query = self.input.text().to_string();
        self.matches = self.candidates(tab, &query);
        // prefer an entry that starts with the input, that is usually the one
        // the user is aiming for
        let lower = query.to_lowercase();
        self.cursor = self
            .matches
            .iter()
            .position(|h| {
                tab.tree
                    .get(*h)
                    .is_some_and(|n| n.name.to_lowercase().starts_with(&lower))
            })
            .unwrap_or(0);
    }

    pub fn current(&self) -> Option<Handle> {
        self.matches.get(self.cursor).copied()
    }

    pub fn move_cursor(&mut self, delta: isize) {
        let last = self.matches.len().saturating_sub(1) as isize;
        self.cursor = (self.cursor as isize + delta).clamp(0, last.max(0)) as usize;
    }

    /// enters a directory or ends the jump on a file
    pub fn accept(&mut self, tab: &mut Tab, handle: Handle) -> Step {
        match tab.tree.get(handle) {
            Some(node) if node.is_dir() => {
                self.dir = Some(handle);
                self.input = Input::default();
                self.update(tab);
                Step::Continue
            }
            Some(_) => Step::Done(Some(handle)),
            None => Step::Continue,
        }
    }

    /// one level up, but never above the root of a normal tab
    pub fn up(&mut self, tab: &mut Tab) {
        let Some(dir) = self.dir else { return };
        let parent = tab.tree.parent(dir);
        if parent.is_none() && tab.root_path().is_some() {
            return;
        }
        self.dir = parent;
        self.input = Input::default();
        self.update(tab);
        // keep the cursor on the directory we came from
        if let Some(pos) = self.matches.iter().position(|h| *h == dir) {
            self.cursor = pos;
        }
    }

    /// the user is done: stay in the directory the jump is in
    pub fn finish(&self, tab: &Tab) -> Step {
        let is_tab_root =
            self.dir.is_some_and(|d| tab.tree.roots().contains(&d)) && tab.root_path().is_some();
        Step::Done(self.dir.filter(|_| !is_tab_root))
    }
}
