//! thin wrappers around libc.
//!
//! lsn talks to the os through libc instead of `std.Io`: the libc api is
//! stable, `std.Io` still changes a lot between zig versions.

const std = @import("std");
const c = std.c;
const linux = std.os.linux;
const Allocator = std.mem.Allocator;

pub const Statx = linux.Statx;

extern "c" fn statx(dirfd: c_int, path: [*:0]const u8, flags: c_int, mask: u32, buf: *Statx) c_int;
extern "c" fn localtime_r(t: *const i64, tm: *Tm) ?*Tm;
extern "c" fn strftime(s: [*]u8, max: usize, fmt: [*:0]const u8, tm: *const Tm) usize;
extern "c" fn mkdtemp(template: [*:0]u8) ?[*:0]u8;

const Tm = extern struct {
    sec: c_int,
    min: c_int,
    hour: c_int,
    mday: c_int,
    mon: c_int,
    year: c_int,
    wday: c_int,
    yday: c_int,
    isdst: c_int,
    gmtoff: c_long,
    zone: ?[*:0]const u8,
};

pub const Error = error{
    FileNotFound,
    AccessDenied,
    AlreadyExists,
    NotEmpty,
    CrossDevice,
    NotDir,
    IsDir,
    InvalidArgument,
    NameTooLong,
    NoSpace,
    ReadOnly,
    Unexpected,
};

/// the error of the last failed libc call
pub fn lastError() Error {
    return switch (std.posix.errno(@as(c_int, -1))) {
        .NOENT => error.FileNotFound,
        .ACCES, .PERM => error.AccessDenied,
        .EXIST => error.AlreadyExists,
        .NOTEMPTY => error.NotEmpty,
        .XDEV => error.CrossDevice,
        .NOTDIR => error.NotDir,
        .ISDIR => error.IsDir,
        .INVAL => error.InvalidArgument,
        .NAMETOOLONG => error.NameTooLong,
        .NOSPC => error.NoSpace,
        .ROFS => error.ReadOnly,
        else => error.Unexpected,
    };
}

/// human readable text for the errors we produce
pub fn describe(err: anyerror) []const u8 {
    return switch (err) {
        error.FileNotFound => "no such file or directory",
        error.AccessDenied => "permission denied",
        error.AlreadyExists => "already exists",
        error.NotEmpty => "directory not empty",
        error.CrossDevice => "cross-device link",
        error.NotDir => "not a directory",
        error.IsDir => "is a directory",
        error.InvalidArgument => "invalid argument",
        error.NameTooLong => "name too long",
        error.NoSpace => "no space left on device",
        error.ReadOnly => "read-only file system",
        error.InvalidName => "invalid name",
        error.InvalidMode => "mode must be octal, e.g. 644",
        error.IntoItself => "cannot put a directory into itself",
        error.OutOfMemory => "out of memory",
        else => @errorName(err),
    };
}

/// a path as a null terminated string for libc, on the stack
pub const PathZ = struct {
    buf: [4096:0]u8 = undefined,

    pub fn init(path: []const u8) Error!PathZ {
        var p: PathZ = .{};
        if (path.len >= p.buf.len) return error.NameTooLong;
        @memcpy(p.buf[0..path.len], path);
        p.buf[path.len] = 0;
        return p;
    }

    pub fn ptr(self: *const PathZ) [*:0]const u8 {
        return @ptrCast(&self.buf);
    }
};

fn check(rc: c_int) Error!void {
    if (rc != 0) return lastError();
}

const STATX_BASIC_STATS: u32 = 0x7ff;

/// stat without following symlinks
pub fn lstat(path: []const u8) Error!Statx {
    const p = try PathZ.init(path);
    var st: Statx = undefined;
    try check(statx(linux.AT.FDCWD, p.ptr(), linux.AT.SYMLINK_NOFOLLOW, STATX_BASIC_STATS, &st));
    return st;
}

/// stat following symlinks
pub fn stat(path: []const u8) Error!Statx {
    const p = try PathZ.init(path);
    var st: Statx = undefined;
    try check(statx(linux.AT.FDCWD, p.ptr(), 0, STATX_BASIC_STATS, &st));
    return st;
}

pub fn exists(path: []const u8) bool {
    _ = lstat(path) catch return false;
    return true;
}

