const std = @import("std");
const Allocator = std.mem.Allocator;

const arena_mod = @import("../arena.zig");
const Arena = arena_mod.Arena;
const Handle = arena_mod.Handle;
const node_mod = @import("node.zig");
const Node = node_mod.Node;
const sys = @import("../sys.zig");

/// the model: everything we know about the file system, stored in an arena.
///
/// nodes reference each other through handles instead of pointers, so we can
/// keep stable references to them (view list, cursor) and walk up to the
/// parent.
///
/// a tree can have multiple roots. a normal tab has exactly one, the pins tab
/// has one per pinned folder.
///
/// children are stored in the order the os returns them (sorted by name).
/// sorting and filtering is a concern of the view (see `view.zig`) so the
/// model never has to change when the user only changes how things are
/// displayed.
///
/// pointers returned by `get` are only valid until the next node is inserted.
pub const FileTree = struct {
    gpa: Allocator,
    arena: Arena(Node) = .{},
    roots: std.ArrayList(Handle) = .empty,

    pub fn init(gpa: Allocator) FileTree {
        return .{ .gpa = gpa };
    }

    /// creates a tree with a single, already expanded root
    pub fn withRoot(gpa: Allocator, path: []const u8) !FileTree {
        var tree = init(gpa);
        errdefer tree.deinit();
        const root = try tree.addRoot(path);
        try tree.expand(root);
        return tree;
    }

    pub fn deinit(self: *FileTree) void {
        for (self.roots.items) |root| self.removeSubtree(root);
        self.roots.deinit(self.gpa);
        self.arena.deinit(self.gpa);
    }

    pub fn addRoot(self: *FileTree, path: []const u8) !Handle {
        const real = try sys.realPath(self.gpa, path);
        var node = try Node.fromOwnedPath(self.gpa, real, null);
        errdefer node.deinit(self.gpa);
        const handle = try self.arena.insert(self.gpa, node);
        try self.roots.append(self.gpa, handle);
        return handle;
    }

    pub fn removeRoot(self: *FileTree, handle: Handle) void {
        for (self.roots.items, 0..) |r, i| {
            if (r.eql(handle)) {
                _ = self.roots.orderedRemove(i);
                break;
            }
        }
        self.removeSubtree(handle);
    }

    pub fn isRoot(self: *const FileTree, handle: Handle) bool {
        for (self.roots.items) |r| if (r.eql(handle)) return true;
        return false;
    }

    pub fn get(self: *const FileTree, handle: Handle) ?*const Node {
        return self.arena.get(handle);
    }

    fn getMut(self: *FileTree, handle: Handle) ?*Node {
        return self.arena.getMut(handle);
    }

    pub fn pathOf(self: *const FileTree, handle: Handle) ?[]const u8 {
        const node = self.get(handle) orelse return null;
        return node.path;
    }

    pub fn parent(self: *const FileTree, handle: Handle) ?Handle {
        const node = self.get(handle) orelse return null;
        return node.parent;
    }

    /// loaded children of a node, empty if the node is a file or was not read
    /// yet. only valid until the tree changes.
    pub fn children(self: *const FileTree, handle: Handle) []const Handle {
        const node = self.get(handle) orelse return &.{};
        return node.childList();
    }

    pub fn len(self: *const FileTree) usize {
        return self.arena.len();
    }

    /// reads the directory behind `handle` and stores its entries as
    /// children. errors are stored on the node so the ui can show them.
    pub fn loadChildren(self: *FileTree, handle: Handle) Allocator.Error!void {
        const node = self.get(handle) orelse return;
        if (!node.isDir()) return;
        const dir_path = try self.gpa.dupe(u8, node.path);
        defer self.gpa.free(dir_path);

        var list: std.ArrayList(Handle) = .empty;
        errdefer list.deinit(self.gpa);
        var read_err: ?anyerror = null;

        if (sys.listDir(self.gpa, dir_path)) |names| {
            defer sys.freeNames(self.gpa, names);
            for (names) |name| {
                const p = try sys.joinPath(self.gpa, dir_path, name);
                var child = Node.fromOwnedPath(self.gpa, p, handle) catch |err| switch (err) {
                    error.OutOfMemory => return error.OutOfMemory,
                    // vanished while reading, ignore it
                    else => continue,
                };
                errdefer child.deinit(self.gpa);
                try list.append(self.gpa, try self.arena.insert(self.gpa, child));
            }
        } else |err| switch (err) {
            error.OutOfMemory => return error.OutOfMemory,
            else => read_err = err,
        }

        const n = self.getMut(handle).?;
        if (n.children) |*old| old.deinit(self.gpa);
        n.children = list;
        n.err = read_err;
    }

    pub fn expand(self: *FileTree, handle: Handle) Allocator.Error!void {
        const node = self.get(handle) orelse return;
        if (!node.isDir()) return;
        if (node.children == null) try self.loadChildren(handle);
        self.getMut(handle).?.expanded = true;
    }

    pub fn collapse(self: *FileTree, handle: Handle) void {
        if (self.getMut(handle)) |n| n.expanded = false;
    }

    pub fn toggle(self: *FileTree, handle: Handle) Allocator.Error!void {
        const node = self.get(handle) orelse return;
        if (node.expanded) self.collapse(handle) else try self.expand(handle);
    }

    /// collapses the node and all of its loaded descendants
    pub fn collapseAll(self: *FileTree, handle: Handle) Allocator.Error!void {
        var stack: std.ArrayList(Handle) = .empty;
        defer stack.deinit(self.gpa);
        try stack.append(self.gpa, handle);
        while (stack.pop()) |h| {
            try stack.appendSlice(self.gpa, self.children(h));
            self.collapse(h);
        }
    }

    /// re-reads a directory from disk while keeping the handles (and with
    /// them the expanded state) of entries that still exist. loaded
    /// descendants are refreshed as well.
    pub fn refresh(self: *FileTree, handle: Handle) Allocator.Error!void {
        const node = self.get(handle) orelse return;
        const dir_path = try self.gpa.dupe(u8, node.path);
        defer self.gpa.free(dir_path);

        // update the node itself, it might have been chmod'ed or replaced
        if (sys.lstat(dir_path)) |st| {
            const kind = try node_mod.kindOf(self.gpa, dir_path, st.mode);
            const n = self.getMut(handle).?;
            if (!n.kind.sameAs(kind)) {
                // a directory became a file (or the other way around)
                if (n.kind == .symlink) self.gpa.free(n.kind.symlink.target);
                n.kind = kind;
                n.expanded = false;
                if (n.children) |*old| {
                    const items = try self.gpa.dupe(Handle, old.items);
                    defer self.gpa.free(items);
                    old.deinit(self.gpa);
                    n.children = null;
                    for (items) |child| self.removeSubtree(child);
                }
            } else if (kind == .symlink) {
                self.gpa.free(kind.symlink.target);
            }
            self.getMut(handle).?.meta = node_mod.metaOf(st);
        } else |_| {}

        const loaded = self.get(handle).?.children orelse return;
        const old_children = try self.gpa.dupe(Handle, loaded.items);
        defer self.gpa.free(old_children);

        const names = sys.listDir(self.gpa, dir_path) catch |err| switch (err) {
            error.OutOfMemory => return error.OutOfMemory,
            else => {
                for (old_children) |child| self.removeSubtree(child);
                const n = self.getMut(handle).?;
                n.children.?.clearRetainingCapacity();
                n.err = err;
                return;
            },
        };
        defer sys.freeNames(self.gpa, names);

        // the path keys borrow from the nodes, they stay alive until the
        // nodes are removed at the very end
        var existing: std.StringHashMapUnmanaged(Handle) = .empty;
        defer existing.deinit(self.gpa);
        for (old_children) |h| {
            if (self.get(h)) |n| try existing.put(self.gpa, n.path, h);
        }

        var list: std.ArrayList(Handle) = .empty;
        errdefer list.deinit(self.gpa);
        for (names) |name| {
            const p = try sys.joinPath(self.gpa, dir_path, name);
            if (existing.fetchRemove(p)) |kv| {
                self.gpa.free(p);
                try list.append(self.gpa, kv.value);
                try self.refresh(kv.value);
            } else {
                var child = Node.fromOwnedPath(self.gpa, p, handle) catch |err| switch (err) {
                    error.OutOfMemory => return error.OutOfMemory,
                    else => continue,
                };
                errdefer child.deinit(self.gpa);
                try list.append(self.gpa, try self.arena.insert(self.gpa, child));
            }
        }

        // whatever is left vanished from disk
        var gone: std.ArrayList(Handle) = .empty;
        defer gone.deinit(self.gpa);
        var it = existing.valueIterator();
        while (it.next()) |h| try gone.append(self.gpa, h.*);
        existing.clearRetainingCapacity();
        for (gone.items) |h| self.removeSubtree(h);

        const n = self.getMut(handle).?;
        n.children.?.deinit(self.gpa);
        n.children = list;
        n.err = null;
    }

    pub fn refreshAll(self: *FileTree) Allocator.Error!void {
        const roots = try self.gpa.dupe(Handle, self.roots.items);
        defer self.gpa.free(roots);
        for (roots) |r| try self.refresh(r);
    }

    /// removes the node and everything below it from the arena. the caller
    /// is responsible for unlinking it from its parent.
    fn removeSubtree(self: *FileTree, handle: Handle) void {
        var stack: std.ArrayList(Handle) = .empty;
        defer stack.deinit(self.gpa);
        stack.append(self.gpa, handle) catch return;
        while (stack.pop()) |h| {
            var node = self.arena.remove(h) orelse continue;
            stack.appendSlice(self.gpa, node.childList()) catch {};
            node.deinit(self.gpa);
        }
    }

    fn rootFor(self: *const FileTree, target: []const u8) ?Handle {
        for (self.roots.items) |root| {
            if (sys.isWithin(target, self.pathOf(root).?)) return root;
        }
        return null;
    }

    fn childNamed(self: *const FileTree, handle: Handle, name: []const u8) ?Handle {
        for (self.children(handle)) |c| {
            if (std.mem.eql(u8, self.get(c).?.name, name)) return c;
        }
        return null;
    }

    /// finds an already loaded node by its path
    pub fn find(self: *const FileTree, target: []const u8) ?Handle {
        for (self.roots.items) |root| {
            const root_path = self.pathOf(root).?;
            if (!sys.isWithin(target, root_path)) continue;
            var current = root;
            var it = std.mem.tokenizeScalar(u8, target[root_path.len..], '/');
            const found = while (it.next()) |component| {
                current = self.childNamed(current, component) orelse break false;
            } else true;
            if (found) return current;
        }
        return null;
    }

    /// like `find`, but loads and expands every directory on the way so the
    /// node becomes visible
    pub fn reveal(self: *FileTree, target: []const u8) Allocator.Error!?Handle {
        const root = self.rootFor(target) orelse return null;
        const root_path = self.pathOf(root).?;
        const rest = try self.gpa.dupe(u8, target[root_path.len..]);
        defer self.gpa.free(rest);

        var current = root;
        var it = std.mem.tokenizeScalar(u8, rest, '/');
        while (it.next()) |component| {
            try self.expand(current);
            current = self.childNamed(current, component) orelse return null;
        }
        return current;
    }

    /// reads not yet loaded directories breadth first, so a search can find
    /// entries the user never opened. symlinks are not followed and hidden
    /// directories are skipped unless asked for. stops after `budget` new
    /// nodes and returns `false` in that case.
    pub fn loadDeep(self: *FileTree, budget: usize, include_hidden: bool) Allocator.Error!bool {
        const start = self.arena.len();
        var queue: std.ArrayList(Handle) = .empty;
        defer queue.deinit(self.gpa);
        try queue.appendSlice(self.gpa, self.roots.items);
        var head: usize = 0;
        while (head < queue.items.len) : (head += 1) {
            if (self.arena.len() - start >= budget) return false;
            const h = queue.items[head];
            const node = self.get(h) orelse continue;
            if (node.kind != .dir) continue;
            if (!include_hidden and node.isHidden() and node.parent != null) continue;
            if (node.children == null) try self.loadChildren(h);
            try queue.appendSlice(self.gpa, self.children(h));
        }
        return true;
    }

    /// paths of all expanded directories, parents before children. used to
    /// carry the expanded state over into a new tree (e.g. `cd ..`).
    /// the caller owns the list and the paths.
    pub fn expandedPaths(self: *const FileTree, gpa: Allocator) Allocator.Error![][]u8 {
        var out: std.ArrayList([]u8) = .empty;
        errdefer {
            for (out.items) |p| gpa.free(p);
            out.deinit(gpa);
        }
        var stack: std.ArrayList(Handle) = .empty;
        defer stack.deinit(gpa);
        var i = self.roots.items.len;
        while (i > 0) : (i -= 1) try stack.append(gpa, self.roots.items[i - 1]);
        while (stack.pop()) |h| {
            const node = self.get(h) orelse continue;
            if (!node.expanded) continue;
            try out.append(gpa, try gpa.dupe(u8, node.path));
            const ch = node.childList();
            var j = ch.len;
            while (j > 0) : (j -= 1) try stack.append(gpa, ch[j - 1]);
        }
        return out.toOwnedSlice(gpa);
    }
};

