const std = @import("std");
const Allocator = std.mem.Allocator;

const sys = @import("sys.zig");

/// pinned folders, stored as one path per line in `$XDG_DATA_HOME/lsn/pins`
pub const Pins = struct {
    gpa: Allocator,
    paths: std.ArrayList([]u8) = .empty,
    /// `null` keeps the pins in memory only (tests)
    file: ?[]u8 = null,

    pub fn load(gpa: Allocator) Allocator.Error!Pins {
        var pins: Pins = .{ .gpa = gpa, .file = try defaultFile(gpa) };
        const file = pins.file orelse return pins;
        const fd = sys.open(file, .read, 0) catch return pins;
        defer sys.close(fd);
        var buf: [64 * 1024]u8 = undefined;
        const n = sys.readAll(fd, &buf) catch return pins;
        var lines = std.mem.splitScalar(u8, buf[0..n], '\n');
        while (lines.next()) |line| {
            if (line.len > 0) try pins.paths.append(gpa, try gpa.dupe(u8, line));
        }
        return pins;
    }

    pub fn inMemory(gpa: Allocator) Pins {
        return .{ .gpa = gpa };
    }

    pub fn deinit(self: *Pins) void {
        for (self.paths.items) |p| self.gpa.free(p);
        self.paths.deinit(self.gpa);
        if (self.file) |f| self.gpa.free(f);
    }

    pub fn list(self: *const Pins) []const []const u8 {
        return @ptrCast(self.paths.items);
    }

    pub fn contains(self: *const Pins, path: []const u8) bool {
        return self.indexOf(path) != null;
    }

    fn indexOf(self: *const Pins, path: []const u8) ?usize {
        for (self.paths.items, 0..) |p, i| if (std.mem.eql(u8, p, path)) return i;
        return null;
    }

    /// returns whether the path is pinned now
    pub fn toggle(self: *Pins, path: []const u8) !bool {
        if (self.indexOf(path)) |i| {
            self.gpa.free(self.paths.orderedRemove(i));
            try self.save();
            return false;
        }
        try self.paths.append(self.gpa, try self.gpa.dupe(u8, path));
        try self.save();
        return true;
    }

    pub fn remove(self: *Pins, path: []const u8) !void {
        const i = self.indexOf(path) orelse return;
        self.gpa.free(self.paths.orderedRemove(i));
        try self.save();
    }

    fn save(self: *const Pins) !void {
        const file = self.file orelse return;
        if (sys.dirName(file)) |dir| try mkdirAll(dir);
        const fd = try sys.open(file, .write_truncate, 0o644);
        defer sys.close(fd);
        for (self.paths.items) |p| {
            try sys.writeAll(fd, p);
            try sys.writeAll(fd, "\n");
        }
    }
};

fn mkdirAll(path: []const u8) sys.Error!void {
    if (sys.isDir(path)) return;
    if (sys.dirName(path)) |parent| try mkdirAll(parent);
    sys.mkdir(path, 0o755) catch |err| if (err != error.AlreadyExists) return err;
}

fn defaultFile(gpa: Allocator) Allocator.Error!?[]u8 {
    if (sys.getenv("XDG_DATA_HOME")) |data| {
        if (data.len > 0 and data[0] == '/') return try std.fmt.allocPrint(gpa, "{s}/lsn/pins", .{data});
    }
    const home = sys.getenv("HOME") orelse return null;
    return try std.fmt.allocPrint(gpa, "{s}/.local/share/lsn/pins", .{home});
}
