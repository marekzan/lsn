use crate::view::SortBy;

/// everything the user can do in normal mode.
///
/// keys are mapped to actions in `keymap.rs`, the app only knows actions.
/// this keeps the key handling configurable and testable without a terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    // navigation
    Up,
    Down,
    Top,
    Bottom,
    HalfPageUp,
    HalfPageDown,
    /// expand a directory or move into its first child
    Expand,
    /// collapse a directory or move to its parent
    Collapse,
    /// toggle a directory, pick a file
    Open,
    CollapseAll,
    /// make the parent of the root the new root (`cd ..`)
    RootUp,
    /// make the directory under the cursor the new root (`cd dir`)
    RootDown,
    RootHome,

    // marks
    ToggleMark,
    InvertMarks,
    ClearMarks,

    // file operations
    Yank,
    Cut,
    Paste,
    Delete,
    Rename,
    Create,
    Chmod,

    // view
    Search,
    FilterMenu,
    SortMenu,
    ToggleHidden,
    ToggleMeta,
    Refresh,

    // filter menu
    ToggleDirs,
    ToggleFiles,
    ToggleSymlinks,
    ToggleOnlyMarked,
    ResetFilter,

    // sort menu
    SortBy(SortBy),
    ToggleDirsFirst,
    ToggleReverse,

    // tabs
    NewTab,
    CloseTab,
    NextTab,
    PrevTab,
    GoToTab(usize),
    TogglePin,
    OpenPins,

    // leaving
    /// print the marked paths (or the path under the cursor) and quit
    Pick,
    /// print the directory under the cursor and quit, for `cd "$(lsn)"`
    PickDir,
    Help,
    Escape,
    Quit,
}
