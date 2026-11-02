# Architecture

lsn is split into layers that only depend on the layers below them:

```
main.rs ─ event loop: draw → wait for key → update (no ticks, only redraws after input)
  │
  ├─ term.rs ── terminal setup: inline viewport (default) or alternate screen (--fullscreen)
  ├─ ui.rs ──── renders App state with ratatui, never mutates the model
  │
  └─ app.rs ─── App: tabs, global marks, clipboard, pins, mode (normal/menu/prompt/confirm/help)
       │         keys ─(keymap.rs)→ Action ─(App::apply)→ state changes
       │
       ├─ tab.rs ──── Tab: one FileTree + its view list, cursor, filter and sort
       ├─ jump.rs ─── level by level quick navigation
       ├─ view.rs ─── view-model: flattens the visible part of a tree (filter + sort)
       ├─ fs/
       │   ├─ tree.rs  model: FileTree, nodes stored in the Arena, lazy loading, refresh
       │   ├─ node.rs  Node, NodeKind, Meta
       │   ├─ ops.rs   copy/move/delete/rename/create/chmod on plain paths
       │   └─ owner.rs uid/gid → name (cached)
       └─ arena.rs ── generational arena, stable `Handle`s instead of Box/Rc
```

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
- **View (`ui.rs`)**: reads `App` and draws only the visible slice of the list.

## Global vs. per tab state

| state                       | where       | why                                   |
|-----------------------------|-------------|---------------------------------------|
| tree, cursor, filter, sort  | `Tab`       | every tab looks at its own directory  |
| marks (`BTreeSet<PathBuf>`) | `App`       | mark in several tabs, act on all      |
| clipboard                   | `App`       | copy in one tab, paste in another     |
| pins                        | `App`/disk  | shared, stored in `$XDG_DATA_HOME/lsn/pins` |

Marks and the clipboard use paths, not handles. Handles are only valid inside
one tree, while paths stay valid across tabs and trees.

After a file operation, `App::after_change` refreshes the affected directories
in **every** tab that has them loaded, so all tabs show the change at once.

## Filters

A node is visible if it matches all active filters. A directory that doesn't
match itself is still shown as context when one of its descendants matches.
Search and "only marked" also look into collapsed directories, so matches deep
in the tree show up with their parents. When a search starts, the tab loads up
to 20 000 not yet read entries breadth first, so files in folders you never
opened are found too.

## Jump

`e` starts a jump (`jump.rs`) on the top level of the tab, `E` on the level of
the cursor. It shows one flat level and filters it with the typed text.
`enter` on a directory shows its content as the next level (with an empty
input), `enter` on a file ends the jump with the cursor on it, `backspace` on
an empty input goes one level up. `esc` ends the jump in the current
directory. Only at the end the result is revealed in the real tree.

## Inline mode and stdout

The UI always draws to the terminal. If stdout is not a tty (`cd "$(lsn)"`),
`term.rs` points fd 1 at `/dev/tty` while the UI runs. Picked paths are printed
to the real stdout only after the terminal was restored.

## Keys

All keys are defined once in `keymap.rs` (`NORMAL`, `FILTER_MENU`,
`SORT_MENU`). Both the key handling and the help screen (`?`) use these tables,
which is also where a config file can later plug in.
