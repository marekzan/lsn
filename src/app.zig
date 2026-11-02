const std = @import("std");
const Allocator = std.mem.Allocator;

const Action = @import("action.zig").Action;
const Handle = @import("arena.zig").Handle;
const ops = @import("fs/ops.zig");
const Input = @import("input.zig").Input;
const jump_mod = @import("jump.zig");
const Jump = jump_mod.Jump;
const Step = jump_mod.Step;
const keymap = @import("keymap.zig");
const PathSet = @import("pathset.zig").PathSet;
const Pins = @import("pins.zig").Pins;
const sys = @import("sys.zig");
const Tab = @import("tab.zig").Tab;
const Key = @import("term/key.zig").Key;
const view = @import("view.zig");

/// how many entries a search may load from disk before it gives up
const search_budget = 20_000;

pub const Menu = enum { filter, sort };

pub const PromptKind = union(enum) {
    /// live search, the previous query is restored on escape
    search: []u8,
    rename: []u8,
    /// create a file or directory in this directory
    create: []u8,
    chmod: [][]u8,

    pub fn label(self: PromptKind) []const u8 {
        return switch (self) {
            .search => "/",
            .rename => "rename: ",
            .create => "new (dir/ for dir): ",
            .chmod => "chmod: ",
        };
    }
};

pub const Prompt = struct {
    kind: PromptKind,
    input: Input,
};

pub const Mode = union(enum) {
    normal,
    /// waiting for the second key of `f` / `s`
    menu: Menu,
    prompt: Prompt,
    /// waiting for `y` to delete these paths
    confirm_delete: [][]u8,
    /// the help screen, scrolled down by this many lines
    help: u16,
    /// quick navigation level by level, see `jump.zig`
    jump: Jump,
};

pub const Clipboard = struct {
    paths: PathSet,
    kind: ops.TransferKind,
};

pub const Status = struct {
    text: []u8,
    is_error: bool,
};

/// user and group names for the metadata columns, the same few ids show up
/// over and over again
pub const NameCache = struct {
    users: std.AutoHashMapUnmanaged(u32, []u8) = .empty,
    groups: std.AutoHashMapUnmanaged(u32, []u8) = .empty,

    pub fn deinit(self: *NameCache, gpa: Allocator) void {
        inline for (.{ &self.users, &self.groups }) |map| {
            var it = map.valueIterator();
            while (it.next()) |v| gpa.free(v.*);
            map.deinit(gpa);
        }
    }

    pub fn user(self: *NameCache, gpa: Allocator, uid: u32) []const u8 {
        return lookup(&self.users, gpa, uid, sys.userName);
    }

    pub fn group(self: *NameCache, gpa: Allocator, gid: u32) []const u8 {
        return lookup(&self.groups, gpa, gid, sys.groupName);
    }

    fn lookup(map: *std.AutoHashMapUnmanaged(u32, []u8), gpa: Allocator, id: u32, f: *const fn (u32, *[4096]u8) ?[]const u8) []const u8 {
        if (map.get(id)) |name| return name;
        var buf: [4096]u8 = undefined;
        var num: [12]u8 = undefined;
        const name = f(id, &buf) orelse (std.fmt.bufPrint(&num, "{d}", .{id}) catch "?");
        const owned = gpa.dupe(u8, name) catch return "?";
        map.put(gpa, id, owned) catch {
            gpa.free(owned);
            return "?";
        };
        return owned;
    }
};

