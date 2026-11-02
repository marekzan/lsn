# lsn

Explore and manage files in the terminal via a tree view.

- tree view with lazy loading, vim keys
- tabs: copy / move files between them with a global clipboard
- marks across tabs, act on many files at once
- filters (dirs, files, symlinks, hidden, only marked) and live search
- jump mode: type to narrow a level, single matches are entered automatically
- sort by name, size, modified, extension
- metadata columns like `eza -l` (permissions, owner, size, date), chmod
- pinned directories in their own tab
- runs inline below your prompt (default) or fullscreen (`-f`)

## Usage

```sh
lsn                 # current directory
lsn src docs        # one tab per directory
lsn -f              # fullscreen
lsn -H 30           # inline with 30 lines

cd "$(lsn)"         # press O on a directory
vim $(lsn)          # mark files with space, press o
```

Press `?` inside lsn for all keys. The most important ones:

| key           | action                                   |
|---------------|------------------------------------------|
| `hjkl` arrows | navigate, open / close directories       |
| `space`       | mark (works across tabs)                 |
| `y` `x` `p`   | copy, cut, paste into dir under cursor   |
| `d` `r` `a` `c` | delete, rename, new file (`dir/`), chmod |
| `/`           | search                                   |
| `;`           | jump: type to narrow the level, single matches open after 500ms, `esc` stays |
| `f` …         | filter menu (`d` `f` `l` `.` `m` `r`)    |
| `s` …         | sort menu (`n` `s` `m` `e` `d` `r`)      |
| `i`           | metadata columns                         |
| `t` `tab` `C-w` | new tab, next tab, close tab           |
| `b` `B`       | pin directory, pins tab                  |
| `H` `L` `~`   | root to parent / to dir / home           |
| `enter` `o` `O` | pick and print, then quit              |

See [ARCHITECTURE.md](./ARCHITECTURE.md) for how it is built.

## **Contributing**

Contributions are welcome! Fork the repository and submit a pull request.

## **License**

lsn is dual-licensed under:

- **Apache License, Version 2.0** ([LICENSE-APACHE](./LICENSE-APACHE))
- **MIT License** ([LICENSE-MIT](./LICENSE-MIT))
