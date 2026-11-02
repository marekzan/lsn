use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{action::Action, jump::Start, view::SortBy};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    pub code: KeyCode,
    pub mods: KeyModifiers,
}

const fn k(c: char) -> Key {
    Key {
        code: KeyCode::Char(c),
        mods: KeyModifiers::NONE,
    }
}

const fn ctrl(c: char) -> Key {
    Key {
        code: KeyCode::Char(c),
        mods: KeyModifiers::CONTROL,
    }
}

const fn code(code: KeyCode) -> Key {
    Key {
        code,
        mods: KeyModifiers::NONE,
    }
}

pub struct Binding {
    pub keys: &'static [Key],
    pub action: Action,
    pub desc: &'static str,
}

const fn b(keys: &'static [Key], action: Action, desc: &'static str) -> Binding {
    Binding { keys, action, desc }
}

/// the single source of truth for key handling and the help screen
pub const NORMAL: &[Binding] = &[
    b(&[k('j'), code(KeyCode::Down)], Action::Down, "down"),
    b(&[k('k'), code(KeyCode::Up)], Action::Up, "up"),
    b(
        &[k('l'), code(KeyCode::Right)],
        Action::Expand,
        "expand / into dir",
    ),
    b(
        &[k('h'), code(KeyCode::Left)],
        Action::Collapse,
        "collapse / to parent",
    ),
    b(
        &[code(KeyCode::Enter)],
        Action::Open,
        "toggle dir / pick file",
    ),
    b(&[k('g'), code(KeyCode::Home)], Action::Top, "top"),
    b(&[k('G'), code(KeyCode::End)], Action::Bottom, "bottom"),
    b(
        &[ctrl('d'), code(KeyCode::PageDown)],
        Action::HalfPageDown,
        "half page down",
    ),
    b(
        &[ctrl('u'), code(KeyCode::PageUp)],
        Action::HalfPageUp,
        "half page up",
    ),
    b(&[k('z')], Action::CollapseAll, "collapse all"),
    b(
        &[k('H'), code(KeyCode::Backspace)],
        Action::RootUp,
        "root: parent dir",
    ),
    b(&[k('L')], Action::RootDown, "root: dir under cursor"),
    b(&[k('~')], Action::RootHome, "root: home"),
    b(&[k(' ')], Action::ToggleMark, "mark / unmark"),
    b(&[k('*')], Action::InvertMarks, "invert marks (visible)"),
    b(&[k('U')], Action::ClearMarks, "clear marks"),
    b(&[k('y')], Action::Yank, "copy to clipboard"),
    b(&[k('x')], Action::Cut, "cut to clipboard"),
    b(&[k('p')], Action::Paste, "paste into dir"),
    b(&[k('d'), code(KeyCode::Delete)], Action::Delete, "delete"),
    b(&[k('r')], Action::Rename, "rename"),
    b(&[k('a')], Action::Create, "new file (dir/ for dir)"),
    b(&[k('c')], Action::Chmod, "chmod"),
    b(&[k('/')], Action::Search, "search"),
    b(&[k('e')], Action::Jump(Start::Root), "jump from top level"),
    b(
        &[k('E')],
        Action::Jump(Start::Here),
        "jump from current level",
    ),
    b(&[k('f')], Action::FilterMenu, "filter menu"),
    b(&[k('s')], Action::SortMenu, "sort menu"),
    b(&[k('.')], Action::ToggleHidden, "toggle hidden"),
    b(&[k('i')], Action::ToggleMeta, "toggle metadata"),
    b(&[k('R'), ctrl('r')], Action::Refresh, "reload from disk"),
    b(&[k('t')], Action::NewTab, "new tab at dir"),
    b(&[ctrl('w')], Action::CloseTab, "close tab"),
    b(&[code(KeyCode::Tab), k(']')], Action::NextTab, "next tab"),
    b(
        &[code(KeyCode::BackTab), k('[')],
        Action::PrevTab,
        "previous tab",
    ),
    b(&[k('1')], Action::GoToTab(0), "tab 1 (… 9)"),
    b(&[k('2')], Action::GoToTab(1), ""),
    b(&[k('3')], Action::GoToTab(2), ""),
    b(&[k('4')], Action::GoToTab(3), ""),
    b(&[k('5')], Action::GoToTab(4), ""),
    b(&[k('6')], Action::GoToTab(5), ""),
    b(&[k('7')], Action::GoToTab(6), ""),
    b(&[k('8')], Action::GoToTab(7), ""),
    b(&[k('9')], Action::GoToTab(8), ""),
    b(&[k('b')], Action::TogglePin, "pin / unpin dir"),
    b(&[k('B')], Action::OpenPins, "pins tab"),
    b(&[k('o')], Action::Pick, "print marked/cursor, quit"),
    b(&[k('O')], Action::PickDir, "print dir, quit (cd)"),
    b(&[k('?')], Action::Help, "help"),
    b(
        &[code(KeyCode::Esc)],
        Action::Escape,
        "clear search/marks, quit",
    ),
    b(&[k('q'), ctrl('c')], Action::Quit, "quit"),
];