/// the whole application state.
///
/// the app never touches the terminal, it only reacts to keys and actions.
/// rendering lives in `ui.zig`, the event loop in `main.zig`.
pub const App = struct {
    gpa: Allocator,
    tabs: std.ArrayList(Tab) = .empty,
    active: usize = 0,
    /// marks are global, so things can be marked in several tabs at once
    marked: PathSet = .{},
    clipboard: ?Clipboard = null,
    pins: Pins,
    mode: Mode = .normal,
    status: ?Status = null,
    show_meta: bool = true,
    should_quit: bool = false,
    /// paths printed to stdout after the ui is gone
    output: std.ArrayList([]u8) = .empty,
    /// height of the tree area, set by the ui, used for page movements
    page: usize = 10,
    names: NameCache = .{},

    pub fn init(gpa: Allocator, dirs: []const []const u8, pins: Pins) !App {
        var app: App = .{ .gpa = gpa, .pins = pins };
        errdefer app.deinit();
        for (dirs) |dir| try app.tabs.append(gpa, try Tab.newDir(gpa, dir));
        return app;
    }

    pub fn deinit(self: *App) void {
        for (self.tabs.items) |*t| t.deinit();
        self.tabs.deinit(self.gpa);
        self.marked.deinit(self.gpa);
        if (self.clipboard) |*c| c.paths.deinit(self.gpa);
        self.pins.deinit();
        self.freeMode(&self.mode);
        self.clearStatus();
        self.freeOutput();
        self.names.deinit(self.gpa);
    }

    pub fn tab(self: *App) *Tab {
        return &self.tabs.items[self.active];
    }

    pub fn tabConst(self: *const App) *const Tab {
        return &self.tabs.items[self.active];
    }

    /// brings the view list of the active tab up to date
    pub fn prepare(self: *App) Allocator.Error!void {
        const t = self.tab();
        if (t.stale) try t.rebuild(&self.marked);
    }

    fn invalidateAll(self: *App) void {
        for (self.tabs.items) |*t| t.stale = true;
    }

    fn freeMode(self: *App, mode: *Mode) void {
        switch (mode.*) {
            .prompt => |*p| {
                p.input.deinit(self.gpa);
                switch (p.kind) {
                    .search, .rename, .create => |s| self.gpa.free(s),
                    .chmod => |paths| freePaths(self.gpa, paths),
                }
            },
            .confirm_delete => |paths| freePaths(self.gpa, paths),
            .jump => |*j| j.deinit(self.gpa),
            else => {},
        }
        mode.* = .normal;
    }

    fn freeOutput(self: *App) void {
        for (self.output.items) |p| self.gpa.free(p);
        self.output.deinit(self.gpa);
    }

    fn clearStatus(self: *App) void {
        if (self.status) |s| self.gpa.free(s.text);
        self.status = null;
    }

    fn setStatus(self: *App, is_error: bool, comptime fmt: []const u8, args: anytype) void {
        self.clearStatus();
        const text = std.fmt.allocPrint(self.gpa, fmt, args) catch return;
        self.status = .{ .text = text, .is_error = is_error };
    }

    fn info(self: *App, comptime fmt: []const u8, args: anytype) void {
        self.setStatus(false, fmt, args);
    }

    fn fail(self: *App, comptime fmt: []const u8, args: anytype) void {
        self.setStatus(true, fmt, args);
    }

    pub fn handleKey(self: *App, key: Key) !void {
        try self.prepare();
        if (self.mode != .prompt) self.clearStatus();

        var mode = self.mode;
        self.mode = .normal;
        switch (mode) {
            .normal => if (keymap.lookup(&keymap.normal, key)) |action| try self.apply(action),
            .menu => |menu| {
                const table: []const keymap.Binding = switch (menu) {
                    .filter => &keymap.filter_menu,
                    .sort => &keymap.sort_menu,
                };
                // any other key just closes the menu
                if (keymap.lookup(table, key)) |action| try self.apply(action);
            },
            .help => |scroll| switch (key.code) {
                .down => self.mode = .{ .help = scroll + 1 },
                .up => self.mode = .{ .help = scroll -| 1 },
                .char => |c| if (c == 'j') {
                    self.mode = .{ .help = scroll + 1 };
                } else if (c == 'k') {
                    self.mode = .{ .help = scroll -| 1 };
                },
                else => {},
            },
            .confirm_delete => |paths| {
                defer freePaths(self.gpa, paths);
                if (key.code == .char and (key.code.char == 'y' or key.code.char == 'Y')) {
                    try self.delete(paths);
                } else {
                    self.info("delete cancelled", .{});
                }
            },
            .prompt => |*prompt| try self.handlePrompt(prompt, key),
            .jump => |*j| try self.handleJump(j, key),
        }
        try self.prepare();
    }

    /// takes ownership of the prompt, it either ends here or goes back into
    /// `self.mode`
    fn handlePrompt(self: *App, prompt: *Prompt, key: Key) !void {
        switch (key.code) {
            .esc => {
                if (prompt.kind == .search) {
                    self.tab().filter.setQuery(prompt.kind.search);
                    self.tab().stale = true;
                }
                var m: Mode = .{ .prompt = prompt.* };
                self.freeMode(&m);
            },
            .enter => {
                defer {
                    var m: Mode = .{ .prompt = prompt.* };
                    self.freeMode(&m);
                }
                try self.submit(prompt);
            },
            // moving through the results while searching
            .up, .down => {
                if (prompt.kind == .search) self.tab().moveCursor(if (key.code == .up) -1 else 1);
                self.mode = .{ .prompt = prompt.* };
            },
            else => {
                if (try prompt.input.handle(self.gpa, key) and prompt.kind == .search) {
                    const t = self.tab();
                    t.filter.setQuery(prompt.input.text());
                    t.stale = true;
                    t.cursor = 0;
                }
                self.mode = .{ .prompt = prompt.* };
            },
        }
    }

    fn submit(self: *App, prompt: *const Prompt) !void {
        const text = prompt.input.text();
        switch (prompt.kind) {
            .search => {},
            .rename => |path| {
                const new = ops.rename(self.gpa, path, text) catch |err| return self.fail("rename failed: {s}", .{sys.describe(err)});
                defer self.gpa.free(new);
                if (self.marked.remove(self.gpa, path)) try self.marked.insert(self.gpa, new);
                try self.afterChange(&.{sys.dirName(path) orelse path});
                try self.reveal(new);
                self.info("renamed to {s}", .{text});
            },
            .create => |dir| {
                const new = ops.create(self.gpa, dir, text) catch |err| return self.fail("create failed: {s}", .{sys.describe(err)});
                defer self.gpa.free(new);
                try self.afterChange(&.{dir});
                try self.reveal(new);
                self.info("created {s}", .{text});
            },
            .chmod => |paths| {
                var failed: ?anyerror = null;
                for (paths) |path| ops.chmod(path, text) catch |err| {
                    failed = err;
                };
                try self.afterChangeParents(paths);
                if (failed) |err| {
                    self.fail("chmod failed: {s}", .{sys.describe(err)});
                } else {
                    self.info("chmod {s} on {d} item(s)", .{ text, paths.len });
                }
            },
        }
    }

    pub fn apply(self: *App, action: Action) !void {
        try self.prepare();
        const page: isize = @intCast(@max(self.page, 2));
        switch (action) {
            .down => self.tab().moveCursor(1),
            .up => self.tab().moveCursor(-1),
            .top => self.tab().cursor = 0,
            .bottom => self.tab().moveCursor(std.math.maxInt(isize) / 2),
            .half_page_down => self.tab().moveCursor(@divTrunc(page, 2)),
            .half_page_up => self.tab().moveCursor(-@divTrunc(page, 2)),
            .expand => try self.expand(),
            .collapse => self.collapse(),
            .open => {
                const t = self.tab();
                const node = t.currentNode() orelse return;
                if (node.isDir()) {
                    try t.tree.toggle(t.current().?);
                    t.stale = true;
                } else {
                    try self.apply(.pick);
                }
            },
            .collapse_all => try self.collapseAll(),
            .root_up => try self.rootUp(),
            .root_down => if (try self.currentDir()) |dir| {
                defer self.gpa.free(dir);
                try self.openRoot(dir);
            },
            .root_home => if (sys.getenv("HOME")) |home| try self.openRoot(home),

            .toggle_mark => {
                const path = self.tab().currentPath() orelse return;
                try self.marked.toggle(self.gpa, path);
                self.invalidateAll();
                self.tab().moveCursor(1);
            },
            .invert_marks => {
                const t = self.tab();
                for (t.items.items) |item| {
                    if (t.tree.pathOf(item.handle)) |p| try self.marked.toggle(self.gpa, p);
                }
                self.invalidateAll();
            },
            .clear_marks => {
                self.marked.clear(self.gpa);
                self.invalidateAll();
            },

            .yank => try self.fillClipboard(.copy),
            .cut => try self.fillClipboard(.move),
            .paste => try self.paste(),
            .delete => {
                const targets = try self.collectTargets();
                if (targets.len == 0) return self.gpa.free(targets);
                self.mode = .{ .confirm_delete = targets };
            },
            .rename => {
                const node = self.tab().currentNode() orelse return;
                const path = try self.gpa.dupe(u8, node.path);
                errdefer self.gpa.free(path);
                self.mode = .{ .prompt = .{ .kind = .{ .rename = path }, .input = try Input.init(self.gpa, node.name) } };
            },
            .create => {
                const dir = try self.gpa.dupe(u8, self.tab().targetDir() orelse return);
                errdefer self.gpa.free(dir);
                self.mode = .{ .prompt = .{ .kind = .{ .create = dir }, .input = .{} } };
            },
            .chmod => {
                const targets = try self.collectTargets();
                if (targets.len == 0) return self.gpa.free(targets);
                errdefer freePaths(self.gpa, targets);
                var buf: [8]u8 = undefined;
                const mode = if (self.tab().currentNode()) |n|
                    std.fmt.bufPrint(&buf, "{o}", .{n.meta.mode & 0o7777}) catch ""
                else
                    "";
                self.mode = .{ .prompt = .{ .kind = .{ .chmod = targets }, .input = try Input.init(self.gpa, mode) } };
            },

            .search => {
                const t = self.tab();
                if (!try t.tree.loadDeep(search_budget, t.filter.show_hidden)) {
                    self.info("large tree: search covers the first {d} entries", .{search_budget});
                }
                const previous = try self.gpa.dupe(u8, t.filter.query());
                errdefer self.gpa.free(previous);
                self.mode = .{ .prompt = .{ .kind = .{ .search = previous }, .input = try Input.init(self.gpa, previous) } };
            },
            .jump => |start| self.mode = .{ .jump = try Jump.init(self.tab(), start) },
            .filter_menu => self.mode = .{ .menu = .filter },
            .sort_menu => self.mode = .{ .menu = .sort },
            .toggle_hidden => self.filter().show_hidden = !self.filter().show_hidden,
            .toggle_dirs => self.filter().show_dirs = !self.filter().show_dirs,
            .toggle_files => self.filter().show_files = !self.filter().show_files,
            .toggle_symlinks => self.filter().show_symlinks = !self.filter().show_symlinks,
            .toggle_only_marked => self.filter().only_marked = !self.filter().only_marked,
            .reset_filter => self.filter().* = .{},
            .sort_by => |by| self.sortMut().by = by,
            .toggle_dirs_first => self.sortMut().dirs_first = !self.sortMut().dirs_first,
            .toggle_reverse => self.sortMut().reverse = !self.sortMut().reverse,
            .toggle_meta => self.show_meta = !self.show_meta,
            .refresh => {
                for (self.tabs.items) |*t| try t.tree.refreshAll();
                self.pruneMarks();
                self.invalidateAll();
                self.info("reloaded", .{});
            },

            .new_tab => {
                const dir = (try self.currentDir()) orelse
                    try self.gpa.dupe(u8, self.tab().targetDir() orelse return);
                defer self.gpa.free(dir);
                try self.newTab(dir);
            },
            .close_tab => {
                if (self.tabs.items.len == 1) return self.info("this is the last tab, quit with q", .{});
                var removed = self.tabs.orderedRemove(self.active);
                removed.deinit();
                self.active = @min(self.active, self.tabs.items.len - 1);
            },
            .next_tab => self.active = (self.active + 1) % self.tabs.items.len,
            .prev_tab => self.active = (self.active + self.tabs.items.len - 1) % self.tabs.items.len,
            .go_to_tab => |i| if (i < self.tabs.items.len) {
                self.active = i;
            },
            .toggle_pin => try self.togglePin(),
            .open_pins => try self.openPins(),

            .pick => {
                const targets = try self.collectTargets();
                defer self.gpa.free(targets);
                for (self.output.items) |p| self.gpa.free(p);
                self.output.clearRetainingCapacity();
                try self.output.appendSlice(self.gpa, targets);
                self.should_quit = self.output.items.len > 0;
            },
            .pick_dir => {
                for (self.output.items) |p| self.gpa.free(p);
                self.output.clearRetainingCapacity();
                if (self.tab().targetDir()) |dir| try self.output.append(self.gpa, try self.gpa.dupe(u8, dir));
                self.should_quit = true;
            },
            .help => self.mode = .{ .help = 0 },
            .escape => {
                if (self.tab().filter.query_len > 0) {
                    self.filter().setQuery("");
                } else if (self.marked.count() > 0) {
                    try self.apply(.clear_marks);
                } else if (self.clipboard) |*c| {
                    c.paths.deinit(self.gpa);
                    self.clipboard = null;
                } else {
                    self.should_quit = true;
                }
            },
            .quit => self.should_quit = true,
        }
    }

    fn filter(self: *App) *view.Filter {
        const t = self.tab();
        t.stale = true;
        return &t.filter;
    }

    fn sortMut(self: *App) *view.Sort {
        const t = self.tab();
        t.stale = true;
        return &t.sort;
    }

    fn reveal(self: *App, path: []const u8) !void {
        try self.tab().reveal(path, &self.marked);
    }

    /// what an operation works on: the marks, or the entry under the cursor.
    /// the caller owns the list and the paths.
    fn collectTargets(self: *App) Allocator.Error![][]u8 {
        var out: std.ArrayList([]u8) = .empty;
        errdefer {
            for (out.items) |p| self.gpa.free(p);
            out.deinit(self.gpa);
        }
        if (self.marked.count() == 0) {
            if (self.tab().currentPath()) |p| try out.append(self.gpa, try self.gpa.dupe(u8, p));
        } else {
            const sorted = try self.marked.sorted(self.gpa);
            defer self.gpa.free(sorted);
            for (sorted) |p| try out.append(self.gpa, try self.gpa.dupe(u8, p));
        }
        return out.toOwnedSlice(self.gpa);
    }

    /// the directory under the cursor, if it is one (owned by the caller)
    fn currentDir(self: *App) Allocator.Error!?[]u8 {
        const node = self.tab().currentNode() orelse return null;
        if (!node.isDir()) return null;
        return try self.gpa.dupe(u8, node.path);
    }

    fn expand(self: *App) !void {
        const t = self.tab();
        const h = t.current() orelse return;
        const node = t.tree.get(h) orelse return;
        if (!node.isDir()) return;
        if (!node.expanded) {
            try t.tree.expand(h);
            t.stale = true;
            return;
        }
        // already open: step into the first visible child
        const depth = t.items.items[t.cursor].depth;
        if (t.cursor + 1 < t.items.items.len and t.items.items[t.cursor + 1].depth > depth) t.cursor += 1;
    }

    fn collapse(self: *App) void {
        const t = self.tab();
        const h = t.current() orelse return;
        const node = t.tree.get(h) orelse return;
        if (node.expanded) {
            t.tree.collapse(h);
            t.stale = true;
        } else if (node.parent) |p| {
            t.select(p);
        }
    }

    fn collapseAll(self: *App) !void {
        const t = self.tab();
        // remember the top level entry the cursor is in
        var top = t.current();
        while (top) |h| {
            const p = t.tree.parent(h) orelse break;
            if (t.position(p) == null) break;
            top = p;
        }
        const roots = try self.gpa.dupe(Handle, t.tree.roots.items);
        defer self.gpa.free(roots);
        for (roots) |root| {
            const children = try self.gpa.dupe(Handle, t.tree.children(root));
            defer self.gpa.free(children);
            for (children) |child| try t.tree.collapseAll(child);
            if (t.kind == .pins) t.tree.collapse(root);
        }
        try t.rebuild(&self.marked);
        if (top) |h| t.select(h);
    }

    fn rootUp(self: *App) !void {
        const root = self.tab().rootPath() orelse return;
        const parent = try self.gpa.dupe(u8, sys.dirName(root) orelse return);
        defer self.gpa.free(parent);
        self.tab().setRoot(parent, &self.marked) catch |err| switch (err) {
            error.OutOfMemory => return error.OutOfMemory,
            else => self.fail("{s}: {s}", .{ parent, sys.describe(err) }),
        };
    }

    /// changes the root of a normal tab, the pins tab opens a new tab instead
    fn openRoot(self: *App, dir: []const u8) !void {
        if (self.tab().kind == .pins) return self.newTab(dir);
        self.tab().setRoot(dir, &self.marked) catch |err| switch (err) {
            error.OutOfMemory => return error.OutOfMemory,
            else => self.fail("{s}: {s}", .{ dir, sys.describe(err) }),
        };
    }

    fn newTab(self: *App, dir: []const u8) !void {
        const t = Tab.newDir(self.gpa, dir) catch |err| switch (err) {
            error.OutOfMemory => return error.OutOfMemory,
            else => return self.fail("{s}: {s}", .{ dir, sys.describe(err) }),
        };
        try self.tabs.insert(self.gpa, self.active + 1, t);
        self.active += 1;
    }

    fn togglePin(self: *App) !void {
        const dir = (try self.currentDir()) orelse
            try self.gpa.dupe(u8, self.tab().targetDir() orelse return);
        defer self.gpa.free(dir);
        const pinned = self.pins.toggle(dir) catch |err| switch (err) {
            error.OutOfMemory => return error.OutOfMemory,
            else => return self.fail("could not save pins: {s}", .{sys.describe(err)}),
        };
        self.info("{s} {s}", .{ if (pinned) "pinned" else "unpinned", dir });
        try self.syncPinsTab();
    }

    /// keeps the roots of an open pins tab in line with the pin list
    fn syncPinsTab(self: *App) !void {
        for (self.tabs.items) |*t| {
            if (t.kind != .pins) continue;
            const roots = try self.gpa.dupe(Handle, t.tree.roots.items);
            defer self.gpa.free(roots);
            for (roots) |root| {
                if (!self.pins.contains(t.tree.pathOf(root).?)) t.tree.removeRoot(root);
            }
            for (self.pins.list()) |pin| {
                const present = for (t.tree.roots.items) |r| {
                    if (std.mem.eql(u8, t.tree.pathOf(r).?, pin)) break true;
                } else false;
                if (!present) _ = t.tree.addRoot(pin) catch |err| switch (err) {
                    error.OutOfMemory => return error.OutOfMemory,
                    else => {},
                };
            }
            t.stale = true;
        }
    }

    fn openPins(self: *App) !void {
        for (self.tabs.items, 0..) |t, i| if (t.kind == .pins) {
            self.active = i;
            return;
        };
        try self.tabs.append(self.gpa, try Tab.newPins(self.gpa, self.pins.list()));
        self.active = self.tabs.items.len - 1;
        if (self.pins.list().len == 0) self.info("no pins yet, pin a directory with b", .{});
    }

    fn fillClipboard(self: *App, kind: ops.TransferKind) !void {
        const targets = try self.collectTargets();
        defer freePaths(self.gpa, targets);
        if (targets.len == 0) return;
        var paths: PathSet = .{};
        errdefer paths.deinit(self.gpa);
        for (targets) |p| try paths.insert(self.gpa, p);
        if (self.clipboard) |*c| c.paths.deinit(self.gpa);
        self.clipboard = .{ .paths = paths, .kind = kind };
        self.info("{d} item(s) to {s}, paste with p", .{ targets.len, if (kind == .copy) "copy" else "move" });
        self.marked.clear(self.gpa);
        self.invalidateAll();
    }

    fn paste(self: *App) !void {
        var clipboard = self.clipboard orelse return self.info("clipboard is empty", .{});
        self.clipboard = null;
        const dest = try self.gpa.dupe(u8, self.tab().targetDir() orelse {
            self.clipboard = clipboard;
            return;
        });
        defer self.gpa.free(dest);

        const sources = try clipboard.paths.sorted(self.gpa);
        defer self.gpa.free(sources);
        var created: std.ArrayList([]u8) = .empty;
        defer {
            for (created.items) |p| self.gpa.free(p);
            created.deinit(self.gpa);
        }
        var errors: std.ArrayList(u8) = .empty;
        defer errors.deinit(self.gpa);
        for (sources) |src| {
            if (ops.transfer(self.gpa, src, dest, clipboard.kind)) |new| {
                try created.append(self.gpa, new);
            } else |err| {
                if (errors.items.len > 0) try errors.appendSlice(self.gpa, ", ");
                try errors.print(self.gpa, "{s}: {s}", .{ sys.baseName(src), sys.describe(err) });
            }
        }

        var changed: std.ArrayList([]const u8) = .empty;
        defer changed.deinit(self.gpa);
        try changed.append(self.gpa, dest);
        if (clipboard.kind == .move) {
            for (sources) |src| if (sys.dirName(src)) |p| try changed.append(self.gpa, p);
        }
        try self.afterChange(changed.items);
        if (clipboard.kind == .move) {
            clipboard.paths.deinit(self.gpa);
        } else {
            // a copy can be pasted again
            self.clipboard = clipboard;
        }

        if (created.items.len > 0) try self.reveal(created.items[0]);
        if (errors.items.len == 0) {
            self.info("pasted {d} item(s) into {s}", .{ created.items.len, dest });
        } else {
            self.fail("{s}", .{errors.items});
        }
    }

    fn delete(self: *App, paths: []const []u8) !void {
        var errors: std.ArrayList(u8) = .empty;
        defer errors.deinit(self.gpa);
        for (paths) |path| {
            ops.remove(self.gpa, path) catch |err| {
                if (errors.items.len > 0) try errors.appendSlice(self.gpa, ", ");
                try errors.print(self.gpa, "{s}: {s}", .{ sys.baseName(path), sys.describe(err) });
            };
            self.pins.remove(path) catch {};
        }
        if (self.clipboard) |*c| c.paths.retain(self.gpa, sys.exists);
        try self.afterChangeParents(paths);
        try self.syncPinsTab();
        if (errors.items.len == 0) {
            self.info("deleted {d} item(s)", .{paths.len});
        } else {
            self.fail("{s}", .{errors.items});
        }
    }

    fn afterChangeParents(self: *App, paths: []const []u8) !void {
        var dirs: std.ArrayList([]const u8) = .empty;
        defer dirs.deinit(self.gpa);
        for (paths) |p| if (sys.dirName(p)) |d| try dirs.append(self.gpa, d);
        try self.afterChange(dirs.items);
    }

    /// re-reads the given directories in every tab that has them loaded
    fn afterChange(self: *App, dirs: []const []const u8) !void {
        // the paths might be borrowed from nodes that the refresh frees
        var owned: std.ArrayList([]u8) = .empty;
        defer {
            for (owned.items) |p| self.gpa.free(p);
            owned.deinit(self.gpa);
        }
        for (dirs) |d| try owned.append(self.gpa, try self.gpa.dupe(u8, d));

        for (self.tabs.items) |*t| {
            for (owned.items) |dir| {
                if (t.tree.find(dir)) |h| try t.tree.refresh(h);
            }
        }
        self.pruneMarks();
        self.invalidateAll();
    }

    fn pruneMarks(self: *App) void {
        self.marked.retain(self.gpa, sys.exists);
    }

    /// takes ownership of the jump, it either ends here or goes back into
    /// `self.mode`
    fn handleJump(self: *App, j: *Jump, key: Key) !void {
        const t = self.tab();
        var step: Step = .@"continue";
        switch (key.code) {
            .esc => step = j.finish(t),
            .enter => if (j.current()) |h| {
                step = try j.accept(t, h);
            },
            .up, .backtab => j.moveCursor(-1),
            .down, .tab => j.moveCursor(1),
            .backspace => if (j.input.text().len == 0) {
                try j.up(t);
            } else if (try j.input.handle(self.gpa, key)) {
                try j.update(t);
            },
            .char => |c| if (key.ctrl and c == 'p') {
                j.moveCursor(-1);
            } else if (key.ctrl and c == 'n') {
                j.moveCursor(1);
            } else if (try j.input.handle(self.gpa, key)) {
                try j.update(t);
            },
            else => if (try j.input.handle(self.gpa, key)) try j.update(t),
        }

        switch (step) {
            .@"continue" => self.mode = .{ .jump = j.* },
            .done => |target| {
                defer j.deinit(self.gpa);
                const h = target orelse return;
                const path = try self.gpa.dupe(u8, t.tree.pathOf(h) orelse return);
                defer self.gpa.free(path);
                if (t.tree.get(h).?.isDir()) try t.tree.expand(h);
                try t.reveal(path, &self.marked);
            },
        }
    }
};