const testing = std.testing;
const TmpDir = @import("../testutil.zig").TmpDir;

fn fixture() !TmpDir {
    var dir = try TmpDir.init(testing.allocator);
    errdefer dir.deinit();
    try dir.mkdir("a/b");
    try dir.write("a/b/c.txt", "c");
    try dir.write("a/x.rs", "x");
    try dir.write("z.md", "z");
    return dir;
}

fn expectNames(tree: *const FileTree, handle: Handle, expected: []const []const u8) !void {
    const ch = tree.children(handle);
    try testing.expectEqual(expected.len, ch.len);
    for (ch, expected) |h, name| try testing.expectEqualStrings(name, tree.get(h).?.name);
}

test "loads lazily" {
    var dir = try fixture();
    defer dir.deinit();
    var tree = try FileTree.withRoot(testing.allocator, dir.root);
    defer tree.deinit();
    const root = tree.roots.items[0];
    try expectNames(&tree, root, &.{ "a", "z.md" });
    try testing.expect(tree.get(tree.children(root)[0]).?.children == null);
    try testing.expectEqual(3, tree.len());
}

test "find and reveal" {
    var dir = try fixture();
    defer dir.deinit();
    var tree = try FileTree.withRoot(testing.allocator, dir.root);
    defer tree.deinit();
    const target = try dir.path("a/b/c.txt");
    defer testing.allocator.free(target);

    try testing.expect(tree.find(target) == null);
    const h = (try tree.reveal(target)).?;
    try testing.expect(tree.find(target).?.eql(h));
    try testing.expectEqualStrings("c.txt", tree.get(h).?.name);
    try testing.expect(tree.get(tree.parent(h).?).?.expanded);
}

