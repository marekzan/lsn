const std = @import("std");
const Allocator = std.mem.Allocator;

const Key = @import("term/key.zig").Key;

/// a single line text input used by the prompts and the jump
pub const Input = struct {
    buf: std.ArrayList(u8) = .empty,
    /// byte offset, always on a codepoint boundary
    cursor: usize = 0,

    pub fn init(gpa: Allocator, initial: []const u8) Allocator.Error!Input {
        var input: Input = .{};
        try input.buf.appendSlice(gpa, initial);
        input.cursor = initial.len;
        return input;
    }

    pub fn deinit(self: *Input, gpa: Allocator) void {
        self.buf.deinit(gpa);
    }

    pub fn text(self: *const Input) []const u8 {
        return self.buf.items;
    }

    pub fn clear(self: *Input) void {
        self.buf.clearRetainingCapacity();
        self.cursor = 0;
    }

    /// returns whether the text changed
    pub fn handle(self: *Input, gpa: Allocator, key: Key) Allocator.Error!bool {
        switch (key.code) {
            .char => |c| {
                if (key.ctrl) return switch (c) {
                    'u' => blk: {
                        self.clear();
                        break :blk true;
                    },
                    'w' => self.deleteWord(),
                    'a' => blk: {
                        self.cursor = 0;
                        break :blk false;
                    },
                    'e' => blk: {
                        self.cursor = self.buf.items.len;
                        break :blk false;
                    },
                    else => false,
                };
                if (key.alt) return false;
                var tmp: [4]u8 = undefined;
                const n = std.unicode.utf8Encode(c, &tmp) catch return false;
                try self.buf.insertSlice(gpa, self.cursor, tmp[0..n]);
                self.cursor += n;
                return true;
            },
            .backspace => {
                if (self.cursor == 0) return false;
                const start = self.prev(self.cursor);
                self.buf.replaceRangeAssumeCapacity(start, self.cursor - start, &.{});
                self.cursor = start;
                return true;
            },
            .delete => {
                if (self.cursor >= self.buf.items.len) return false;
                const end = self.next(self.cursor);
                self.buf.replaceRangeAssumeCapacity(self.cursor, end - self.cursor, &.{});
                return true;
            },
            .left => self.cursor = self.prev(self.cursor),
            .right => self.cursor = self.next(self.cursor),
            .home => self.cursor = 0,
            .end => self.cursor = self.buf.items.len,
            else => {},
        }
        return false;
    }

    fn deleteWord(self: *Input) bool {
        const t = self.buf.items;
        var start = self.cursor;
        while (start > 0 and t[start - 1] == ' ') start -= 1;
        while (start > 0 and t[start - 1] != ' ' and t[start - 1] != '/') start -= 1;
        if (start == self.cursor) return false;
        self.buf.replaceRangeAssumeCapacity(start, self.cursor - start, &.{});
        self.cursor = start;
        return true;
    }

    fn prev(self: *const Input, i: usize) usize {
        var j = i;
        while (j > 0) {
            j -= 1;
            if (self.buf.items[j] & 0xC0 != 0x80) break;
        }
        return j;
    }

    fn next(self: *const Input, i: usize) usize {
        var j = @min(i + 1, self.buf.items.len);
        while (j < self.buf.items.len and self.buf.items[j] & 0xC0 == 0x80) j += 1;
        return j;
    }

    /// number of codepoints before the cursor, for placing the terminal cursor
    pub fn cursorColumn(self: *const Input) usize {
        return std.unicode.utf8CountCodepoints(self.buf.items[0..self.cursor]) catch self.cursor;
    }
};

const testing = std.testing;

test "editing with multibyte chars" {
    var input = try Input.init(testing.allocator, "äb");
    defer input.deinit(testing.allocator);
    _ = try input.handle(testing.allocator, Key.of(.left));
    _ = try input.handle(testing.allocator, Key.char('ö'));
    try testing.expectEqualStrings("äöb", input.text());
    _ = try input.handle(testing.allocator, Key.of(.backspace));
    _ = try input.handle(testing.allocator, Key.of(.delete));
    try testing.expectEqualStrings("ä", input.text());
    try testing.expectEqual(1, input.cursorColumn());
}

test "delete word" {
    var input = try Input.init(testing.allocator, "src/foo bar");
    defer input.deinit(testing.allocator);
    _ = try input.handle(testing.allocator, Key.ctrlChar('w'));
    try testing.expectEqualStrings("src/foo ", input.text());
    _ = try input.handle(testing.allocator, Key.ctrlChar('w'));
    try testing.expectEqualStrings("src/", input.text());
}
