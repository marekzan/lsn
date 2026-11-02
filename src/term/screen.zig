//! a grid of styled cells and the diff that turns two grids into escape codes

const std = @import("std");
const Allocator = std.mem.Allocator;

const width = @import("width.zig");

pub const Color = union(enum) {
    default,
    /// 0-15 are the basic colors, everything else is the 256 color palette
    index: u8,

    pub const black: Color = .{ .index = 0 };
    pub const red: Color = .{ .index = 1 };
    pub const green: Color = .{ .index = 2 };
    pub const yellow: Color = .{ .index = 3 };
    pub const blue: Color = .{ .index = 4 };
    pub const magenta: Color = .{ .index = 5 };
    pub const cyan: Color = .{ .index = 6 };
    pub const gray: Color = .{ .index = 8 };

    fn eql(a: Color, b: Color) bool {
        return switch (a) {
            .default => b == .default,
            .index => |i| b == .index and b.index == i,
        };
    }
};

pub const Style = struct {
    fg: Color = .default,
    bg: Color = .default,
    bold: bool = false,
    dim: bool = false,
    italic: bool = false,
    underline: bool = false,
    reverse: bool = false,

    pub fn eql(a: Style, b: Style) bool {
        return a.fg.eql(b.fg) and a.bg.eql(b.bg) and a.bold == b.bold and a.dim == b.dim and
            a.italic == b.italic and a.underline == b.underline and a.reverse == b.reverse;
    }

    /// `overlay` on top of `self`: colors are replaced if set, flags are added
    pub fn patch(self: Style, overlay: Style) Style {
        var s = self;
        if (overlay.fg != .default) s.fg = overlay.fg;
        if (overlay.bg != .default) s.bg = overlay.bg;
        s.bold = s.bold or overlay.bold;
        s.dim = s.dim or overlay.dim;
        s.italic = s.italic or overlay.italic;
        s.underline = s.underline or overlay.underline;
        s.reverse = s.reverse or overlay.reverse;
        return s;
    }

    fn write(self: Style, out: *std.ArrayList(u8), gpa: Allocator) Allocator.Error!void {
        try out.appendSlice(gpa, "\x1b[0");
        if (self.bold) try out.appendSlice(gpa, ";1");
        if (self.dim) try out.appendSlice(gpa, ";2");
        if (self.italic) try out.appendSlice(gpa, ";3");
        if (self.underline) try out.appendSlice(gpa, ";4");
        if (self.reverse) try out.appendSlice(gpa, ";7");
        try writeColor(out, gpa, self.fg, 30);
        try writeColor(out, gpa, self.bg, 40);
        try out.append(gpa, 'm');
    }
};

fn writeColor(out: *std.ArrayList(u8), gpa: Allocator, color: Color, base: u8) Allocator.Error!void {
    switch (color) {
        .default => {},
        .index => |i| if (i < 8) {
            try out.print(gpa, ";{d}", .{base + i});
        } else if (i < 16) {
            try out.print(gpa, ";{d}", .{base + 60 + i - 8});
        } else {
            try out.print(gpa, ";{d};5;{d}", .{ base + 8, i });
        },
    }
}

pub const Cell = struct {
    /// utf-8 of the char plus any combining marks
    bytes: [16]u8 = .{ ' ', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0 },
    len: u8 = 1,
    style: Style = .{},
    /// the right half of a wide char, nothing is printed for it
    continuation: bool = false,

    fn eql(a: *const Cell, b: *const Cell) bool {
        return a.len == b.len and a.continuation == b.continuation and
            std.mem.eql(u8, a.bytes[0..a.len], b.bytes[0..b.len]) and a.style.eql(b.style);
    }

    pub fn text(self: *const Cell) []const u8 {
        return self.bytes[0..self.len];
    }
};