pub fn freePaths(gpa: Allocator, paths: []const []u8) void {
    for (paths) |p| gpa.free(p);
    gpa.free(paths);
}

const testing = std.testing;
const TmpDir = @import("testutil.zig").TmpDir;

const Fixture = struct {
    dir: TmpDir,
    app: App,

    /// root/{a/{one.txt, two.txt}, b/, c.txt}
    fn init() !Fixture {
        var dir = try TmpDir.init(testing.allocator);
        errdefer dir.deinit();
        try dir.mkdir("a");
        try dir.mkdir("b");
        try dir.write("a/one.txt", "1");
        try dir.write("a/two.txt", "2");
        try dir.write("c.txt", "c");
        var app = try App.init(testing.allocator, &.{dir.root}, Pins.inMemory(testing.allocator));
        try app.prepare();
        return .{ .dir = dir, .app = app };
    }

    fn deinit(self: *Fixture) void {
        self.app.deinit();
        self.dir.deinit();
    }

    fn keys(self: *Fixture, text: []const u8) !void {
        var it = std.unicode.Utf8View.initUnchecked(text).iterator();
        while (it.nextCodepoint()) |c| {
            const key: Key = switch (c) {
                '\n' => Key.of(.enter),
                0x1b => Key.of(.esc),
                0x7f => Key.of(.backspace),
                else => Key.char(c),
            };
            try self.app.handleKey(key);
        }
    }

    fn press(self: *Fixture, k: Key) !void {
        try self.app.handleKey(k);
    }

    fn expectVisible(self: *Fixture, expected: []const []const u8) !void {
        var buf: std.ArrayList(u8) = .empty;
        defer buf.deinit(testing.allocator);
        const t = self.app.tab();
        for (t.items.items) |item| {
            for (0..item.depth) |_| try buf.appendSlice(testing.allocator, "  ");
            try buf.appendSlice(testing.allocator, t.tree.get(item.handle).?.name);
            try buf.append(testing.allocator, '|');
        }
        var want: std.ArrayList(u8) = .empty;
        defer want.deinit(testing.allocator);
        for (expected) |e| {
            try want.appendSlice(testing.allocator, e);
            try want.append(testing.allocator, '|');
        }
        try testing.expectEqualStrings(want.items, buf.items);
    }

    fn expectCurrent(self: *Fixture, rel: []const u8) !void {
        const want = try self.dir.path(rel);
        defer testing.allocator.free(want);
        try testing.expectEqualStrings(want, self.app.tab().currentPath().?);
    }

    fn expectJumpLevel(self: *Fixture, expected: []const []const u8) !void {
        const j = &self.app.mode.jump;
        try testing.expectEqual(expected.len, j.matches.items.len);
        for (j.matches.items, expected) |h, name| {
            try testing.expectEqualStrings(name, self.app.tab().tree.get(h).?.name);
        }
    }
};

