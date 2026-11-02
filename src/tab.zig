const std = @import("std");
const Allocator = std.mem.Allocator;

const Handle = @import("arena.zig").Handle;
const Node = @import("fs/node.zig").Node;
const FileTree = @import("fs/tree.zig").FileTree;
const PathSet = @import("pathset.zig").PathSet;
const sys = @import("sys.zig");
const view = @import("view.zig");

pub const Kind = enum {
    /// a normal tab rooted at one directory
    dir,
    /// the pinned folders, each one is a root of the tree
    pins,
};

/// one tab: its own tree, its own view on it and its own cursor.
///
/// marks and the clipboard are not part of a tab, they are global so files
/// can be marked in one tab and pasted in another.
pub const Tab = struct {
    gpa: Allocator,
    kind: Kind,
    tree: FileTree,
    items: std.ArrayList(view.ViewItem) = .empty,
    cursor: usize = 0,
    /// first visible row, kept up to date by the ui
    offset: usize = 0,
    filter: view.Filter = .{},
    sort: view.Sort = .{},
    /// the view list has to be rebuilt before it is used next
    stale: bool = true,

    pub fn newDir(gpa: Allocator, path: []const u8) !Tab {
        return .{ .gpa = gpa, .kind = .dir, .tree = try FileTree.withRoot(gpa, path) };
    }

    pub fn newPins(gpa: Allocator, pins: []const []const u8) Allocator.Error!Tab {
        var tree = FileTree.init(gpa);
        for (pins) |pin| {
            // pins that vanished from disk are skipped silently
            _ = tree.addRoot(pin) catch |err| switch (err) {
                error.OutOfMemory => return error.OutOfMemory,
                else => {},
            };
        }
        return .{ .gpa = gpa, .kind = .pins, .tree = tree };
    }

    pub fn deinit(self: *Tab) void {
        self.tree.deinit();
        self.items.deinit(self.gpa);
    }

    pub fn title(self: *const Tab) []const u8 {
        return switch (self.kind) {
            .pins => "★ pins",
            .dir => sys.baseName(self.rootPath() orelse ""),
        };
    }

    /// root of a normal tab
    pub fn rootPath(self: *const Tab) ?[]const u8 {
        if (self.kind != .dir or self.tree.roots.items.len == 0) return null;
        return self.tree.pathOf(self.tree.roots.items[0]);
    }

    pub fn rebuild(self: *Tab, marked: *const PathSet) Allocator.Error!void {
        const previous = self.current();
        try view.build(self.gpa, &self.tree, .{
            .filter = &self.filter,
            .sort = self.sort,
            .marked = marked,
        }, self.kind == .pins, &self.items);
        self.stale = false;

        // keep the cursor on the same node if it is still visible
        if (previous) |h| if (self.position(h)) |pos| {
            self.cursor = pos;
        };
        self.cursor = @min(self.cursor, self.items.items.len -| 1);
    }

    pub fn position(self: *const Tab, handle: Handle) ?usize {
        for (self.items.items, 0..) |item, i| if (item.handle.eql(handle)) return i;
        return null;
    }

    pub fn current(self: *const Tab) ?Handle {
        if (self.cursor >= self.items.items.len) return null;
        return self.items.items[self.cursor].handle;
    }

    pub fn currentNode(self: *const Tab) ?*const Node {
        return self.tree.get(self.current() orelse return null);
    }

    /// borrowed from the tree, only valid until the tree changes
    pub fn currentPath(self: *const Tab) ?[]const u8 {
        const node = self.currentNode() orelse return null;
        return node.path;
    }

    /// the directory something should be created in or pasted into: the
    /// directory under the cursor, or the parent of the file under it.
    /// borrowed from the tree.
    pub fn targetDir(self: *const Tab) ?[]const u8 {
        const node = self.currentNode() orelse return self.rootPath();
        if (node.isDir()) return node.path;
        return sys.dirName(node.path);
    }

    pub fn moveCursor(self: *Tab, delta: isize) void {
        const last: isize = @intCast(self.items.items.len -| 1);
        const pos = std.math.clamp(@as(isize, @intCast(self.cursor)) +| delta, 0, last);
        self.cursor = @intCast(pos);
    }

    pub fn select(self: *Tab, handle: Handle) void {
        if (self.position(handle)) |pos| self.cursor = pos;
    }

    /// expands the way to `path` and puts the cursor on it
    pub fn reveal(self: *Tab, path: []const u8, marked: *const PathSet) Allocator.Error!void {
        const owned = try self.gpa.dupe(u8, path);
        defer self.gpa.free(owned);
        if (try self.tree.reveal(owned)) |h| {
            try self.rebuild(marked);
            self.select(h);
        }
    }

    /// makes `path` the new root of this tab. directories that were expanded
    /// before stay expanded if they are part of the new tree.
    pub fn setRoot(self: *Tab, path: []const u8, marked: *const PathSet) !void {
        const old_root = if (self.rootPath()) |p| try self.gpa.dupe(u8, p) else null;
        defer if (old_root) |p| self.gpa.free(p);
        const expanded = try self.tree.expandedPaths(self.gpa);
        defer {
            for (expanded) |p| self.gpa.free(p);
            self.gpa.free(expanded);
        }

        var tree = try FileTree.withRoot(self.gpa, path);
        errdefer tree.deinit();
        for (expanded) |dir| {
            if (try tree.reveal(dir)) |h| try tree.expand(h);
        }
        self.tree.deinit();
        self.tree = tree;
        self.cursor = 0;
        self.offset = 0;
        try self.rebuild(marked);

        // after `cd ..` the cursor should be on the directory we came from
        if (old_root) |p| if (self.tree.find(p)) |h| self.select(h);
    }
};
