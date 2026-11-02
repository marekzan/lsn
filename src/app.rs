use std::{
    collections::BTreeSet,
    io,
    path::{Path, PathBuf},
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    action::Action,
    fs::ops::{self, TransferKind},
    input::Input,
    jump::{Jump, Step},
    keymap,
    pins::Pins,
    tab::{Tab, TabKind},
};

/// how many entries a search may load from disk before it gives up
const SEARCH_BUDGET: usize = 20_000;

pub enum Mode {
    Normal,
    /// waiting for the second key of `f` / `s`
    Menu(Menu),
    Prompt(Prompt),
    /// waiting for `y` to delete these paths
    ConfirmDelete(Vec<PathBuf>),
    /// the help screen, scrolled down by this many lines
    Help(u16),
    /// quick navigation level by level, see `jump.rs`
    Jump(Jump),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Menu {
    Filter,
    Sort,
}

pub struct Prompt {
    pub kind: PromptKind,
    pub input: Input,
}

pub enum PromptKind {
    /// live search, `previous` is restored on escape
    Search {
        previous: String,
    },
    Rename(PathBuf),
    /// create a file or directory in this directory
    Create(PathBuf),
    Chmod(Vec<PathBuf>),
}

impl PromptKind {
    pub fn label(&self) -> &'static str {
        match self {
            PromptKind::Search { .. } => "/",
            PromptKind::Rename(_) => "rename: ",
            PromptKind::Create(_) => "new (dir/ for dir): ",
            PromptKind::Chmod(_) => "chmod: ",
        }
    }
}

pub struct Clipboard {
    pub paths: BTreeSet<PathBuf>,
    pub kind: TransferKind,
}

pub struct Status {
    pub text: String,
    pub error: bool,
}

/// the whole application state.
///
/// the app never touches the terminal, it only reacts to keys and actions.
/// rendering lives in `ui`, the event loop in `main`.
pub struct App {
    pub tabs: Vec<Tab>,
    pub active: usize,
    /// marks are global, so things can be marked in several tabs at once
    pub marked: BTreeSet<PathBuf>,
    pub clipboard: Option<Clipboard>,
    pub pins: Pins,
    pub mode: Mode,
    pub status: Option<Status>,
    pub show_meta: bool,
    pub should_quit: bool,
    /// paths printed to stdout after the ui is gone
    pub output: Vec<PathBuf>,
    /// height of the tree area, set by the ui, used for page movements
    pub page: usize,
}

impl App {
    pub fn new(paths: &[PathBuf], pins: Pins) -> io::Result<Self> {
        let tabs = paths
            .iter()
            .map(|p| Tab::new_dir(p))
            .collect::<io::Result<Vec<_>>>()?;
        Ok(Self {
            tabs,
            active: 0,
            marked: BTreeSet::new(),
            clipboard: None,
            pins,
            mode: Mode::Normal,
            status: None,
            show_meta: true,
            should_quit: false,
            output: Vec::new(),
            page: 10,
        })
    }

    pub fn tab(&self) -> &Tab {
        &self.tabs[self.active]
    }