test "navigate and expand" {
    var f = try Fixture.init();
    defer f.deinit();
    try f.expectVisible(&.{ "a", "b", "c.txt" });
    try f.keys("l");
    try f.expectVisible(&.{ "a", "  one.txt", "  two.txt", "b", "c.txt" });
    try f.keys("lj");
    try f.expectCurrent("a/two.txt");
    try f.keys("h");
    try f.expectCurrent("a");
    try f.keys("h");
    try f.expectVisible(&.{ "a", "b", "c.txt" });
    try f.keys("G");
    try f.expectCurrent("c.txt");
}

test "copy between tabs" {
    var f = try Fixture.init();
    defer f.deinit();
    // mark both files in a
    try f.keys("lj  ");
    try testing.expectEqual(2, f.app.marked.count());
    try f.keys("y");
    try testing.expectEqual(0, f.app.marked.count());

    // open a second tab rooted at b and paste there
    try f.keys("gjjjt");
    try testing.expectEqual(2, f.app.tabs.items.len);
    try testing.expectEqualStrings("b", f.app.tab().title());
    try f.keys("p");
    try testing.expect(f.dir.exists("b/one.txt") and f.dir.exists("b/two.txt") and f.dir.exists("a/one.txt"));
    try f.expectVisible(&.{ "one.txt", "two.txt" });

    // the first tab sees the new files as well once b is opened there
    try f.keys("[");
    try testing.expectEqual(0, f.app.active);
    try f.keys("ggjjjl");
    try f.expectCurrent("b");
    try f.expectVisible(&.{ "a", "  one.txt", "  two.txt", "b", "  one.txt", "  two.txt", "c.txt" });
}

