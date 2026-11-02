const std = @import("std");
const Allocator = std.mem.Allocator;

const Handle = @import("arena.zig").Handle;
const Node = @import("fs/node.zig").Node;
const FileTree = @import("fs/tree.zig").FileTree;
const PathSet = @import("pathset.zig").PathSet;
const sys = @import("sys.zig");

/// one row of the view-model: a flat list of the nodes that are currently
/// visible.
///
/// it is rebuilt from the tree whenever the tree, the filter or the sort
/// order changes. everything the ui and the key handling needs (cursor,
/// scrolling, "node under cursor") works on this list instead of walking the
/// tree.
pub const ViewItem = struct {
    handle: Handle,
    depth: u16,
    /// bit `i` is set if the ancestor on level `i` still has siblings below
    /// it. used to draw the tree guides (`│`, `├─`, `└─`).
    guides: u64,
    is_last: bool,

    pub fn hasGuide(self: ViewItem, level: usize) bool {
        return level < 64 and self.guides & (@as(u64, 1) << @intCast(level)) != 0;
    }
};

pub const Filter = struct {
    show_hidden: bool = false,
    show_dirs: bool = true,
    show_files: bool = true,
    show_symlinks: bool = true,
    /// only show marked entries (and the content of marked directories)
    only_marked: bool = false,
    /// substring search on the name, smartcase
    query_buf: [256]u8 = undefined,
    query_len: usize = 0,

    pub fn query(self: *const Filter) []const u8 {
        return self.query_buf[0..self.query_len];
    }

    pub fn setQuery(self: *Filter, text: []const u8) void {
        const n = @min(text.len, self.query_buf.len);
        @memcpy(self.query_buf[0..n], text[0..n]);
        self.query_len = n;
    }

    /// filters that also look into collapsed directories, so a match deep in
    /// the tree becomes visible together with its parents
    fn reveals(self: *const Filter) bool {
        return self.only_marked or self.query_len > 0;
    }

    fn matchesKind(self: *const Filter, node: *const Node) bool {
        return switch (node.kind) {
            .dir => self.show_dirs,
            .file => self.show_files,
            .symlink => self.show_symlinks,
        };
    }

    /// short description of the active filters for the status bar
    pub fn describe(self: *const Filter, buf: []u8) ?[]const u8 {
        var w: std.Io.Writer = .fixed(buf);
        var any = false;
        if (self.show_hidden) {
            w.writeAll("hidden ") catch {};
            any = true;
        }
        if (!(self.show_dirs and self.show_files and self.show_symlinks)) {
            w.writeAll("only ") catch {};
            if (self.show_dirs) w.writeByte('d') catch {};
            if (self.show_files) w.writeByte('f') catch {};
            if (self.show_symlinks) w.writeByte('l') catch {};
            w.writeByte(' ') catch {};
            any = true;
        }
        if (self.only_marked) {
            w.writeAll("marked ") catch {};
            any = true;
        }
        if (self.query_len > 0) {
            w.print("/{s} ", .{self.query()}) catch {};
            any = true;
        }
        if (!any) return null;
        return std.mem.trimEnd(u8, w.buffered(), " ");
    }
};

pub const SortBy = enum { name, size, modified, extension };

pub const Sort = struct {
    by: SortBy = .name,
    dirs_first: bool = true,
    reverse: bool = false,

    pub fn compare(self: Sort, a: *const Node, b: *const Node) std.math.Order {
        if (self.dirs_first) {
            if (a.isDir() and !b.isDir()) return .lt;
            if (!a.isDir() and b.isDir()) return .gt;
        }
        const by_name = naturalCmp(a.name, b.name);
        const ord: std.math.Order = switch (self.by) {
            .name => by_name,
            // biggest / newest first feels more natural for these
            .size => orElse(std.math.order(size(b), size(a)), by_name),
            .modified => orElse(std.math.order(b.meta.mtime orelse 0, a.meta.mtime orelse 0), by_name),
            .extension => orElse(asciiCmp(a.extension(), b.extension()), by_name),
        };
        return if (self.reverse) ord.invert() else ord;
    }

    pub fn lessThan(self: Sort, a: *const Node, b: *const Node) bool {
        return self.compare(a, b) == .lt;
    }

    pub fn describe(self: Sort) []const u8 {
        return switch (self.by) {
            .name => if (self.reverse) "name ↓" else "name",
            .size => if (self.reverse) "size ↓" else "size",
            .modified => if (self.reverse) "modified ↓" else "modified",
            .extension => if (self.reverse) "ext ↓" else "ext",
        };
    }
};