/// second key after `f`
pub const FILTER_MENU: &[Binding] = &[
    b(&[k('d')], Action::ToggleDirs, "dirs"),
    b(&[k('f')], Action::ToggleFiles, "files"),
    b(&[k('l')], Action::ToggleSymlinks, "symlinks"),
    b(&[k('.')], Action::ToggleHidden, "hidden"),
    b(&[k('m')], Action::ToggleOnlyMarked, "only marked"),
    b(&[k('r')], Action::ResetFilter, "reset"),
];

/// second key after `s`
pub const SORT_MENU: &[Binding] = &[
    b(&[k('n')], Action::SortBy(SortBy::Name), "name"),
    b(&[k('s')], Action::SortBy(SortBy::Size), "size"),
    b(&[k('m')], Action::SortBy(SortBy::Modified), "modified"),
    b(&[k('e')], Action::SortBy(SortBy::Extension), "extension"),
    b(&[k('d')], Action::ToggleDirsFirst, "dirs first"),
    b(&[k('r')], Action::ToggleReverse, "reverse"),
];

pub fn lookup(table: &[Binding], event: KeyEvent) -> Option<Action> {
    let key = normalize(event);
    table
        .iter()
        .find(|b| b.keys.contains(&key))
        .map(|b| b.action)
}

/// terminals report `G` as `Shift+G`, we only care about the character
fn normalize(event: KeyEvent) -> Key {
    let mods = match event.code {
        KeyCode::Char(_) | KeyCode::BackTab => event.modifiers - KeyModifiers::SHIFT,
        _ => event.modifiers,
    };
    Key {
        code: event.code,
        mods,
    }
}

pub fn label(key: &Key) -> String {
    let name = match key.code {
        KeyCode::Char(' ') => "space".to_string(),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Enter => "enter".into(),
        KeyCode::Esc => "esc".into(),
        KeyCode::Tab => "tab".into(),
        KeyCode::BackTab => "S-tab".into(),
        KeyCode::Backspace => "bksp".into(),
        KeyCode::Delete => "del".into(),
        KeyCode::Up => "↑".into(),
        KeyCode::Down => "↓".into(),
        KeyCode::Left => "←".into(),
        KeyCode::Right => "→".into(),
        KeyCode::Home => "home".into(),
        KeyCode::End => "end".into(),
        KeyCode::PageUp => "pgup".into(),
        KeyCode::PageDown => "pgdn".into(),
        other => format!("{other:?}"),
    };
    if key.mods.contains(KeyModifiers::CONTROL) {
        format!("C-{name}")
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shift_is_ignored_for_chars() {
        let ev = KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT);
        assert_eq!(lookup(NORMAL, ev), Some(Action::Bottom));
        let ev = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL);
        assert_eq!(lookup(NORMAL, ev), Some(Action::HalfPageDown));
    }

    #[test]
    fn no_duplicate_keys() {
        for table in [NORMAL, FILTER_MENU, SORT_MENU] {
            let keys: Vec<_> = table.iter().flat_map(|b| b.keys).collect();
            for (i, key) in keys.iter().enumerate() {
                assert!(!keys[i + 1..].contains(key), "{key:?} bound twice");
            }
        }
    }
}