pub const S_IFMT: u32 = 0o170000;
pub const S_IFDIR: u32 = 0o040000;
pub const S_IFLNK: u32 = 0o120000;

pub fn isDirMode(mode: u32) bool {
    return mode & S_IFMT == S_IFDIR;
}

pub fn isLinkMode(mode: u32) bool {
    return mode & S_IFMT == S_IFLNK;
}

pub fn isDir(path: []const u8) bool {
    const st = stat(path) catch return false;
    return isDirMode(st.mode);
}

pub fn readLink(gpa: Allocator, path: []const u8) (Error || Allocator.Error)![]u8 {
    const p = try PathZ.init(path);
    var buf: [4096]u8 = undefined;
    const n = c.readlink(p.ptr(), &buf, buf.len);
    if (n < 0) return lastError();
    return gpa.dupe(u8, buf[0..@intCast(n)]);
}

pub fn realPath(gpa: Allocator, path: []const u8) (Error || Allocator.Error)![]u8 {
    const p = try PathZ.init(path);
    var buf: [4096]u8 = undefined;
    const res = c.realpath(p.ptr(), &buf) orelse return lastError();
    return gpa.dupe(u8, std.mem.sliceTo(res, 0));
}

pub fn getenv(name: [*:0]const u8) ?[]const u8 {
    const v = c.getenv(name) orelse return null;
    return std.mem.sliceTo(v, 0);
}

/// names of the entries of a directory, without `.` and `..`, sorted.
/// the caller owns the list and every name in it.
pub fn listDir(gpa: Allocator, path: []const u8) (Error || Allocator.Error)![][]u8 {
    const p = try PathZ.init(path);
    const dir = c.opendir(p.ptr()) orelse return lastError();
    defer _ = c.closedir(dir);

    var names: std.ArrayList([]u8) = .empty;
    errdefer {
        for (names.items) |n| gpa.free(n);
        names.deinit(gpa);
    }
    while (c.readdir(dir)) |entry| {
        const name = std.mem.sliceTo(&entry.name, 0);
        if (std.mem.eql(u8, name, ".") or std.mem.eql(u8, name, "..")) continue;
        try names.append(gpa, try gpa.dupe(u8, name));
    }
    // a stable order keeps the view stable when sorting has a lot of ties
    std.mem.sort([]u8, names.items, {}, struct {
        fn lt(_: void, a: []u8, b: []u8) bool {
            return std.mem.lessThan(u8, a, b);
        }
    }.lt);
    return names.toOwnedSlice(gpa);
}

pub fn freeNames(gpa: Allocator, names: [][]u8) void {
    for (names) |n| gpa.free(n);
    gpa.free(names);
}

pub fn rename(from: []const u8, to: []const u8) Error!void {
    const a = try PathZ.init(from);
    const b = try PathZ.init(to);
    try check(c.rename(a.ptr(), b.ptr()));
}

pub fn mkdir(path: []const u8, mode: u32) Error!void {
    const p = try PathZ.init(path);
    try check(c.mkdir(p.ptr(), @intCast(mode)));
}

pub fn rmdir(path: []const u8) Error!void {
    const p = try PathZ.init(path);
    try check(c.rmdir(p.ptr()));
}

pub fn unlink(path: []const u8) Error!void {
    const p = try PathZ.init(path);
    try check(c.unlink(p.ptr()));
}

pub fn chmod(path: []const u8, mode: u32) Error!void {
    const p = try PathZ.init(path);
    try check(c.chmod(p.ptr(), @intCast(mode)));
}

pub fn symlink(target: []const u8, path: []const u8) Error!void {
    const t = try PathZ.init(target);
    const p = try PathZ.init(path);
    try check(c.symlink(t.ptr(), p.ptr()));
}

pub const OpenMode = enum { read, write_new, write_truncate };

pub fn open(path: []const u8, mode: OpenMode, perm: u32) Error!c_int {
    const p = try PathZ.init(path);
    const flags: c.O = switch (mode) {
        .read => .{ .ACCMODE = .RDONLY, .CLOEXEC = true },
        .write_new => .{ .ACCMODE = .WRONLY, .CREAT = true, .EXCL = true, .CLOEXEC = true },
        .write_truncate => .{ .ACCMODE = .WRONLY, .CREAT = true, .TRUNC = true, .CLOEXEC = true },
    };
    const fd = c.open(p.ptr(), flags, @as(c_uint, perm));
    if (fd < 0) return lastError();
    return fd;
}

