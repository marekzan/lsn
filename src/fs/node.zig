const std = @import("std");
const Allocator = std.mem.Allocator;

const Handle = @import("../arena.zig").Handle;
const sys = @import("../sys.zig");

pub const Kind = union(enum) {
    dir,
    file,
    symlink: struct { target: []u8, to_dir: bool },

    pub fn sameAs(a: Kind, b: Kind) bool {
        return switch (a) {
            .dir => b == .dir,
            .file => b == .file,
            .symlink => |s| b == .symlink and s.to_dir == b.symlink.to_dir and
                std.mem.eql(u8, s.target, b.symlink.target),
        };
    }
};

/// the parts of the metadata we display, sort or filter by
pub const Meta = struct {
    size: u64 = 0,
    mode: u32 = 0,
    uid: u32 = 0,
    gid: u32 = 0,
    mtime: ?i64 = null,

    pub fn isExecutable(self: Meta) bool {
        return self.mode & 0o111 != 0;
    }
};

/// a single entry of the file system that we have seen so far.
///
/// nodes only hold data about the entry itself plus the links into the tree
/// (parent and children). ui state that is not tied to the file system
/// (selection, clipboard, filters) lives outside of the tree.
pub const Node = struct {
    /// a slice of `path`
    name: []const u8,
    path: []u8,
    kind: Kind,
    meta: Meta,
    parent: ?Handle,
    /// `null` until the directory was read for the first time (lazy loading)
    children: ?std.ArrayList(Handle) = null,
    expanded: bool = false,
    /// set when reading the directory failed (e.g. permission denied)
    err: ?anyerror = null,

    /// stats the given path (without following symlinks) and creates a
    /// detached node. takes ownership of `path`.
    pub fn fromOwnedPath(gpa: Allocator, path: []u8, parent: ?Handle) !Node {
        errdefer gpa.free(path);
        const st = try sys.lstat(path);
        const kind = try kindOf(gpa, path, st.mode);
        return .{
            .name = sys.baseName(path),
            .path = path,
            .kind = kind,
            .meta = metaOf(st),
            .parent = parent,
        };
    }

    pub fn fromPath(gpa: Allocator, path: []const u8, parent: ?Handle) !Node {
        return fromOwnedPath(gpa, try gpa.dupe(u8, path), parent);
    }

    pub fn deinit(self: *Node, gpa: Allocator) void {
        if (self.kind == .symlink) gpa.free(self.kind.symlink.target);
        if (self.children) |*ch| ch.deinit(gpa);
        gpa.free(self.path);
    }

    /// directories and symlinks pointing to directories can be expanded
    pub fn isDir(self: *const Node) bool {
        return switch (self.kind) {
            .dir => true,
            .symlink => |s| s.to_dir,
            .file => false,
        };
    }

    pub fn isHidden(self: *const Node) bool {
        return self.name.len > 0 and self.name[0] == '.';
    }

    pub fn extension(self: *const Node) []const u8 {
        if (self.isDir()) return "";
        const i = std.mem.findScalarLast(u8, self.name, '.') orelse return "";
        if (i == 0) return "";
        return self.name[i + 1 ..];
    }

    pub fn childList(self: *const Node) []const Handle {
        return if (self.children) |ch| ch.items else &.{};
    }
};

pub fn kindOf(gpa: Allocator, path: []const u8, mode: u32) Allocator.Error!Kind {
    if (sys.isLinkMode(mode)) {
        const target = sys.readLink(gpa, path) catch |err| switch (err) {
            error.OutOfMemory => return error.OutOfMemory,
            else => try gpa.dupe(u8, ""),
        };
        return .{ .symlink = .{ .target = target, .to_dir = sys.isDir(path) } };
    }
    return if (sys.isDirMode(mode)) .dir else .file;
}

pub fn metaOf(st: sys.Statx) Meta {
    return .{
        .size = st.size,
        .mode = st.mode,
        .uid = st.uid,
        .gid = st.gid,
        .mtime = st.mtime.sec,
    };
}
