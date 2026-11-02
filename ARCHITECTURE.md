# Architecture

lsn is split into layers that only depend on the layers below them:

```
main.zig ─ args, event loop: draw → wait for key → update (only redraws after input or a resize)
  │
  ├─ term/ ──────── own terminal layer, everything over /dev/tty
  │   ├─ terminal.zig  raw mode, inline viewport (default) or alternate screen, reading keys
  │   ├─ screen.zig    cell buffer with styles, diff rendering (only changed cells are written)
  │   ├─ parse.zig     bytes → keys (escape sequences, utf-8, ctrl/alt)
  │   ├─ width.zig     display width of codepoints (wide chars, combining marks)
  │   └─ key.zig       the key type
  ├─ ui.zig ─────── renders App state into a frame, never mutates the model
  │
  └─ app.zig ────── App: tabs, global marks, clipboard, pins, mode (normal/menu/prompt/confirm/help/jump)
       │             keys ─(keymap.zig)→ Action ─(App.apply)→ state changes
       │
       ├─ tab.zig ──── Tab: one FileTree + its view list, cursor, filter and sort
       ├─ jump.zig ─── level by level quick navigation
       ├─ view.zig ─── view-model: flattens the visible part of a tree (filter + sort)
       ├─ fs/
       │   ├─ tree.zig  model: FileTree, nodes stored in the Arena, lazy loading, refresh
       │   ├─ node.zig  Node, Kind, Meta
       │   └─ ops.zig   copy/move/delete/rename/create/chmod on plain paths
       ├─ pathset.zig ─ owned set of paths (marks, clipboard)
       ├─ arena.zig ── generational arena, stable `Handle`s instead of pointers
       └─ sys.zig ──── thin libc wrappers (stat, dirs, files, users, local time)
```

The file system and the tty are used through libc instead of `std.Io`: the
libc api is stable, `std.Io` still changes a lot between Zig versions.

## Memory

Every owner frees what it allocated: nodes own their path, the arena owns the
nodes, the tree owns the arena, a tab owns its tree. Paths handed out by the
tree (`Tab.currentPath`, `Tab.targetDir`) are borrowed and only valid until
the tree changes, so the app copies them before any file operation. All tests
run with `std.testing.allocator`, which fails on leaks.

## Model, view-model, view

- **Model (`FileTree`)**: the truth about the file system. Nodes live in an
  `Arena<Node>` and link to each other with `Handle`s (`parent`, `children`).
  Directories are read lazily the first time they are opened. `refresh`
  re-reads a directory and keeps the handles of entries that still exist, so
  expanded state and cursor survive. Children keep the order the OS gives
  (sorted by path). The model never changes just because the user sorts or
  filters.
- **View-model (`view::build` → `Vec<ViewItem>`)**: the flat list of visible
  nodes with their depth and tree guides. Filtering and sorting happen here.
  Cursor movement, scrolling and "node under cursor" are O(1) on this list.
  It is rebuilt lazily (`Tab::stale`) after something changed.
- **View (`ui.zig`)**: reads `App` and draws only the visible slice of the list.

## Global vs. per tab state

| state                       | where       | why                                   |
|-----------------------------|-------------|---------------------------------------|
| tree, cursor, filter, sort  | `Tab`       | every tab looks at its own directory  |
| marks (`BTreeSet<PathBuf>`) | `App`       | mark in several tabs, act on all      |
| clipboard                   | `App`       | copy in one tab, paste in another     |
| pins                        | `App`/disk  | shared, stored in `$XDG_DATA_HOME/lsn/pins` |

Marks and the clipboard use paths, not handles. Handles are only valid inside
one tree, while paths stay valid across tabs and trees.

After a file operation, `App.afterChange` refreshes the affected directories
in **every** tab that has them loaded, so all tabs show the change at once.

## Filters

A node is visible if it matches all active filters. A directory that doesn't
match itself is still shown as context when one of its descendants matches.
Search and "only marked" also look into collapsed directories, so matches deep
in the tree show up with their parents. When a search starts, the tab loads up
to 20 000 not yet read entries breadth first, so files in folders you never
opened are found too.

## Jump

`e` starts a jump (`jump.zig`) on the top level of the tab, `E` on the level of
the cursor. It shows one flat level and filters it with the typed text.
`enter` on a directory shows its content as the next level (with an empty
input), `enter` on a file ends the jump with the cursor on it, `backspace` on
an empty input goes one level up. `esc` ends the jump in the current
directory. Only at the end the result is revealed in the real tree.

## Inline mode and stdout

The UI reads and writes `/dev/tty` directly, never stdin or stdout. stdout
stays free for the picked paths (`cd "$(lsn)"`), which are printed after the
terminal was restored. Inline mode asks the terminal for the cursor row, makes
room below the prompt and wipes its lines on exit. A panic handler restores
the terminal before the panic message is printed.

## Keys

All keys are defined once in `keymap.zig` (`normal`, `filter_menu`,
`sort_menu`). Both the key handling and the help screen (`?`) use these tables,
which is also where a config file can later plug in.
