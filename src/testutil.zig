//! helpers for tests that need a real directory on disk

const std = @import("std");
const Allocator = std.mem.Allocator;

const sys = @import("sys.zig");
const ops = @import("fs/ops.zig");

pub const TmpDir = struct {
    gpa: Allocator,
    /// canonical path, so it compares equal to what the tree stores
    root: []u8,

    pub fn init(gpa: Allocator) !TmpDir {
        const raw = try sys.makeTempDir(gpa);
        defer gpa.free(raw);
        return .{ .gpa = gpa, .root = try sys.realPath(gpa, raw) };
    }

    pub fn deinit(self: *TmpDir) void {
        ops.remove(self.gpa, self.root) catch {};
        self.gpa.free(self.root);
    }

    /// absolute path of `rel`, the caller owns it
    pub fn path(self: *const TmpDir, rel: []const u8) ![]u8 {
        if (rel.len == 0) return self.gpa.dupe(u8, self.root);
        return sys.joinPath(self.gpa, self.root, rel);
    }

    pub fn mkdir(self: *const TmpDir, rel: []const u8) !void {
        const p = try self.path(rel);
        defer self.gpa.free(p);
        // create the parents as well
        var i: usize = self.root.len + 1;
        while (std.mem.findScalarPos(u8, p, i, '/')) |slash| : (i = slash + 1) {
            sys.mkdir(p[0..slash], 0o755) catch |err| if (err != error.AlreadyExists) return err;
        }
        sys.mkdir(p, 0o755) catch |err| if (err != error.AlreadyExists) return err;
    }

    pub fn write(self: *const TmpDir, rel: []const u8, content: []const u8) !void {
        const p = try self.path(rel);
        defer self.gpa.free(p);
        const fd = try sys.open(p, .write_truncate, 0o644);
        defer sys.close(fd);
        try sys.writeAll(fd, content);
    }

    pub fn remove(self: *const TmpDir, rel: []const u8) !void {
        const p = try self.path(rel);
        defer self.gpa.free(p);
        try ops.remove(self.gpa, p);
    }

    pub fn exists(self: *const TmpDir, rel: []const u8) bool {
        const p = self.path(rel) catch return false;
        defer self.gpa.free(p);
        return sys.exists(p);
    }

    pub fn read(self: *const TmpDir, rel: []const u8, buf: []u8) ![]u8 {
        const p = try self.path(rel);
        defer self.gpa.free(p);
        const fd = try sys.open(p, .read, 0);
        defer sys.close(fd);
        return buf[0..try sys.readAll(fd, buf)];
    }
};