/// the size of a directory entry itself means nothing to the user
fn size(n: *const Node) u64 {
    return if (n.isDir()) 0 else n.meta.size;
}

fn orElse(a: std.math.Order, b: std.math.Order) std.math.Order {
    return if (a != .eq) a else b;
}

fn asciiCmp(a: []const u8, b: []const u8) std.math.Order {
    for (a[0..@min(a.len, b.len)], b[0..@min(a.len, b.len)]) |x, y| {
        const o = std.math.order(std.ascii.toLower(x), std.ascii.toLower(y));
        if (o != .eq) return o;
    }
    return std.math.order(a.len, b.len);
}

/// case insensitive compare that orders numbers by value (`file2` < `file10`)
pub fn naturalCmp(a: []const u8, b: []const u8) std.math.Order {
    var i: usize = 0;
    var j: usize = 0;
    while (i < a.len and j < b.len) {
        if (std.ascii.isDigit(a[i]) and std.ascii.isDigit(b[j])) {
            const si = i;
            const sj = j;
            while (i < a.len and std.ascii.isDigit(a[i])) i += 1;
            while (j < b.len and std.ascii.isDigit(b[j])) j += 1;
            const na = std.mem.trimStart(u8, a[si..i], "0");
            const nb = std.mem.trimStart(u8, b[sj..j], "0");
            const o = orElse(std.math.order(na.len, nb.len), std.mem.order(u8, na, nb));
            if (o != .eq) return o;
        } else {
            const o = std.math.order(std.ascii.toLower(a[i]), std.ascii.toLower(b[j]));
            if (o != .eq) return o;
            i += 1;
            j += 1;
        }
    }
    return std.math.order(a.len - i, b.len - j);
}

/// byte range of the first occurrence of `query` in `name`. smartcase: the
/// search is case sensitive as soon as the query has an uppercase letter.
/// case folding only covers ascii.
pub fn matchRange(query: []const u8, name: []const u8) ?[2]usize {
    const sensitive = for (query) |ch| {
        if (std.ascii.isUpper(ch)) break true;
    } else false;
    const i = if (sensitive)
        std.mem.find(u8, name, query)
    else
        std.ascii.findIgnoreCase(name, query);
    return if (i) |start| .{ start, start + query.len } else null;
}

/// everything outside of the tree that decides what is visible
pub const Context = struct {
    filter: *const Filter,
    sort: Sort,
    marked: *const PathSet,
};

/// flattens the visible part of the tree into `out`.
///
/// `include_roots`: a normal tab shows the content of its root, the pins tab
/// shows the pinned folders themselves.
pub fn build(gpa: Allocator, tree: *const FileTree, ctx: Context, include_roots: bool, out: *std.ArrayList(ViewItem)) Allocator.Error!void {
    out.clearRetainingCapacity();
    if (include_roots) {
        _ = try walk(gpa, tree, ctx, tree.roots.items, false, 0, 0, out);
        return;
    }
    for (tree.roots.items) |root| {
        const marked = isMarkedOrInside(ctx.marked, tree.pathOf(root).?);
        _ = try walk(gpa, tree, ctx, tree.children(root), marked, 0, 0, out);
    }
}