test "cut and paste moves" {
    var f = try Fixture.init();
    defer f.deinit();
    try f.keys("Gx");
    try f.keys("ggp");
    try testing.expect(!f.dir.exists("c.txt") and f.dir.exists("a/c.txt"));
    try testing.expect(f.app.clipboard == null);
    try f.expectCurrent("a/c.txt");
}

test "delete needs confirmation" {
    var f = try Fixture.init();
    defer f.deinit();
    try f.keys("Gdn");
    try testing.expect(f.dir.exists("c.txt"));
    try f.keys("dy");
    try testing.expect(!f.dir.exists("c.txt"));
    try f.expectVisible(&.{ "a", "b" });
}

test "rename, create, chmod" {
    var f = try Fixture.init();
    defer f.deinit();
    try f.keys("Gr");
    try f.press(Key.ctrlChar('u'));
    try f.keys("d.md\n");
    try testing.expect(f.dir.exists("d.md"));
    try f.expectCurrent("d.md");

    try f.keys("ggjanew/\n");
    try testing.expect(f.dir.exists("b/new"));
    try f.expectCurrent("b/new");

    try f.keys("Gc");
    try f.press(Key.ctrlChar('u'));
    try f.keys("600\n");
    const p = try f.dir.path("d.md");
    defer testing.allocator.free(p);
    try testing.expectEqual(0o600, (try sys.lstat(p)).mode & 0o777);
}

