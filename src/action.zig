const SortBy = @import("view.zig").SortBy;
const jump = @import("jump.zig");

/// everything the user can do in normal mode.
///
/// keys are mapped to actions in `keymap.zig`, the app only knows actions.
/// this keeps the key handling configurable and testable without a terminal.
pub const Action = union(enum) {
    // navigation
    up,
    down,
    top,
    bottom,
    half_page_up,
    half_page_down,
    /// expand a directory or move into its first child
    expand,
    /// collapse a directory or move to its parent
    collapse,
    /// toggle a directory, pick a file
    open,
    collapse_all,
    /// make the parent of the root the new root (`cd ..`)
    root_up,
    /// make the directory under the cursor the new root (`cd dir`)
    root_down,
    root_home,

    // marks
    toggle_mark,
    invert_marks,
    clear_marks,

    // file operations
    yank,
    cut,
    paste,
    delete,
    rename,
    create,
    chmod,

    // view
    search,
    /// type to narrow a level, enter to go deeper
    jump: jump.Start,
    filter_menu,
    sort_menu,
    toggle_hidden,
    toggle_meta,
    refresh,

    // filter menu
    toggle_dirs,
    toggle_files,
    toggle_symlinks,
    toggle_only_marked,
    reset_filter,

    // sort menu
    sort_by: SortBy,
    toggle_dirs_first,
    toggle_reverse,

    // tabs
    new_tab,
    close_tab,
    next_tab,
    prev_tab,
    go_to_tab: usize,
    toggle_pin,
    open_pins,

    // leaving
    /// print the marked paths (or the path under the cursor) and quit
    pick,
    /// print the directory under the cursor and quit, for `cd "$(lsn)"`
    pick_dir,
    help,
    escape,
    quit,
};