fn isMarkedOrInside(marked: *const PathSet, path: []const u8) bool {
    var p: ?[]const u8 = path;
    while (p) |current| : (p = sys.dirName(current)) {
        if (marked.contains(current)) return true;
    }
    return false;
}

const SortCtx = struct {
    tree: *const FileTree,
    sort: Sort,
    fn lessThan(self: SortCtx, a: Handle, b: Handle) bool {
        return self.sort.lessThan(self.tree.get(a).?, self.tree.get(b).?);
    }
};

/// emits the visible nodes of one level (and below) and returns whether
/// anything was emitted
fn walk(
    gpa: Allocator,
    tree: *const FileTree,
    ctx: Context,
    siblings: []const Handle,
    parent_marked: bool,
    depth: u16,
    guides: u64,
    out: *std.ArrayList(ViewItem),
) Allocator.Error!bool {
    var sorted: std.ArrayList(Handle) = .empty;
    defer sorted.deinit(gpa);
    for (siblings) |h| {
        const node = tree.get(h) orelse continue;
        if (ctx.filter.show_hidden or !node.isHidden()) try sorted.append(gpa, h);
    }
    std.mem.sort(Handle, sorted.items, SortCtx{ .tree = tree, .sort = ctx.sort }, SortCtx.lessThan);

    // where the block of every emitted sibling starts. we only know which one
    // is the last visible sibling after all of them were filtered.
    var starts: std.ArrayList(usize) = .empty;
    defer starts.deinit(gpa);

    // the bit for this level is set for the children for now and cleared
    // below for the children of the last sibling
    const child_guides = if (depth < 64) guides | (@as(u64, 1) << @intCast(depth)) else guides;

    for (sorted.items) |h| {
        const node = tree.get(h).?;
        const marked = parent_marked or ctx.marked.contains(node.path);
        const start = out.items.len;
        try out.append(gpa, .{ .handle = h, .depth = depth, .guides = guides, .is_last = false });

        const descend = node.isDir() and (node.expanded or ctx.filter.reveals());
        const has_children = descend and
            try walk(gpa, tree, ctx, tree.children(h), marked, depth + 1, child_guides, out);

        const matches_self = ctx.filter.matchesKind(node) and
            (ctx.filter.query_len == 0 or matchRange(ctx.filter.query(), node.name) != null) and
            (!ctx.filter.only_marked or marked);

        // directories without a match themselves stay as context for their
        // matching descendants
        if (matches_self or has_children) {
            try starts.append(gpa, start);
        } else {
            out.shrinkRetainingCapacity(start);
        }
    }

    for (starts.items, 0..) |start, i| {
        const last = i + 1 == starts.items.len;
        out.items[start].is_last = last;
        if (last and depth < 64) {
            // children of a last sibling don't need a `│` at this level
            const end = out.items.len;
            for (out.items[start + 1 .. end]) |*item| item.guides &= ~(@as(u64, 1) << @intCast(depth));
        }
    }
    return starts.items.len > 0;
}

const testing = std.testing;
const TmpDir = @import("testutil.zig").TmpDir;