    pub fn tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active]
    }

    /// brings the view list of the active tab up to date
    pub fn prepare(&mut self) {
        let tab = &mut self.tabs[self.active];
        if tab.stale {
            tab.rebuild(&self.marked);
        }
    }

    fn invalidate_all(&mut self) {
        for tab in &mut self.tabs {
            tab.stale = true;
        }
    }

    fn info(&mut self, text: impl Into<String>) {
        self.status = Some(Status {
            text: text.into(),
            error: false,
        });
    }

    fn error(&mut self, text: impl Into<String>) {
        self.status = Some(Status {
            text: text.into(),
            error: true,
        });
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        self.prepare();
        if !matches!(self.mode, Mode::Prompt(_)) {
            self.status = None;
        }

        match std::mem::replace(&mut self.mode, Mode::Normal) {
            Mode::Normal => {
                if let Some(action) = keymap::lookup(keymap::NORMAL, key) {
                    self.apply(action);
                }
            }
            Mode::Menu(menu) => {
                let table = match menu {
                    Menu::Filter => keymap::FILTER_MENU,
                    Menu::Sort => keymap::SORT_MENU,
                };
                // any other key just closes the menu
                if let Some(action) = keymap::lookup(table, key) {
                    self.apply(action);
                }
            }
            Mode::Help(scroll) => match key.code {
                KeyCode::Char('j') | KeyCode::Down => self.mode = Mode::Help(scroll + 1),
                KeyCode::Char('k') | KeyCode::Up => {
                    self.mode = Mode::Help(scroll.saturating_sub(1))
                }
                _ => {}
            },
            Mode::ConfirmDelete(paths) => {
                if matches!(key.code, KeyCode::Char('y' | 'Y')) {
                    self.delete(paths);
                } else {
                    self.info("delete cancelled");
                }
            }
            Mode::Prompt(prompt) => self.handle_prompt(prompt, key),
            Mode::Jump(jump) => self.handle_jump(jump, key),
        }
        self.prepare();
    }

    fn handle_jump(&mut self, mut jump: Jump, key: KeyEvent) {
        let tab = &mut self.tabs[self.active];
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let step = match key.code {
            KeyCode::Esc => jump.finish(tab),
            KeyCode::Enter => match jump.current() {
                Some(h) => jump.accept(tab, h),
                None => Step::Continue,
            },
            KeyCode::Up | KeyCode::BackTab => {
                jump.move_cursor(-1);
                Step::Continue
            }
            KeyCode::Char('p') if ctrl => {
                jump.move_cursor(-1);
                Step::Continue
            }
            KeyCode::Down | KeyCode::Tab => {
                jump.move_cursor(1);
                Step::Continue
            }
            KeyCode::Char('n') if ctrl => {
                jump.move_cursor(1);
                Step::Continue
            }
            KeyCode::Backspace if jump.input.text().is_empty() => {
                jump.up(tab);
                Step::Continue
            }
            _ => {
                if jump.input.handle(key) {
                    jump.update(tab);
                }
                Step::Continue
            }
        };
        self.after_jump_step(jump, step);
    }

    fn after_jump_step(&mut self, jump: Jump, step: Step) {
        match step {
            Step::Continue => self.mode = Mode::Jump(jump),
            Step::Done(None) => {}
            Step::Done(Some(target)) => {
                let tab = &mut self.tabs[self.active];
                let Some(path) = tab.tree.path(target).map(Path::to_path_buf) else {
                    return;
                };
                if tab.tree.get(target).is_some_and(|n| n.is_dir()) {
                    tab.tree.expand(target);
                }
                tab.reveal(&path, &self.marked);
            }
        }
    }

    fn handle_prompt(&mut self, mut prompt: Prompt, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                if let PromptKind::Search { previous } = prompt.kind {
                    self.tab_mut().filter.query = previous;
                    self.tab_mut().stale = true;
                }
            }
            KeyCode::Enter => self.submit(prompt),
            // moving through the results while searching
            KeyCode::Up | KeyCode::Down if matches!(prompt.kind, PromptKind::Search { .. }) => {
                self.tab_mut()
                    .move_cursor(if key.code == KeyCode::Up { -1 } else { 1 });
                self.mode = Mode::Prompt(prompt);
            }
            _ => {
                if prompt.input.handle(key) && matches!(prompt.kind, PromptKind::Search { .. }) {
                    let tab = self.tab_mut();
                    tab.filter.query = prompt.input.text().to_string();
                    tab.stale = true;
                    tab.cursor = 0;
                }
                self.mode = Mode::Prompt(prompt);
            }
        }
    }

    fn submit(&mut self, prompt: Prompt) {
        let text = prompt.input.text().to_string();
        match prompt.kind {
            PromptKind::Search { .. } => {}
            PromptKind::Rename(path) => match ops::rename(&path, &text) {
                Ok(new) => {
                    if self.marked.remove(&path) {
                        self.marked.insert(new.clone());
                    }
                    self.after_change(path.parent().into_iter().map(Path::to_path_buf));
                    self.reveal(&new);
                    self.info(format!("renamed to {text}"));
                }
                Err(err) => self.error(format!("rename failed: {err}")),
            },
            PromptKind::Create(dir) => match ops::create(&dir, &text) {
                Ok(new) => {
                    self.after_change([dir]);
                    self.reveal(&new);
                    self.info(format!("created {text}"));
                }
                Err(err) => self.error(format!("create failed: {err}")),
            },
            PromptKind::Chmod(paths) => {
                let mut failed = 0;
                for path in &paths {
                    if let Err(err) = ops::chmod(path, &text) {
                        failed += 1;
                        self.error(format!("chmod failed: {err}"));
                    }
                }
                self.after_change(parents(&paths));
                if failed == 0 {
                    self.info(format!("chmod {text} on {}", count(paths.len())));
                }
            }
        }
    }

    pub fn apply(&mut self, action: Action) {
        self.prepare();
        let page = self.page.max(2) as isize;
        match action {
            Action::Down => self.tab_mut().move_cursor(1),
            Action::Up => self.tab_mut().move_cursor(-1),
            Action::Top => self.tab_mut().cursor = 0,
            Action::Bottom => self.tab_mut().move_cursor(isize::MAX / 2),
            Action::HalfPageDown => self.tab_mut().move_cursor(page / 2),
            Action::HalfPageUp => self.tab_mut().move_cursor(-page / 2),
            Action::Expand => self.expand(),
            Action::Collapse => self.collapse(),
            Action::Open => match self.tab().current_node() {
                Some(node) if node.is_dir() => {
                    let h = self.tab().current().expect("node exists");
                    self.tab_mut().tree.toggle(h);
                    self.tab_mut().stale = true;
                }
                Some(_) => self.apply(Action::Pick),
                None => {}
            },
            Action::CollapseAll => self.collapse_all(),
            Action::RootUp => self.root_up(),
            Action::RootDown => {
                if let Some(dir) = self.current_dir() {
                    self.open_root(&dir);
                }
            }
            Action::RootHome => {
                if let Some(home) = std::env::var_os("HOME") {
                    self.open_root(Path::new(&home));
                }
            }

            Action::ToggleMark => {
                if let Some(path) = self.tab().current_path() {
                    if !self.marked.remove(&path) {
                        self.marked.insert(path);
                    }
                    self.invalidate_all();
                    self.tab_mut().move_cursor(1);
                }
            }
            Action::InvertMarks => {
                let tab = self.tab();
                let paths: Vec<PathBuf> = tab
                    .items
                    .iter()
                    .filter_map(|i| tab.tree.path(i.handle).map(Path::to_path_buf))
                    .collect();
                for path in paths {
                    if !self.marked.remove(&path) {
                        self.marked.insert(path);
                    }
                }
                self.invalidate_all();
            }
            Action::ClearMarks => {
                self.marked.clear();
                self.invalidate_all();
            }

            Action::Yank => self.fill_clipboard(TransferKind::Copy),
            Action::Cut => self.fill_clipboard(TransferKind::Move),
            Action::Paste => self.paste(),
            Action::Delete => {
                let targets = self.targets();
                if !targets.is_empty() {
                    self.mode = Mode::ConfirmDelete(targets);
                }
            }
            Action::Rename => {
                if let Some(node) = self.tab().current_node() {
                    let (path, name) = (node.path.clone(), node.name.clone());
                    self.prompt(PromptKind::Rename(path), &name);
                }
            }
            Action::Create => {
                if let Some(dir) = self.tab().target_dir() {
                    self.prompt(PromptKind::Create(dir), "");
                }
            }
            Action::Chmod => {
                let targets = self.targets();
                let mode = self
                    .tab()
                    .current_node()
                    .map(|n| format!("{:o}", n.meta.mode & 0o7777))
                    .unwrap_or_default();
                if !targets.is_empty() {
                    self.prompt(PromptKind::Chmod(targets), &mode);
                }
            }

            Action::Search => {
                let tab = self.tab_mut();
                let complete = tab.tree.load_deep(SEARCH_BUDGET, tab.filter.show_hidden);
                if !complete {
                    self.info(format!(
                        "large tree: search covers the first {SEARCH_BUDGET} entries"
                    ));
                }
                let previous = self.tab().filter.query.clone();
                self.prompt(
                    PromptKind::Search {
                        previous: previous.clone(),
                    },
                    &previous,
                );
            }
            Action::Jump(start) => {
                let jump = Jump::new(&mut self.tabs[self.active], start);
                self.mode = Mode::Jump(jump);
            }
            Action::FilterMenu => self.mode = Mode::Menu(Menu::Filter),
            Action::SortMenu => self.mode = Mode::Menu(Menu::Sort),
            Action::ToggleHidden => self.filter(|f| f.show_hidden = !f.show_hidden),
            Action::ToggleDirs => self.filter(|f| f.show_dirs = !f.show_dirs),
            Action::ToggleFiles => self.filter(|f| f.show_files = !f.show_files),
            Action::ToggleSymlinks => self.filter(|f| f.show_symlinks = !f.show_symlinks),
            Action::ToggleOnlyMarked => self.filter(|f| f.only_marked = !f.only_marked),
            Action::ResetFilter => self.filter(|f| *f = Default::default()),
            Action::SortBy(by) => self.sort(|s| s.by = by),
            Action::ToggleDirsFirst => self.sort(|s| s.dirs_first = !s.dirs_first),
            Action::ToggleReverse => self.sort(|s| s.reverse = !s.reverse),
            Action::ToggleMeta => self.show_meta = !self.show_meta,
            Action::Refresh => {
                for tab in &mut self.tabs {
                    tab.tree.refresh_all();
                }
                self.prune_marks();
                self.invalidate_all();
                self.info("reloaded");
            }

            Action::NewTab => {
                if let Some(dir) = self.current_dir().or_else(|| self.tab().target_dir()) {
                    self.new_tab(&dir);
                }
            }
            Action::CloseTab => {
                if self.tabs.len() == 1 {
                    self.info("this is the last tab, quit with q");
                } else {
                    self.tabs.remove(self.active);
                    self.active = self.active.min(self.tabs.len() - 1);
                }
            }
            Action::NextTab => self.active = (self.active + 1) % self.tabs.len(),
            Action::PrevTab => self.active = (self.active + self.tabs.len() - 1) % self.tabs.len(),
            Action::GoToTab(i) => {
                if i < self.tabs.len() {
                    self.active = i;
                }
            }
            Action::TogglePin => self.toggle_pin(),
            Action::OpenPins => self.open_pins(),

            Action::Pick => {
                self.output = self.targets();
                self.should_quit = !self.output.is_empty();
            }
            Action::PickDir => {
                self.output = self.tab().target_dir().into_iter().collect();
                self.should_quit = true;
            }
            Action::Help => self.mode = Mode::Help(0),
            Action::Escape => {
                if !self.tab().filter.query.is_empty() {
                    self.filter(|f| f.query.clear());
                } else if !self.marked.is_empty() {
                    self.apply(Action::ClearMarks);
                } else if self.clipboard.is_some() {
                    self.clipboard = None;
                } else {
                    self.should_quit = true;
                }
            }
            Action::Quit => self.should_quit = true,
        }
    }

    fn reveal(&mut self, path: &Path) {
        self.tabs[self.active].reveal(path, &self.marked);
    }

    fn prompt(&mut self, kind: PromptKind, text: &str) {
        self.mode = Mode::Prompt(Prompt {
            kind,
            input: Input::new(text),
        });
    }

    fn filter(&mut self, f: impl FnOnce(&mut crate::view::Filter)) {
        let tab = self.tab_mut();
        f(&mut tab.filter);
        tab.stale = true;
    }

    fn sort(&mut self, f: impl FnOnce(&mut crate::view::Sort)) {
        let tab = self.tab_mut();
        f(&mut tab.sort);
        tab.stale = true;
    }

    /// what an operation works on: the marks, or the entry under the cursor
    fn targets(&self) -> Vec<PathBuf> {
        if self.marked.is_empty() {
            self.tab().current_path().into_iter().collect()
        } else {
            self.marked.iter().cloned().collect()
        }
    }

    /// the directory under the cursor, if it is one
    fn current_dir(&self) -> Option<PathBuf> {
        self.tab()
            .current_node()
            .filter(|n| n.is_dir())
            .map(|n| n.path.clone())
    }

    fn expand(&mut self) {
        let Some(h) = self.tab().current() else {
            return;
        };
        let tab = self.tab_mut();
        match tab.tree.get(h) {
            Some(node) if node.is_dir() && !node.expanded => {
                tab.tree.expand(h);
                tab.stale = true;
            }
            Some(node) if node.is_dir() => {
                // already open: step into the first visible child
                let depth = tab.items[tab.cursor].depth;
                if tab
                    .items
                    .get(tab.cursor + 1)
                    .is_some_and(|i| i.depth > depth)
                {
                    tab.cursor += 1;
                }
            }
            _ => {}
        }
    }

    fn collapse(&mut self) {
        let Some(h) = self.tab().current() else {
            return;
        };
        let tab = self.tab_mut();
        match tab.tree.get(h) {
            Some(node) if node.expanded => {
                tab.tree.collapse(h);
                tab.stale = true;
            }
            Some(node) => {
                if let Some(parent) = node.parent {
                    tab.select(parent);
                }
            }
            None => {}
        }
    }

    fn collapse_all(&mut self) {
        let tab = &mut self.tabs[self.active];
        // remember the top level entry the cursor is in
        let mut top = tab.current();
        while let Some(h) = top {
            match tab.tree.parent(h) {
                Some(p) if tab.position(p).is_some() => top = Some(p),
                _ => break,
            }
        }
        for root in tab.tree.roots().to_vec() {
            for child in tab.tree.children(root).to_vec() {
                tab.tree.collapse_all(child);
            }
            if tab.kind == TabKind::Pins {
                tab.tree.collapse(root);
            }
        }
        tab.rebuild(&self.marked);
        if let Some(h) = top {
            self.tab_mut().select(h);
        }
    }

    fn root_up(&mut self) {
        let Some(parent) = self
            .tab()
            .root_path()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
        else {
            return;
        };
        let marked = self.marked.clone();
        if let Err(err) = self.tab_mut().set_root(&parent, &marked) {
            self.error(format!("{}: {err}", parent.display()));
        }
    }

    /// changes the root of a normal tab, the pins tab opens a new tab instead
    fn open_root(&mut self, dir: &Path) {
        if self.tab().kind == TabKind::Pins {
            self.new_tab(dir);
            return;
        }
        let marked = self.marked.clone();
        if let Err(err) = self.tab_mut().set_root(dir, &marked) {
            self.error(format!("{}: {err}", dir.display()));
        }
    }

    fn new_tab(&mut self, dir: &Path) {
        match Tab::new_dir(dir) {
            Ok(tab) => {
                self.tabs.insert(self.active + 1, tab);
                self.active += 1;
            }
            Err(err) => self.error(format!("{}: {err}", dir.display())),
        }
    }

    fn toggle_pin(&mut self) {
        let Some(dir) = self.current_dir().or_else(|| self.tab().target_dir()) else {
            return;
        };
        match self.pins.toggle(&dir) {
            Ok(pinned) => {
                self.info(format!(
                    "{} {}",
                    if pinned { "pinned" } else { "unpinned" },
                    dir.display()
                ));
                self.sync_pins_tab();
            }
            Err(err) => self.error(format!("could not save pins: {err}")),
        }
    }

    /// keeps the roots of an open pins tab in line with the pin list
    fn sync_pins_tab(&mut self) {
        let pins = self.pins.paths().to_vec();
        let Some(tab) = self.tabs.iter_mut().find(|t| t.kind == TabKind::Pins) else {
            return;
        };
        for root in tab.tree.roots().to_vec() {
            let keep = tab
                .tree
                .path(root)
                .is_some_and(|p| pins.iter().any(|pin| pin == p));
            if !keep {
                tab.tree.remove_root(root);
            }
        }
        for pin in &pins {
            if tab
                .tree
                .find(pin)
                .is_none_or(|h| !tab.tree.roots().contains(&h))
            {
                let _ = tab.tree.add_root(pin);
            }
        }
        tab.stale = true;
    }

    fn open_pins(&mut self) {
        if let Some(i) = self.tabs.iter().position(|t| t.kind == TabKind::Pins) {
            self.active = i;
            return;
        }
        self.tabs.push(Tab::new_pins(self.pins.paths()));
        self.active = self.tabs.len() - 1;
        if self.pins.paths().is_empty() {
            self.info("no pins yet, pin a directory with b");
        }
    }

    fn fill_clipboard(&mut self, kind: TransferKind) {
        let paths: BTreeSet<PathBuf> = self.targets().into_iter().collect();
        if paths.is_empty() {
            return;
        }
        let verb = match kind {
            TransferKind::Copy => "copy",
            TransferKind::Move => "move",
        };
        self.info(format!("{} to {verb}, paste with p", count(paths.len())));
        self.clipboard = Some(Clipboard { paths, kind });
        self.marked.clear();
        self.invalidate_all();
    }

    fn paste(&mut self) {
        let Some(clipboard) = self.clipboard.take() else {
            self.info("clipboard is empty");
            return;
        };
        let Some(dest) = self.tab().target_dir() else {
            self.clipboard = Some(clipboard);
            return;
        };

        let mut errors = Vec::new();
        let mut created = Vec::new();
        for src in &clipboard.paths {
            match ops::transfer(src, &dest, clipboard.kind) {
                Ok(new) => created.push(new),
                Err(err) => errors.push(format!("{}: {err}", crate::fs::node::display_name(src))),
            }
        }

        let mut changed = vec![dest.clone()];
        if clipboard.kind == TransferKind::Move {
            changed.extend(parents(
                &clipboard.paths.iter().cloned().collect::<Vec<_>>(),
            ));
        } else {
            // a copy can be pasted again
            self.clipboard = Some(clipboard);
        }
        self.after_change(changed);

        if let Some(first) = created.first() {
            self.reveal(&first.clone());
        }
        if errors.is_empty() {
            self.info(format!(
                "pasted {} into {}",
                count(created.len()),
                dest.display()
            ));
        } else {
            self.error(errors.join(", "));
        }
    }

    fn delete(&mut self, paths: Vec<PathBuf>) {
        let mut errors = Vec::new();
        for path in &paths {
            if let Err(err) = ops::remove(path) {
                errors.push(format!("{}: {err}", crate::fs::node::display_name(path)));
            }
            let _ = self.pins.remove(path);
        }
        if let Some(clipboard) = &mut self.clipboard {
            clipboard.paths.retain(|p| p.symlink_metadata().is_ok());
        }
        self.after_change(parents(&paths));
        self.sync_pins_tab();
        if errors.is_empty() {
            self.info(format!("deleted {}", count(paths.len())));
        } else {
            self.error(errors.join(", "));
        }
    }

    /// re-reads the given directories in every tab that has them loaded
    fn after_change(&mut self, dirs: impl IntoIterator<Item = PathBuf>) {
        let dirs: BTreeSet<PathBuf> = dirs.into_iter().collect();
        for tab in &mut self.tabs {
            for dir in &dirs {
                if let Some(h) = tab.tree.find(dir) {
                    tab.tree.refresh(h);
                }
            }
        }
        self.prune_marks();
        self.invalidate_all();
    }

    fn prune_marks(&mut self) {
        self.marked.retain(|p| p.symlink_metadata().is_ok());
    }
}