test "search and escape" {
    var f = try Fixture.init();
    defer f.deinit();
    try f.keys("/two");
    try f.expectVisible(&.{ "a", "  two.txt" });
    try f.keys("\n");
    try testing.expectEqualStrings("two", f.app.tab().filter.query());
    try f.keys("\x1b");
    try testing.expectEqualStrings("", f.app.tab().filter.query());
    try f.expectVisible(&.{ "a", "b", "c.txt" });
    try testing.expect(!f.app.should_quit);
    try f.keys("\x1b");
    try testing.expect(f.app.should_quit);
}

test "filter and sort menus" {
    var f = try Fixture.init();
    defer f.deinit();
    try f.keys("ff");
    try f.expectVisible(&.{ "a", "b" });
    try f.keys("fr");
    try f.keys("sr");
    try f.expectVisible(&.{ "b", "a", "c.txt" });
    try f.keys("sd");
    try f.expectVisible(&.{ "c.txt", "b", "a" });
}

test "only marked filter" {
    var f = try Fixture.init();
    defer f.deinit();
    try f.keys("jj fm");
    try f.expectVisible(&.{"c.txt"});
}

test "root up and down keep state" {
    var f = try Fixture.init();
    defer f.deinit();
    try f.keys("lL");
    try testing.expectEqualStrings("a", f.app.tab().title());
    try f.expectVisible(&.{ "one.txt", "two.txt" });
    try f.keys("H");
    try testing.expectEqualStrings(f.dir.root, f.app.tab().rootPath().?);
    try f.expectCurrent("a");
    try f.expectVisible(&.{ "a", "  one.txt", "  two.txt", "b", "c.txt" });
}