pub const Buffer = struct {
    width: u16 = 0,
    height: u16 = 0,
    cells: []Cell = &.{},

    pub fn init(gpa: Allocator, w: u16, h: u16) Allocator.Error!Buffer {
        const cells = try gpa.alloc(Cell, @as(usize, w) * h);
        @memset(cells, .{});
        return .{ .width = w, .height = h, .cells = cells };
    }

    pub fn deinit(self: *Buffer, gpa: Allocator) void {
        gpa.free(self.cells);
        self.* = .{};
    }

    pub fn resize(self: *Buffer, gpa: Allocator, w: u16, h: u16) Allocator.Error!void {
        if (self.width == w and self.height == h) return;
        const new = try init(gpa, w, h);
        self.deinit(gpa);
        self.* = new;
    }

    pub fn clear(self: *Buffer) void {
        @memset(self.cells, .{});
    }

    pub fn cell(self: *Buffer, x: u16, y: u16) *Cell {
        return &self.cells[@as(usize, y) * self.width + x];
    }

    /// sets the style of a span without changing the text (backgrounds)
    pub fn paint(self: *Buffer, x: u16, y: u16, w: u16, style: Style) void {
        if (y >= self.height) return;
        var i = x;
        while (i < x +| w and i < self.width) : (i += 1) {
            const c = self.cell(i, y);
            c.style = c.style.patch(style);
        }
    }

    /// prints `s` at x/y, clipped to `max` columns and the buffer. returns
    /// the columns used. the style is put on top of what is already there,
    /// so a row background survives.
    pub fn print(self: *Buffer, x: u16, y: u16, max: u16, s: []const u8, style: Style) u16 {
        if (y >= self.height) return 0;
        const limit = @min(@as(usize, x) + max, self.width);
        var col: usize = x;
        var it = (std.unicode.Utf8View.init(s) catch return 0).iterator();
        var last: ?usize = null;
        while (it.nextCodepointSlice()) |bytes| {
            const cp = std.unicode.utf8Decode(bytes) catch continue;
            const w = width.codepoint(cp);
            if (w == 0) {
                // combining mark: goes into the previous cell
                if (last) |l| {
                    const c = &self.cells[@as(usize, y) * self.width + l];
                    if (c.len + bytes.len <= c.bytes.len) {
                        @memcpy(c.bytes[c.len..][0..bytes.len], bytes);
                        c.len += @intCast(bytes.len);
                    }
                }
                continue;
            }
            if (col + w > limit) break;
            const c = self.cell(@intCast(col), y);
            @memcpy(c.bytes[0..bytes.len], bytes);
            c.len = @intCast(bytes.len);
            c.continuation = false;
            c.style = c.style.patch(style);
            if (w == 2) {
                const next = self.cell(@intCast(col + 1), y);
                next.* = .{ .continuation = true, .style = c.style, .len = 0 };
            }
            last = col;
            col += w;
        }
        return @intCast(col - x);
    }
};

/// appends the escape codes that turn `prev` into `next` to `out`.
/// `origin_row` is the screen row of the first buffer row (inline mode).
/// with `full` every cell is written.
pub fn diff(gpa: Allocator, out: *std.ArrayList(u8), prev: *const Buffer, next: *const Buffer, origin_row: u16, full: bool) Allocator.Error!void {
    var style: ?Style = null;
    var cursor: ?[2]usize = null;
    for (0..next.height) |y| {
        for (0..next.width) |x| {
            const i = y * next.width + x;
            const c = &next.cells[i];
            if (c.continuation) continue;
            const same = !full and prev.width == next.width and prev.height == next.height and c.eql(&prev.cells[i]);
            if (same) continue;

            if (cursor == null or cursor.?[0] != x or cursor.?[1] != y) {
                try out.print(gpa, "\x1b[{d};{d}H", .{ origin_row + y + 1, x + 1 });
            }
            if (style == null or !style.?.eql(c.style)) {
                try c.style.write(out, gpa);
                style = c.style;
            }
            try out.appendSlice(gpa, c.text());
            const w = width.str(c.text());
            cursor = .{ x + @max(w, 1), y };
        }
    }
    if (style != null) try out.appendSlice(gpa, "\x1b[0m");
}

const testing = std.testing;

test "print clips and handles wide chars" {
    var buf = try Buffer.init(testing.allocator, 6, 1);
    defer buf.deinit(testing.allocator);
    try testing.expectEqual(4, buf.print(0, 0, 10, "a日b", .{}));
    try testing.expect(buf.cell(2, 0).continuation);
    try testing.expectEqual(2, buf.print(4, 0, 10, "xyz", .{}));
    try testing.expectEqualStrings("y", buf.cell(5, 0).text());
}

test "diff only writes changed cells" {
    const gpa = testing.allocator;
    var a = try Buffer.init(gpa, 4, 2);
    defer a.deinit(gpa);
    var b = try Buffer.init(gpa, 4, 2);
    defer b.deinit(gpa);
    _ = b.print(1, 1, 4, "hi", .{ .bold = true });

    var out: std.ArrayList(u8) = .empty;
    defer out.deinit(gpa);
    try diff(gpa, &out, &a, &b, 10, false);
    try testing.expectEqualStrings("\x1b[12;2H\x1b[0;1mhi\x1b[0m", out.items);

    out.clearRetainingCapacity();
    try diff(gpa, &out, &b, &b, 10, false);
    try testing.expectEqualStrings("", out.items);
}
