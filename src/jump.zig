const std = @import("std");
const Allocator = std.mem.Allocator;

const Handle = @import("arena.zig").Handle;
const Input = @import("input.zig").Input;
const Tab = @import("tab.zig").Tab;
const view = @import("view.zig");

/// where a jump starts
pub const Start = enum {
    /// the top level of the tab
    root,
    /// the level of the entry under the cursor
    here,
};

/// what the app has to do after a jump step
pub const Step = union(enum) {
    /// still jumping
    @"continue",
    /// the jump ended on this node (a file, or the directory we stopped in)
    done: ?Handle,
};

/// quick navigation, level by level.
///
/// one level of the tree is shown flat and filtered by what the user types.
/// `enter` on a directory shows its content as the next level, `enter` on a
/// file ends the jump with the cursor on it.
///
/// the jump only looks at the tree, it never changes the real view. only when
/// it ends the result is revealed in the tab.
pub const Jump = struct {
    /// the directory whose content is shown. `null` is the level of the roots
    /// in the pins tab.
    dir: ?Handle,
    input: Input = .{},
    matches: std.ArrayList(Handle) = .empty,
    cursor: usize = 0,

    pub fn init(tab: *Tab, start: Start) Allocator.Error!Jump {
        // the top level of a normal tab is the content of its root, the top
        // level of the pins tab are the roots themselves
        const top: ?Handle = if (tab.rootPath() != null) tab.tree.roots.items[0] else null;
        const dir = switch (start) {
            .here => if (tab.current()) |h| tab.tree.parent(h) else top,
            .root => top,
        };
        var jump: Jump = .{ .dir = dir };
        try jump.update(tab);
        return jump;
    }

    pub fn deinit(self: *Jump, gpa: Allocator) void {
        self.input.deinit(gpa);
        self.matches.deinit(gpa);
    }

    const SortCtx = struct {
        tab: *const Tab,
        fn lessThan(self: SortCtx, a: Handle, b: Handle) bool {
            return self.tab.sort.lessThan(self.tab.tree.get(a).?, self.tab.tree.get(b).?);
        }
    };

    /// the entries of the current level that match the input, sorted like the
    /// tab. the cursor goes to the first one that starts with the input,
    /// that is usually the one the user is aiming for.
    pub fn update(self: *Jump, tab: *Tab) Allocator.Error!void {
        if (self.dir) |dir| {
            if (tab.tree.get(dir)) |n| if (n.children == null) try tab.tree.loadChildren(dir);
        }
        const level = if (self.dir) |dir| tab.tree.children(dir) else tab.tree.roots.items;
        const query = self.input.text();

        self.matches.clearRetainingCapacity();
        for (level) |h| {
            const n = tab.tree.get(h) orelse continue;
            const hidden_ok = tab.filter.show_hidden or !n.isHidden() or std.mem.startsWith(u8, query, ".");
            if (hidden_ok and (query.len == 0 or view.matchRange(query, n.name) != null)) {
                try self.matches.append(tab.gpa, h);
            }
        }
        std.mem.sort(Handle, self.matches.items, SortCtx{ .tab = tab }, SortCtx.lessThan);

        self.cursor = 0;
        for (self.matches.items, 0..) |h, i| {
            if (std.ascii.startsWithIgnoreCase(tab.tree.get(h).?.name, query)) {
                self.cursor = i;
                break;
            }
        }
    }

    pub fn current(self: *const Jump) ?Handle {
        if (self.cursor >= self.matches.items.len) return null;
        return self.matches.items[self.cursor];
    }

    pub fn moveCursor(self: *Jump, delta: isize) void {
        const last: isize = @intCast(self.matches.items.len -| 1);
        self.cursor = @intCast(std.math.clamp(@as(isize, @intCast(self.cursor)) + delta, 0, last));
    }

    /// enters a directory or ends the jump on a file
    pub fn accept(self: *Jump, tab: *Tab, handle: Handle) Allocator.Error!Step {
        const node = tab.tree.get(handle) orelse return .@"continue";
        if (!node.isDir()) return .{ .done = handle };
        self.dir = handle;
        self.input.clear();
        try self.update(tab);
        return .@"continue";
    }

    /// one level up, but never above the root of a normal tab
    pub fn up(self: *Jump, tab: *Tab) Allocator.Error!void {
        const dir = self.dir orelse return;
        const parent = tab.tree.parent(dir);
        if (parent == null and tab.rootPath() != null) return;
        self.dir = parent;
        self.input.clear();
        try self.update(tab);
        // keep the cursor on the directory we came from
        for (self.matches.items, 0..) |h, i| if (h.eql(dir)) {
            self.cursor = i;
        };
    }

    /// the user is done: stay in the directory the jump is in
    pub fn finish(self: *const Jump, tab: *const Tab) Step {
        const dir = self.dir orelse return .{ .done = null };
        const is_tab_root = tab.rootPath() != null and tab.tree.isRoot(dir);
        return .{ .done = if (is_tab_root) null else dir };
    }
};