test "pins tab" {
    var f = try Fixture.init();
    defer f.deinit();
    try f.keys("bjb");
    try testing.expectEqual(2, f.app.pins.list().len);
    try f.keys("B");
    try testing.expectEqual(.pins, f.app.tab().kind);
    try f.expectVisible(&.{ "a", "b" });
    try f.keys("l");
    try f.expectVisible(&.{ "a", "  one.txt", "  two.txt", "b" });
    // unpinning from the pins tab updates it right away
    try f.keys("Gb");
    try f.expectVisible(&.{ "a", "  one.txt", "  two.txt" });
}

test "pick prints marks or cursor" {
    {
        var f = try Fixture.init();
        defer f.deinit();
        try f.keys("G\n");
        try testing.expect(f.app.should_quit);
        try testing.expectEqualStrings("c.txt", sys.baseName(f.app.output.items[0]));
    }
    {
        var f = try Fixture.init();
        defer f.deinit();
        try f.keys(" j o");
        try testing.expectEqual(2, f.app.output.items.len);
        try testing.expectEqualStrings("a", sys.baseName(f.app.output.items[0]));
        try testing.expectEqualStrings("c.txt", sys.baseName(f.app.output.items[1]));
    }
    {
        var f = try Fixture.init();
        defer f.deinit();
        try f.keys("GO");
        try testing.expectEqualStrings(f.dir.root, f.app.output.items[0]);
    }
}

