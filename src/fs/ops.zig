//! file system operations. everything here works on paths only and knows
//! nothing about trees or tabs, the app refreshes the affected directories
//! afterwards.

const std = @import("std");
const Allocator = std.mem.Allocator;

const sys = @import("../sys.zig");

/// what happens with the clipboard content when it is pasted
pub const TransferKind = enum { copy, move };

pub const Error = sys.Error || Allocator.Error || error{ InvalidName, InvalidMode, IntoItself };

/// copies or moves `src` into the directory `dest_dir` and returns the new
/// path (owned by the caller). if the name is already taken a free one
/// (`name_1.ext`, ...) is used.
pub fn transfer(gpa: Allocator, src: []const u8, dest_dir: []const u8, kind: TransferKind) Error![]u8 {
    const name = sys.baseName(src);
    if (sys.isDir(src) and sys.isWithin(dest_dir, src)) return error.IntoItself;

    const wanted = try sys.joinPath(gpa, dest_dir, name);
    // moving to the same place is a no-op
    if (kind == .move and std.mem.eql(u8, wanted, src)) return wanted;
    defer gpa.free(wanted);
    const dest = try freeName(gpa, wanted);
    errdefer gpa.free(dest);

    switch (kind) {
        .copy => try copyRecursive(gpa, src, dest),
        .move => sys.rename(src, dest) catch |err| {
            // rename does not work across file systems
            if (err != error.CrossDevice) return err;
            try copyRecursive(gpa, src, dest);
            try remove(gpa, src);
        },
    }
    return dest;
}

/// `path` if it does not exist yet, otherwise `stem_N.ext` with the smallest
/// free N. the caller owns the result.
pub fn freeName(gpa: Allocator, path: []const u8) Allocator.Error![]u8 {
    if (!sys.exists(path)) return gpa.dupe(u8, path);
    const parent = sys.dirName(path) orelse "";
    const file_name = sys.baseName(path);
    // dotfiles like `.bashrc` have no extension
    const dot = std.mem.findScalarLast(u8, file_name, '.');
    const stem, const ext = if (dot) |i| (if (i > 0) .{ file_name[0..i], file_name[i..] } else .{ file_name, "" }) else .{ file_name, "" };

    var n: usize = 1;
    while (true) : (n += 1) {
        const candidate = try std.fmt.allocPrint(gpa, "{s}/{s}_{d}{s}", .{ parent, stem, n, ext });
        if (!sys.exists(candidate)) return candidate;
        gpa.free(candidate);
    }
}

fn copyRecursive(gpa: Allocator, src: []const u8, dest: []const u8) Error!void {
    const st = try sys.lstat(src);
    if (sys.isLinkMode(st.mode)) {
        const target = try sys.readLink(gpa, src);
        defer gpa.free(target);
        try sys.symlink(target, dest);
    } else if (sys.isDirMode(st.mode)) {
        try sys.mkdir(dest, 0o700);
        const names = try sys.listDir(gpa, src);
        defer sys.freeNames(gpa, names);
        for (names) |name| {
            const from = try sys.joinPath(gpa, src, name);
            defer gpa.free(from);
            const to = try sys.joinPath(gpa, dest, name);
            defer gpa.free(to);
            try copyRecursive(gpa, from, to);
        }
        try sys.chmod(dest, st.mode & 0o7777);
    } else {
        try copyFile(src, dest, st.mode & 0o7777);
    }
}

fn copyFile(src: []const u8, dest: []const u8, mode: u32) Error!void {
    const in = try sys.open(src, .read, 0);
    defer sys.close(in);
    const out = try sys.open(dest, .write_new, mode);
    defer sys.close(out);
    var buf: [64 * 1024]u8 = undefined;
    while (true) {
        const n = try sys.readAll(in, &buf);
        if (n == 0) break;
        try sys.writeAll(out, buf[0..n]);
        if (n < buf.len) break;
    }
}

/// deletes a file, symlink or a whole directory
pub fn remove(gpa: Allocator, path: []const u8) Error!void {
    const st = try sys.lstat(path);
    if (!sys.isDirMode(st.mode)) return sys.unlink(path);
    const names = try sys.listDir(gpa, path);
    defer sys.freeNames(gpa, names);
    for (names) |name| {
        const child = try sys.joinPath(gpa, path, name);
        defer gpa.free(child);
        try remove(gpa, child);
    }
    try sys.rmdir(path);
}

/// renames in place, returns the new path (owned by the caller)
pub fn rename(gpa: Allocator, path: []const u8, new_name: []const u8) Error![]u8 {
    try validateName(new_name);
    const parent = sys.dirName(path) orelse return error.InvalidName;
    const dest = try sys.joinPath(gpa, parent, new_name);
    errdefer gpa.free(dest);
    if (!std.mem.eql(u8, dest, path) and sys.exists(dest)) return error.AlreadyExists;
    try sys.rename(path, dest);
    return dest;
}