test "refresh keeps handles and state" {
    var dir = try fixture();
    defer dir.deinit();
    var tree = try FileTree.withRoot(testing.allocator, dir.root);
    defer tree.deinit();
    const root = tree.roots.items[0];
    const a_path = try dir.path("a");
    defer testing.allocator.free(a_path);
    const a = (try tree.reveal(a_path)).?;
    try tree.expand(a);

    try dir.write("new.txt", "");
    try dir.remove("z.md");
    try dir.write("a/y.rs", "");
    try tree.refresh(root);

    try expectNames(&tree, root, &.{ "a", "new.txt" });
    try testing.expect(tree.find(a_path).?.eql(a));
    try testing.expect(tree.get(a).?.expanded);
    try expectNames(&tree, a, &.{ "b", "x.rs", "y.rs" });
}

test "refresh frees removed subtrees" {
    var dir = try fixture();
    defer dir.deinit();
    var tree = try FileTree.withRoot(testing.allocator, dir.root);
    defer tree.deinit();
    const target = try dir.path("a/b/c.txt");
    defer testing.allocator.free(target);
    _ = try tree.reveal(target);
    try testing.expectEqual(6, tree.len());

    try dir.remove("a");
    try tree.refresh(tree.roots.items[0]);
    try testing.expectEqual(2, tree.len());
}

