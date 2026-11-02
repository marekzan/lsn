//! turns the bytes a terminal sends into keys

const std = @import("std");
const key_mod = @import("key.zig");
const Key = key_mod.Key;
const Code = key_mod.Code;

pub const Result = union(enum) {
    /// a key and how many bytes it used
    key: struct { key: Key, len: usize },
    /// bytes that mean nothing to us (mouse, focus, unknown sequences)
    skip: usize,
    /// the bytes so far are the start of a longer sequence, wait for more
    incomplete,
};

pub fn parse(bytes: []const u8) Result {
    if (bytes.len == 0) return .incomplete;
    const b0 = bytes[0];
    switch (b0) {
        0x1b => return parseEscape(bytes),
        '\r', '\n' => return one(Key.of(.enter), 1),
        '\t' => return one(Key.of(.tab), 1),
        0x7f, 0x08 => return one(Key.of(.backspace), 1),
        0x01...0x07, 0x0b, 0x0c, 0x0e...0x1a => return one(Key.ctrlChar('a' + b0 - 1), 1),
        0x00, 0x1c...0x1f => return .{ .skip = 1 },
        else => {},
    }
    const len = std.unicode.utf8ByteSequenceLength(b0) catch return .{ .skip = 1 };
    if (bytes.len < len) return .incomplete;
    const cp = std.unicode.utf8Decode(bytes[0..len]) catch return .{ .skip = 1 };
    return one(Key.char(cp), len);
}

fn one(key: Key, len: usize) Result {
    return .{ .key = .{ .key = key, .len = len } };
}

fn parseEscape(bytes: []const u8) Result {
    if (bytes.len == 1) return .incomplete;
    switch (bytes[1]) {
        '[' => return parseCsi(bytes),
        'O' => {
            if (bytes.len < 3) return .incomplete;
            const code: ?Code = switch (bytes[2]) {
                'A' => .up,
                'B' => .down,
                'C' => .right,
                'D' => .left,
                'H' => .home,
                'F' => .end,
                'P'...'S' => .{ .f = bytes[2] - 'P' + 1 },
                else => null,
            };
            return if (code) |c| one(Key.of(c), 3) else .{ .skip = 3 };
        },
        0x1b => return one(Key.of(.esc), 1),
        else => {
            // alt + key
            const inner = parse(bytes[1..]);
            return switch (inner) {
                .key => |k| blk: {
                    var key = k.key;
                    key.alt = true;
                    break :blk one(key, k.len + 1);
                },
                .skip => |n| .{ .skip = n + 1 },
                .incomplete => .incomplete,
            };
        },
    }
}

fn parseCsi(bytes: []const u8) Result {
    // ESC [ params final
    var i: usize = 2;
    while (i < bytes.len) : (i += 1) {
        const ch = bytes[i];
        if (ch >= 0x40 and ch <= 0x7e) break;
    } else return .incomplete;
    const final = bytes[i];
    const len = i + 1;
    const params = bytes[2..i];

    var nums: [4]u16 = .{ 0, 0, 0, 0 };
    var count: usize = 0;
    var it = std.mem.splitScalar(u8, params, ';');
    while (it.next()) |p| : (count += 1) {
        if (count >= nums.len) break;
        nums[count] = std.fmt.parseInt(u16, p, 10) catch 0;
    }

    const code: ?Code = switch (final) {
        'A' => .up,
        'B' => .down,
        'C' => .right,
        'D' => .left,
        'H' => .home,
        'F' => .end,
        'Z' => .backtab,
        '~' => switch (nums[0]) {
            1, 7 => .home,
            3 => .delete,
            4, 8 => .end,
            5 => .page_up,
            6 => .page_down,
            else => null,
        },
        else => null,
    };
    const c = code orelse return .{ .skip = len };
    var key = Key.of(c);
    // xterm modifiers: 1 + (shift 1 | alt 2 | ctrl 4)
    if (count >= 2 and nums[1] > 1) {
        const m = nums[1] - 1;
        key.alt = m & 2 != 0;
        key.ctrl = m & 4 != 0;
    }
    return one(key, len);
}

/// the answer to a cursor position query: `ESC [ row ; col R`
pub fn cursorReport(bytes: []const u8) ?struct { row: u16, col: u16, len: usize } {
    const start = std.mem.find(u8, bytes, "\x1b[") orelse return null;
    const end = std.mem.findScalarPos(u8, bytes, start, 'R') orelse return null;
    const body = bytes[start + 2 .. end];
    const semi = std.mem.findScalar(u8, body, ';') orelse return null;
    const row = std.fmt.parseInt(u16, body[0..semi], 10) catch return null;
    const col = std.fmt.parseInt(u16, body[semi + 1 ..], 10) catch return null;
    return .{ .row = row, .col = col, .len = end + 1 };
}

const testing = std.testing;

fn expectKey(bytes: []const u8, want: Key, len: usize) !void {
    const r = parse(bytes);
    try testing.expect(r == .key);
    try testing.expect(r.key.key.eql(want));
    try testing.expectEqual(len, r.key.len);
}

test "plain keys and control chars" {
    try expectKey("j", Key.char('j'), 1);
    try expectKey("G", Key.char('G'), 1);
    try expectKey("ä", Key.char('ä'), 2);
    try expectKey("\r", Key.of(.enter), 1);
    try expectKey("\x7f", Key.of(.backspace), 1);
    try expectKey("\x04", Key.ctrlChar('d'), 1);
    try expectKey("\x17", Key.ctrlChar('w'), 1);
    try testing.expect(parse("\xc3") == .incomplete);
}

test "escape sequences" {
    try expectKey("\x1b[A", Key.of(.up), 3);
    try expectKey("\x1bOB", Key.of(.down), 3);
    try expectKey("\x1b[3~", Key.of(.delete), 4);
    try expectKey("\x1b[6~", Key.of(.page_down), 4);
    try expectKey("\x1b[Z", Key.of(.backtab), 3);
    try expectKey("\x1b[1;5C", .{ .code = .right, .ctrl = true }, 6);
    try expectKey("\x1bj", .{ .code = .{ .char = 'j' }, .alt = true }, 2);
    try testing.expect(parse("\x1b") == .incomplete);
    try testing.expect(parse("\x1b[1;5") == .incomplete);
    try testing.expectEqual(Result{ .skip = 3 }, parse("\x1b[I"));
}

test "cursor report" {
    const r = cursorReport("\x1b[12;1R").?;
    try testing.expectEqual(12, r.row);
    try testing.expectEqual(1, r.col);
    try testing.expectEqual(7, r.len);
}