fn parents(paths: &[PathBuf]) -> Vec<PathBuf> {
    paths
        .iter()
        .filter_map(|p| p.parent().map(Path::to_path_buf))
        .collect()
}

fn count(n: usize) -> String {
    if n == 1 {
        "1 item".to_string()
    } else {
        format!("{n} items")
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crossterm::event::KeyModifiers;
    use pretty_assertions::assert_eq;
    use tempfile::TempDir;

    use super::*;

    struct Fixture {
        _dir: TempDir,
        root: PathBuf,
        app: App,
    }

    /// root/{a/{one.txt, two.txt}, b/, c.txt}
    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        fs::create_dir_all(root.join("a")).unwrap();
        fs::create_dir_all(root.join("b")).unwrap();
        fs::write(root.join("a/one.txt"), "1").unwrap();
        fs::write(root.join("a/two.txt"), "2").unwrap();
        fs::write(root.join("c.txt"), "c").unwrap();
        let mut app = App::new(&[root.clone()], Pins::in_memory()).unwrap();
        app.prepare();
        Fixture {
            _dir: dir,
            root,
            app,
        }
    }

    fn keys(app: &mut App, keys: &str) {
        for c in keys.chars() {
            let code = match c {
                '\n' => KeyCode::Enter,
                '\x1b' => KeyCode::Esc,
                c => KeyCode::Char(c),
            };
            app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
        }
    }

    fn visible(app: &App) -> Vec<String> {
        let tab = app.tab();
        tab.items
            .iter()
            .map(|i| {
                let n = tab.tree.get(i.handle).unwrap();
                format!("{}{}", "  ".repeat(i.depth), n.name)
            })
            .collect()
    }

    fn current_name(app: &App) -> String {
        app.tab().current_node().unwrap().name.clone()
    }

    #[test]
    fn navigate_and_expand() {
        let Fixture { mut app, _dir, .. } = fixture();
        assert_eq!(visible(&app), vec!["a", "b", "c.txt"]);
        keys(&mut app, "l");
        assert_eq!(
            visible(&app),
            vec!["a", "  one.txt", "  two.txt", "b", "c.txt"]
        );
        keys(&mut app, "lj");
        assert_eq!(current_name(&app), "two.txt");
        keys(&mut app, "h");
        assert_eq!(current_name(&app), "a");
        keys(&mut app, "h");
        assert_eq!(visible(&app), vec!["a", "b", "c.txt"]);
        keys(&mut app, "G");
        assert_eq!(current_name(&app), "c.txt");
    }

    #[test]
    fn copy_between_tabs() {
        let Fixture {
            mut app,
            root,
            _dir,
        } = fixture();
        // mark both files in a
        keys(&mut app, "lj  ");
        assert_eq!(app.marked.len(), 2);
        keys(&mut app, "y");
        assert!(app.marked.is_empty());

        // open a second tab rooted at b and paste there
        keys(&mut app, "gjjjt");
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.tab().title(), "b");
        keys(&mut app, "p");
        assert!(root.join("b/one.txt").exists());
        assert!(root.join("b/two.txt").exists());
        assert!(root.join("a/one.txt").exists());
        assert_eq!(visible(&app), vec!["one.txt", "two.txt"]);

        // the first tab sees the new files as well once b is opened there
        keys(&mut app, "[");
        assert_eq!(app.active, 0);
        keys(&mut app, "ggjjjl");
        assert_eq!(current_name(&app), "b");
        assert_eq!(visible(&app)[3..], ["b", "  one.txt", "  two.txt", "c.txt"]);
    }

    #[test]
    fn cut_and_paste_moves() {
        let Fixture {
            mut app,
            root,
            _dir,
        } = fixture();
        keys(&mut app, "Gx");
        keys(&mut app, "ggp");
        assert!(!root.join("c.txt").exists());
        assert!(root.join("a/c.txt").exists());
        assert!(app.clipboard.is_none());
        assert_eq!(current_name(&app), "c.txt");
        assert_eq!(app.tab().items[app.tab().cursor].depth, 1);
    }

    #[test]
    fn delete_needs_confirmation() {
        let Fixture {
            mut app,
            root,
            _dir,
        } = fixture();
        keys(&mut app, "Gdn");
        assert!(root.join("c.txt").exists());
        keys(&mut app, "dy");
        assert!(!root.join("c.txt").exists());
        assert_eq!(visible(&app), vec!["a", "b"]);
    }

    #[test]
    fn rename_create_chmod() {
        let Fixture {
            mut app,
            root,
            _dir,
        } = fixture();
        keys(&mut app, "G");
        // clear the prefilled name and type a new one
        app.apply(Action::Rename);
        app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        keys(&mut app, "d.md\n");
        assert!(root.join("d.md").exists());
        assert_eq!(current_name(&app), "d.md");

        keys(&mut app, "ggjanew/\n");
        assert!(root.join("b/new").is_dir());
        assert_eq!(current_name(&app), "new");

        keys(&mut app, "G");
        app.apply(Action::Chmod);
        app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        keys(&mut app, "600\n");
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(root.join("d.md"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn search_and_escape() {
        let Fixture { mut app, _dir, .. } = fixture();
        // load a without leaving it open, the search still finds its content
        keys(&mut app, "lh");
        keys(&mut app, "/two");
        assert_eq!(visible(&app), vec!["a", "  two.txt"]);
        keys(&mut app, "\n");
        assert_eq!(app.tab().filter.query, "two");
        keys(&mut app, "\x1b");
        assert_eq!(app.tab().filter.query, "");
        assert!(!app.should_quit);
        keys(&mut app, "\x1b");
        assert!(app.should_quit);
    }

    #[test]
    fn search_finds_unopened_entries() {
        let Fixture { mut app, _dir, .. } = fixture();
        keys(&mut app, "/one");
        assert_eq!(visible(&app), vec!["a", "  one.txt"]);
        // the search does not open anything
        keys(&mut app, "\x1b");
        assert_eq!(visible(&app), vec!["a", "b", "c.txt"]);
    }

    fn jump_level(app: &App) -> Vec<String> {
        let Mode::Jump(jump) = &app.mode else {
            panic!("not in jump mode");
        };
        jump.matches
            .iter()
            .map(|h| app.tab().tree.get(*h).unwrap().name.clone())
            .collect()
    }

    #[test]
    fn jump_from_root_through_dirs_to_file() {
        let Fixture {
            mut app,
            root,
            _dir,
        } = fixture();
        // the cursor is deep in the tree, `e` still starts at the top level
        keys(&mut app, "lj");
        keys(&mut app, "e");
        assert_eq!(jump_level(&app), vec!["a", "b", "c.txt"]);

        keys(&mut app, "a");
        assert_eq!(jump_level(&app), vec!["a"]);
        keys(&mut app, "\n");
        assert_eq!(jump_level(&app), vec!["one.txt", "two.txt"]);

        keys(&mut app, "tw");
        assert_eq!(jump_level(&app), vec!["two.txt"]);
        keys(&mut app, "\n");
        assert!(matches!(app.mode, Mode::Normal));
        assert_eq!(app.tab().current_path(), Some(root.join("a/two.txt")));
        assert_eq!(
            visible(&app),
            vec!["a", "  one.txt", "  two.txt", "b", "c.txt"]
        );
    }

    #[test]
    fn jump_here_starts_on_cursor_level() {
        let Fixture { mut app, _dir, .. } = fixture();
        keys(&mut app, "lj");
        keys(&mut app, "E");
        assert_eq!(jump_level(&app), vec!["one.txt", "two.txt"]);
    }

    #[test]
    fn jump_escape_stays_in_dir() {
        let Fixture {
            mut app,
            root,
            _dir,
        } = fixture();
        keys(&mut app, "ea\n\x1b");
        assert!(matches!(app.mode, Mode::Normal));
        assert_eq!(app.tab().current_path(), Some(root.join("a")));
        assert!(app.tab().current_node().unwrap().expanded);
    }

    #[test]
    fn jump_backspace_and_cursor() {
        let Fixture {
            mut app,
            root,
            _dir,
        } = fixture();
        keys(&mut app, "e.");
        assert_eq!(jump_level(&app), vec!["c.txt"]);
        app.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        keys(&mut app, "\n");
        assert_eq!(jump_level(&app), vec!["one.txt", "two.txt"]);
        // backspace with an empty input goes one level up again
        app.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(jump_level(&app), vec!["a", "b", "c.txt"]);
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        keys(&mut app, "\n");
        assert_eq!(app.tab().current_path(), Some(root.join("c.txt")));
    }

    #[test]
    fn filter_and_sort_menus() {
        let Fixture { mut app, _dir, .. } = fixture();
        keys(&mut app, "ff");
        assert_eq!(visible(&app), vec!["a", "b"]);
        keys(&mut app, "fr");
        keys(&mut app, "sr");
        assert_eq!(visible(&app), vec!["b", "a", "c.txt"]);
        keys(&mut app, "sd");
        assert_eq!(visible(&app), vec!["c.txt", "b", "a"]);
    }

    #[test]
    fn only_marked_filter() {
        let Fixture { mut app, _dir, .. } = fixture();
        keys(&mut app, "jj ");
        keys(&mut app, "fm");
        assert_eq!(visible(&app), vec!["c.txt"]);
    }

    #[test]
    fn root_up_and_down_keep_state() {
        let Fixture {
            mut app,
            root,
            _dir,
        } = fixture();
        keys(&mut app, "lL");
        assert_eq!(app.tab().root_path(), Some(root.join("a").as_path()));
        assert_eq!(visible(&app), vec!["one.txt", "two.txt"]);
        keys(&mut app, "H");
        assert_eq!(app.tab().root_path(), Some(root.as_path()));
        assert_eq!(current_name(&app), "a");
        assert_eq!(
            visible(&app),
            vec!["a", "  one.txt", "  two.txt", "b", "c.txt"]
        );
    }

    #[test]
    fn pins_tab() {
        let Fixture { mut app, _dir, .. } = fixture();
        keys(&mut app, "bjb");
        assert_eq!(app.pins.paths().len(), 2);
        keys(&mut app, "B");
        assert_eq!(app.tab().kind, TabKind::Pins);
        assert_eq!(visible(&app), vec!["a", "b"]);
        keys(&mut app, "l");
        assert_eq!(visible(&app), vec!["a", "  one.txt", "  two.txt", "b"]);

        // unpinning from the pins tab updates it right away
        keys(&mut app, "Gb");
        assert_eq!(visible(&app), vec!["a", "  one.txt", "  two.txt"]);
    }

    #[test]
    fn pick_outputs_marks_or_cursor() {
        let Fixture {
            mut app,
            root,
            _dir,
        } = fixture();
        keys(&mut app, "G\n");
        assert!(app.should_quit);
        assert_eq!(app.output, vec![root.join("c.txt")]);

        let Fixture {
            mut app,
            root,
            _dir,
        } = fixture();
        keys(&mut app, " j o");
        assert_eq!(app.output, vec![root.join("a"), root.join("c.txt")]);

        let Fixture {
            mut app,
            root,
            _dir,
        } = fixture();
        keys(&mut app, "GO");
        assert_eq!(app.output, vec![root.clone()]);
    }
}