test "unreadable dir stores the error" {
    var dir = try fixture();
    defer dir.deinit();
    var tree = try FileTree.withRoot(testing.allocator, dir.root);
    defer tree.deinit();
    const a = tree.children(tree.roots.items[0])[0];
    try dir.remove("a");
    try tree.expand(a);
    try testing.expect(tree.get(a).?.err != null);
    try testing.expectEqual(0, tree.children(a).len);
}

test "multiple roots" {
    var one = try fixture();
    defer one.deinit();
    var two = try fixture();
    defer two.deinit();
    var tree = FileTree.init(testing.allocator);
    defer tree.deinit();
    _ = try tree.addRoot(one.root);
    const r2 = try tree.addRoot(two.root);
    const target = try two.path("a/x.rs");
    defer testing.allocator.free(target);

    const h = (try tree.reveal(target)).?;
    try testing.expect(tree.parent(tree.parent(h).?).?.eql(r2));
    tree.removeRoot(r2);
    try testing.expectEqual(1, tree.roots.items.len);
    try testing.expect(tree.find(target) == null);
}

test "expanded paths and deep loading" {
    var dir = try fixture();
    defer dir.deinit();
    var tree = try FileTree.withRoot(testing.allocator, dir.root);
    defer tree.deinit();
    try testing.expect(try tree.loadDeep(100, false));
    try testing.expectEqual(6, tree.len());
    try testing.expect(!try tree.loadDeep(0, false));

    const paths = try tree.expandedPaths(testing.allocator);
    defer {
        for (paths) |p| testing.allocator.free(p);
        testing.allocator.free(paths);
    }
    try testing.expectEqual(1, paths.len);
    try testing.expectEqualStrings(dir.root, paths[0]);
}