const Fixture = struct {
    dir: TmpDir,
    tree: FileTree,

    fn init() !Fixture {
        var dir = try TmpDir.init(testing.allocator);
        errdefer dir.deinit();
        try dir.mkdir("src/deep");
        try dir.write("src/deep/needle.rs", "");
        try dir.write("src/main.rs", "");
        try dir.write("file10.txt", "1234567890");
        try dir.write("file2.txt", "12");
        try dir.write(".hidden", "");
        return .{ .dir = dir, .tree = try FileTree.withRoot(testing.allocator, dir.root) };
    }

    fn deinit(self: *Fixture) void {
        self.tree.deinit();
        self.dir.deinit();
    }

    fn reveal(self: *Fixture, rel: []const u8) !Handle {
        const p = try self.dir.path(rel);
        defer testing.allocator.free(p);
        return (try self.tree.reveal(p)).?;
    }

    fn expectView(self: *Fixture, filter: Filter, sort: Sort, marked: *const PathSet, expected: []const []const u8) !void {
        var items: std.ArrayList(ViewItem) = .empty;
        defer items.deinit(testing.allocator);
        try build(testing.allocator, &self.tree, .{ .filter = &filter, .sort = sort, .marked = marked }, false, &items);

        var lines: std.ArrayList(u8) = .empty;
        defer lines.deinit(testing.allocator);
        for (items.items) |item| {
            for (0..item.depth) |level| try lines.appendSlice(testing.allocator, if (item.hasGuide(level)) "│ " else "  ");
            try lines.appendSlice(testing.allocator, if (item.is_last) "└ " else "├ ");
            try lines.appendSlice(testing.allocator, self.tree.get(item.handle).?.name);
            try lines.append(testing.allocator, '\n');
        }
        var want: std.ArrayList(u8) = .empty;
        defer want.deinit(testing.allocator);
        for (expected) |line| {
            try want.appendSlice(testing.allocator, line);
            try want.append(testing.allocator, '\n');
        }
        try testing.expectEqualStrings(want.items, lines.items);
    }
};

test "default view is sorted and hides dotfiles" {
    var f = try Fixture.init();
    defer f.deinit();
    const none: PathSet = .{};
    try f.expectView(.{}, .{}, &none, &.{ "├ src", "├ file2.txt", "└ file10.txt" });
}

test "expanded dirs show children with guides" {
    var f = try Fixture.init();
    defer f.deinit();
    _ = try f.reveal("src/deep/needle.rs");
    const none: PathSet = .{};
    try f.expectView(.{}, .{}, &none, &.{
        "├ src",
        "│ ├ deep",
        "│ │ └ needle.rs",
        "│ └ main.rs",
        "├ file2.txt",
        "└ file10.txt",
    });
}

test "query reveals loaded matches in collapsed dirs" {
    var f = try Fixture.init();
    defer f.deinit();
    const needle = try f.reveal("src/deep/needle.rs");
    const src = f.tree.parent(f.tree.parent(needle).?).?;
    try f.tree.collapseAll(src);
    var filter: Filter = .{};
    filter.setQuery("needle");
    const none: PathSet = .{};
    try f.expectView(filter, .{}, &none, &.{ "└ src", "  └ deep", "    └ needle.rs" });

    // smartcase
    filter.setQuery("File");
    try f.expectView(filter, .{}, &none, &.{});
}

test "kind filter keeps parents as context" {
    var f = try Fixture.init();
    defer f.deinit();
    _ = try f.reveal("src/main.rs");
    const none: PathSet = .{};
    try f.expectView(.{ .show_files = false }, .{}, &none, &.{ "└ src", "  └ deep" });
}

test "only marked" {
    var f = try Fixture.init();
    defer f.deinit();
    _ = try f.reveal("src/main.rs");
    var marked: PathSet = .{};
    defer marked.deinit(testing.allocator);
    for ([_][]const u8{ "src/deep", "file2.txt" }) |rel| {
        const p = try f.dir.path(rel);
        defer testing.allocator.free(p);
        try marked.insert(testing.allocator, p);
    }
    try f.expectView(.{ .only_marked = true }, .{}, &marked, &.{ "├ src", "│ └ deep", "└ file2.txt" });
}

test "sort by size and reverse" {
    var f = try Fixture.init();
    defer f.deinit();
    const none: PathSet = .{};
    try f.expectView(.{}, .{ .by = .size, .dirs_first = false }, &none, &.{ "├ file10.txt", "├ file2.txt", "└ src" });
    try f.expectView(.{}, .{ .dirs_first = false, .reverse = true }, &none, &.{ "├ src", "├ file10.txt", "└ file2.txt" });
}

test "natural order" {
    try testing.expectEqual(.lt, naturalCmp("a2", "a10"));
    try testing.expectEqual(.lt, naturalCmp("A", "b"));
    try testing.expectEqual(.eq, naturalCmp("x01", "x1"));
}
