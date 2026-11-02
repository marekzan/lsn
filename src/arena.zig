const std = @import("std");
const Allocator = std.mem.Allocator;

/// we return this to the caller.
/// this can be used to get data back from the arena
pub const Handle = struct {
    index: usize,
    generation: u64,

    pub fn eql(a: Handle, b: Handle) bool {
        return a.index == b.index and a.generation == b.generation;
    }
};

/// a generational arena: values live in one vec, handles stay valid until
/// their value is removed and are never confused with a value that later
/// reuses the same slot.
pub fn Arena(comptime T: type) type {
    return struct {
        const Self = @This();

        /// a slot represents an already used (free) space in the data vec or
        /// a currently occupied space.
        ///
        /// if it is occupied, the slot holds the actual data which the user can
        /// get back using the handle and the current generation of this slot.
        ///
        /// if the slot is free then it points to the next free index (which
        /// might be another free slot or a true empty index in the vec) with
        /// their current generation
        const Slot = union(enum) {
            occupied: struct { value: T, generation: u64 },
            free: struct { next_free: ?usize, generation: u64 },
        };

        data: std.ArrayList(Slot) = .empty,
        free_slot: ?usize = null,
        count: usize = 0,

        pub fn deinit(self: *Self, gpa: Allocator) void {
            self.data.deinit(gpa);
        }

        pub fn insert(self: *Self, gpa: Allocator, value: T) Allocator.Error!Handle {
            if (self.free_slot) |idx| {
                // get current generation and the next free index from the current free slot
                const free = switch (self.data.items[idx]) {
                    .free => |f| f,
                    .occupied => @panic("corrupt free list"),
                };
                self.free_slot = free.next_free;
                self.data.items[idx] = .{ .occupied = .{ .value = value, .generation = free.generation } };
                self.count += 1;
                return .{ .index = idx, .generation = free.generation };
            }

            // no free slot so we create a new occupied one
            const idx = self.data.items.len;
            try self.data.append(gpa, .{ .occupied = .{ .value = value, .generation = 0 } });
            self.count += 1;
            return .{ .index = idx, .generation = 0 };
        }

        pub fn remove(self: *Self, handle: Handle) ?T {
            if (self.get(handle) == null) return null;

            const old = self.data.items[handle.index];
            self.data.items[handle.index] = .{ .free = .{
                .next_free = self.free_slot,
                .generation = handle.generation + 1,
            } };
            self.free_slot = handle.index;
            self.count -= 1;
            return old.occupied.value;
        }

        /// the pointer is only valid until the next insert
        pub fn get(self: *const Self, handle: Handle) ?*const T {
            if (handle.index >= self.data.items.len) return null;
            return switch (self.data.items[handle.index]) {
                .occupied => |*o| if (o.generation == handle.generation) &o.value else null,
                .free => null,
            };
        }

        /// the pointer is only valid until the next insert
        pub fn getMut(self: *Self, handle: Handle) ?*T {
            if (handle.index >= self.data.items.len) return null;
            return switch (self.data.items[handle.index]) {
                .occupied => |*o| if (o.generation == handle.generation) &o.value else null,
                .free => null,
            };
        }

        /// number of occupied slots
        pub fn len(self: *const Self) usize {
            return self.count;
        }
    };
}

const testing = std.testing;

test "insert and get" {
    var arena: Arena(i32) = .{};
    defer arena.deinit(testing.allocator);
    const h1 = try arena.insert(testing.allocator, 10);
    const h2 = try arena.insert(testing.allocator, 20);
    try testing.expectEqual(10, arena.get(h1).?.*);
    try testing.expectEqual(20, arena.get(h2).?.*);
}

test "remove" {
    var arena: Arena(i32) = .{};
    defer arena.deinit(testing.allocator);
    const h1 = try arena.insert(testing.allocator, 10);
    try testing.expectEqual(10, arena.remove(h1).?);
    try testing.expect(arena.get(h1) == null);
    // double remove should fail
    try testing.expect(arena.remove(h1) == null);
}

test "reuse slot with a new generation" {
    var arena: Arena(i32) = .{};
    defer arena.deinit(testing.allocator);
    const h1 = try arena.insert(testing.allocator, 10);
    _ = arena.remove(h1);
    const h2 = try arena.insert(testing.allocator, 30);
    try testing.expectEqual(h1.index, h2.index);
    try testing.expect(h2.generation > h1.generation);
    // old handle should not access new data
    try testing.expect(arena.get(h1) == null);
    try testing.expectEqual(30, arena.get(h2).?.*);
}

test "get mut" {
    var arena: Arena(i32) = .{};
    defer arena.deinit(testing.allocator);
    const h = try arena.insert(testing.allocator, 1);
    arena.getMut(h).?.* += 41;
    try testing.expectEqual(42, arena.get(h).?.*);
}

// random sequences of operations must behave exactly like a hash map
test "arena matches a hash map model" {
    const gpa = testing.allocator;
    var prng = std.Random.DefaultPrng.init(0x1234);
    const rand = prng.random();

    for (0..50) |_| {
        var arena: Arena(u32) = .{};
        defer arena.deinit(gpa);
        var model: std.AutoHashMapUnmanaged(Handle, u32) = .empty;
        defer model.deinit(gpa);
        var handles: std.ArrayList(Handle) = .empty;
        defer handles.deinit(gpa);

        for (0..rand.uintLessThan(usize, 400)) |_| {
            const pick = if (handles.items.len > 0) handles.items[rand.uintLessThan(usize, handles.items.len)] else null;
            switch (rand.uintLessThan(u8, 5)) {
                0 => {
                    const v = rand.int(u32);
                    const h = try arena.insert(gpa, v);
                    try model.put(gpa, h, v);
                    try handles.append(gpa, h);
                },
                1 => if (pick) |h| {
                    try testing.expectEqual(model.fetchRemove(h) != null, arena.remove(h) != null);
                },
                2 => if (pick) |h| {
                    const a = if (arena.get(h)) |p| p.* else null;
                    try testing.expectEqual(model.get(h), a);
                },
                3 => if (pick) |h| {
                    const v = rand.int(u32);
                    if (model.getPtr(h)) |p| p.* = v;
                    if (arena.getMut(h)) |p| p.* = v;
                    const a = if (arena.get(h)) |p| p.* else null;
                    try testing.expectEqual(model.get(h), a);
                },
                else => {
                    // a forged handle that is very likely invalid
                    const h: Handle = .{ .index = rand.uintLessThan(usize, 500), .generation = rand.uintLessThan(u64, 4) };
                    const a = if (arena.get(h)) |p| p.* else null;
                    try testing.expectEqual(model.get(h), a);
                },
            }
            try testing.expectEqual(model.count(), arena.len());
        }
    }
}