/// creates a file, or a directory when `name` ends with `/`. intermediate
/// directories (`a/b/c.txt`) are created as well. returns the new path.
pub fn create(gpa: Allocator, dir: []const u8, name: []const u8) Error![]u8 {
    const is_dir = std.mem.endsWith(u8, name, "/");
    const trimmed = std.mem.trimEnd(u8, name, "/");
    if (trimmed.len == 0 or trimmed[0] == '/') return error.InvalidName;
    var parts = std.mem.splitScalar(u8, trimmed, '/');
    while (parts.next()) |part| {
        if (part.len == 0 or std.mem.eql(u8, part, ".") or std.mem.eql(u8, part, "..")) return error.InvalidName;
    }

    const path = try sys.joinPath(gpa, dir, trimmed);
    errdefer gpa.free(path);
    if (sys.exists(path)) return error.AlreadyExists;

    // parents first
    var i: usize = dir.len + 1;
    while (std.mem.findScalarPos(u8, path, i, '/')) |slash| : (i = slash + 1) {
        sys.mkdir(path[0..slash], 0o755) catch |err| if (err != error.AlreadyExists) return err;
    }
    if (is_dir) {
        try sys.mkdir(path, 0o755);
    } else {
        sys.close(try sys.open(path, .write_new, 0o644));
    }
    return path;
}

/// sets the permission bits, `mode` is octal like `755` or `0644`
pub fn chmod(path: []const u8, mode: []const u8) Error!void {
    try sys.chmod(path, parseMode(mode) orelse return error.InvalidMode);
}

pub fn parseMode(mode: []const u8) ?u32 {
    const m = std.mem.trim(u8, mode, " ");
    if (m.len == 0 or m.len > 4) return null;
    const value = std.fmt.parseInt(u32, m, 8) catch return null;
    return if (value <= 0o7777) value else null;
}

fn validateName(name: []const u8) Error!void {
    if (name.len == 0 or std.mem.eql(u8, name, ".") or std.mem.eql(u8, name, "..") or
        std.mem.findScalar(u8, name, '/') != null) return error.InvalidName;
}

const testing = std.testing;
const TmpDir = @import("../testutil.zig").TmpDir;

test "copy a dir recursively with a free name" {
    var dir = try TmpDir.init(testing.allocator);
    defer dir.deinit();
    try dir.mkdir("src/inner");
    try dir.write("src/inner/f.txt", "hi");
    const src = try dir.path("src");
    defer testing.allocator.free(src);

    const dest = try transfer(testing.allocator, src, dir.root, .copy);
    defer testing.allocator.free(dest);
    try testing.expectEqualStrings("src_1", sys.baseName(dest));
    var buf: [16]u8 = undefined;
    try testing.expectEqualStrings("hi", try dir.read("src_1/inner/f.txt", &buf));
    try testing.expect(dir.exists("src"));
}

test "move a file" {
    var dir = try TmpDir.init(testing.allocator);
    defer dir.deinit();
    try dir.mkdir("target");
    try dir.write("a.txt", "a");
    const file = try dir.path("a.txt");
    defer testing.allocator.free(file);
    const target = try dir.path("target");
    defer testing.allocator.free(target);

    const dest = try transfer(testing.allocator, file, target, .move);
    defer testing.allocator.free(dest);
    try testing.expect(dir.exists("target/a.txt"));
    try testing.expect(!dir.exists("a.txt"));
}

test "a dir cannot go into itself, moving in place is a no-op" {
    var dir = try TmpDir.init(testing.allocator);
    defer dir.deinit();
    try dir.mkdir("src/inner");
    try dir.write("a.txt", "a");
    const src = try dir.path("src");
    defer testing.allocator.free(src);
    const inner = try dir.path("src/inner");
    defer testing.allocator.free(inner);
    try testing.expectError(error.IntoItself, transfer(testing.allocator, src, inner, .move));
    try testing.expectError(error.IntoItself, transfer(testing.allocator, src, src, .copy));

    const file = try dir.path("a.txt");
    defer testing.allocator.free(file);
    const same = try transfer(testing.allocator, file, dir.root, .move);
    defer testing.allocator.free(same);
    try testing.expectEqualStrings(file, same);
}

test "free names" {
    var dir = try TmpDir.init(testing.allocator);
    defer dir.deinit();
    for ([_][]const u8{ "a.tar.gz", ".bashrc", "a.tar_1.gz" }) |n| try dir.write(n, "");
    inline for (.{ .{ "a.tar.gz", "a.tar_2.gz" }, .{ ".bashrc", ".bashrc_1" }, .{ "new", "new" } }) |case| {
        const p = try dir.path(case[0]);
        defer testing.allocator.free(p);
        const free = try freeName(testing.allocator, p);
        defer testing.allocator.free(free);
        try testing.expectEqualStrings(case[1], sys.baseName(free));
    }
}

test "create, rename, chmod, remove" {
    const gpa = testing.allocator;
    var dir = try TmpDir.init(gpa);
    defer dir.deinit();
    const d = try create(gpa, dir.root, "x/y/");
    defer gpa.free(d);
    try testing.expect(sys.isDir(d));
    const f = try create(gpa, dir.root, "x/z.txt");
    defer gpa.free(f);
    try testing.expectError(error.AlreadyExists, create(gpa, dir.root, "x/z.txt"));
    try testing.expectError(error.InvalidName, create(gpa, dir.root, "../evil"));

    const renamed = try rename(gpa, f, "b");
    defer gpa.free(renamed);
    try testing.expect(dir.exists("x/b") and !dir.exists("x/z.txt"));
    try testing.expectError(error.InvalidName, rename(gpa, renamed, "x/y"));

    try chmod(renamed, "640");
    try testing.expectEqual(0o640, (try sys.lstat(renamed)).mode & 0o7777);
    try testing.expectError(error.InvalidMode, chmod(renamed, "999"));

    const x = try dir.path("x");
    defer gpa.free(x);
    try remove(gpa, x);
    try testing.expect(!dir.exists("x"));
}
