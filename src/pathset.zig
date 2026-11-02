const std = @import("std");
const Allocator = std.mem.Allocator;

/// a set of owned paths. used for marks and the clipboard, which work with
/// paths instead of handles so they stay valid across tabs and trees.
pub const PathSet = struct {
    map: std.StringArrayHashMapUnmanaged(void) = .empty,

    pub fn deinit(self: *PathSet, gpa: Allocator) void {
        self.clear(gpa);
        self.map.deinit(gpa);
    }

    pub fn count(self: *const PathSet) usize {
        return self.map.count();
    }

    pub fn contains(self: *const PathSet, path: []const u8) bool {
        return self.map.contains(path);
    }

    pub fn insert(self: *PathSet, gpa: Allocator, path: []const u8) Allocator.Error!void {
        if (self.map.contains(path)) return;
        const owned = try gpa.dupe(u8, path);
        errdefer gpa.free(owned);
        try self.map.put(gpa, owned, {});
    }

    /// returns whether the path was in the set
    pub fn remove(self: *PathSet, gpa: Allocator, path: []const u8) bool {
        const kv = self.map.fetchOrderedRemove(path) orelse return false;
        gpa.free(kv.key);
        return true;
    }

    /// adds the path if it is missing, removes it otherwise
    pub fn toggle(self: *PathSet, gpa: Allocator, path: []const u8) Allocator.Error!void {
        if (!self.remove(gpa, path)) try self.insert(gpa, path);
    }

    pub fn clear(self: *PathSet, gpa: Allocator) void {
        for (self.map.keys()) |k| gpa.free(k);
        self.map.clearRetainingCapacity();
    }

    /// keeps only the paths for which `keep` returns true
    pub fn retain(self: *PathSet, gpa: Allocator, keep: *const fn ([]const u8) bool) void {
        var i: usize = 0;
        while (i < self.map.count()) {
            const key = self.map.keys()[i];
            if (keep(key)) {
                i += 1;
            } else {
                self.map.orderedRemoveAt(i);
                gpa.free(key);
            }
        }
    }

    /// the paths sorted, borrowed from the set
    pub fn sorted(self: *const PathSet, gpa: Allocator) Allocator.Error![]const []const u8 {
        const out = try gpa.dupe([]const u8, self.map.keys());
        std.mem.sort([]const u8, out, {}, struct {
            fn lt(_: void, a: []const u8, b: []const u8) bool {
                return std.mem.lessThan(u8, a, b);
            }
        }.lt);
        return out;
    }

    pub fn clone(self: *const PathSet, gpa: Allocator) Allocator.Error!PathSet {
        var out: PathSet = .{};
        errdefer out.deinit(gpa);
        for (self.map.keys()) |k| try out.insert(gpa, k);
        return out;
    }
};

test "path set" {
    const gpa = std.testing.allocator;
    var set: PathSet = .{};
    defer set.deinit(gpa);
    try set.insert(gpa, "/b");
    try set.insert(gpa, "/a");
    try set.insert(gpa, "/a");
    try std.testing.expectEqual(2, set.count());
    try set.toggle(gpa, "/b");
    try std.testing.expect(!set.contains("/b"));

    try set.insert(gpa, "/c");
    const list = try set.sorted(gpa);
    defer gpa.free(list);
    try std.testing.expectEqualStrings("/a", list[0]);
    try std.testing.expectEqualStrings("/c", list[1]);
}