pub fn close(fd: c_int) void {
    _ = c.close(fd);
}

pub fn readAll(fd: c_int, buf: []u8) Error!usize {
    var total: usize = 0;
    while (total < buf.len) {
        const n = c.read(fd, buf[total..].ptr, buf.len - total);
        if (n < 0) return lastError();
        if (n == 0) break;
        total += @intCast(n);
    }
    return total;
}

pub fn writeAll(fd: c_int, bytes: []const u8) Error!void {
    var rest = bytes;
    while (rest.len > 0) {
        const n = c.write(fd, rest.ptr, rest.len);
        if (n < 0) {
            if (std.posix.errno(@as(c_int, -1)) == .INTR) continue;
            return lastError();
        }
        rest = rest[@intCast(n)..];
    }
}

/// `YYYY-MM-DD HH:MM` in local time
pub fn formatTime(sec: i64, buf: *[32]u8) []const u8 {
    var tm: Tm = undefined;
    if (localtime_r(&sec, &tm) == null) return "";
    const n = strftime(buf, buf.len, "%Y-%m-%d %H:%M", &tm);
    return buf[0..n];
}

pub fn userName(uid: u32, buf: *[4096]u8) ?[]const u8 {
    var pwd: c.passwd = undefined;
    var res: ?*c.passwd = null;
    if (c.getpwuid_r(uid, &pwd, buf, buf.len, &res) != 0 or res == null) return null;
    return std.mem.sliceTo(pwd.name orelse return null, 0);
}

pub fn groupName(gid: u32, buf: *[4096]u8) ?[]const u8 {
    var grp: c.group = undefined;
    var res: ?*c.group = null;
    if (c.getgrgid_r(gid, &grp, buf, buf.len, &res) != 0 or res == null) return null;
    return std.mem.sliceTo(grp.name orelse return null, 0);
}

/// creates a fresh temporary directory, the caller owns the path
pub fn makeTempDir(gpa: Allocator) (Error || Allocator.Error)![]u8 {
    var template = "/tmp/lsn-XXXXXX".*;
    const res = mkdtemp(&template) orelse return lastError();
    return gpa.dupe(u8, std.mem.sliceTo(res, 0));
}

pub fn joinPath(gpa: Allocator, dir: []const u8, name: []const u8) Allocator.Error![]u8 {
    if (dir.len > 0 and dir[dir.len - 1] == '/') {
        return std.fmt.allocPrint(gpa, "{s}{s}", .{ dir, name });
    }
    return std.fmt.allocPrint(gpa, "{s}/{s}", .{ dir, name });
}

/// the last path component, or the whole path for `/`
pub fn baseName(path: []const u8) []const u8 {
    const trimmed = std.mem.trimEnd(u8, path, "/");
    if (trimmed.len == 0) return path;
    const i = std.mem.findScalarLast(u8, trimmed, '/') orelse return trimmed;
    return trimmed[i + 1 ..];
}

/// the parent directory, `null` for `/` and plain names
pub fn dirName(path: []const u8) ?[]const u8 {
    const trimmed = std.mem.trimEnd(u8, path, "/");
    const i = std.mem.findScalarLast(u8, trimmed, '/') orelse return null;
    if (i == 0) return if (trimmed.len > 1) "/" else null;
    return trimmed[0..i];
}

/// `child` is `parent` or somewhere below it
pub fn isWithin(child: []const u8, parent: []const u8) bool {
    if (!std.mem.startsWith(u8, child, parent)) return false;
    if (child.len == parent.len) return true;
    return parent.len > 0 and (parent[parent.len - 1] == '/' or child[parent.len] == '/');
}

test "path helpers" {
    const t = std.testing;
    try t.expectEqualStrings("c.txt", baseName("/a/b/c.txt"));
    try t.expectEqualStrings("b", baseName("/a/b/"));
    try t.expectEqualStrings("/", baseName("/"));
    try t.expectEqualStrings("/a/b", dirName("/a/b/c").?);
    try t.expectEqualStrings("/", dirName("/a").?);
    try t.expect(dirName("/") == null);
    try t.expect(isWithin("/a/b", "/a"));
    try t.expect(isWithin("/a", "/a"));
    try t.expect(!isWithin("/ab", "/a"));
    try t.expect(isWithin("/x", "/"));
}