test "jump from root through dirs to a file" {
    var f = try Fixture.init();
    defer f.deinit();
    // the cursor is deep in the tree, `e` still starts at the top level
    try f.keys("lje");
    try f.expectJumpLevel(&.{ "a", "b", "c.txt" });
    try f.keys("a");
    try f.expectJumpLevel(&.{"a"});
    try f.keys("\n");
    try f.expectJumpLevel(&.{ "one.txt", "two.txt" });
    try f.keys("tw");
    try f.expectJumpLevel(&.{"two.txt"});
    try f.keys("\n");
    try testing.expect(f.app.mode == .normal);
    try f.expectCurrent("a/two.txt");
    try f.expectVisible(&.{ "a", "  one.txt", "  two.txt", "b", "c.txt" });
}

test "jump here starts on the cursor level" {
    var f = try Fixture.init();
    defer f.deinit();
    try f.keys("ljE");
    try f.expectJumpLevel(&.{ "one.txt", "two.txt" });
    try f.keys("\x1b");
}

test "jump escape stays in the dir" {
    var f = try Fixture.init();
    defer f.deinit();
    try f.keys("ea\n\x1b");
    try testing.expect(f.app.mode == .normal);
    try f.expectCurrent("a");
    try testing.expect(f.app.tab().currentNode().?.expanded);
}

test "jump backspace and cursor" {
    var f = try Fixture.init();
    defer f.deinit();
    try f.keys("e.");
    try f.expectJumpLevel(&.{"c.txt"});
    try f.keys("\x7f\n");
    try f.expectJumpLevel(&.{ "one.txt", "two.txt" });
    // backspace with an empty input goes one level up again
    try f.keys("\x7f");
    try f.expectJumpLevel(&.{ "a", "b", "c.txt" });
    try f.press(Key.of(.down));
    try f.press(Key.of(.down));
    try f.keys("\n");
    try f.expectCurrent("c.txt");
}
