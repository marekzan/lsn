//! display width of codepoints.
//!
//! a small subset of wcwidth: combining marks take no column, east asian
//! wide chars and most emoji take two. everything else takes one. good
//! enough for file names, without pulling in the unicode tables.

const std = @import("std");

const Range = struct { lo: u21, hi: u21 };

const zero = [_]Range{
    .{ .lo = 0x0300, .hi = 0x036F },
    .{ .lo = 0x0483, .hi = 0x0489 },
    .{ .lo = 0x0591, .hi = 0x05BD },
    .{ .lo = 0x0610, .hi = 0x061A },
    .{ .lo = 0x064B, .hi = 0x065F },
    .{ .lo = 0x0E31, .hi = 0x0E31 },
    .{ .lo = 0x0E34, .hi = 0x0E3A },
    .{ .lo = 0x1AB0, .hi = 0x1AFF },
    .{ .lo = 0x1DC0, .hi = 0x1DFF },
    .{ .lo = 0x200B, .hi = 0x200F },
    .{ .lo = 0x20D0, .hi = 0x20FF },
    .{ .lo = 0xFE00, .hi = 0xFE0F },
    .{ .lo = 0xFE20, .hi = 0xFE2F },
    .{ .lo = 0x1F3FB, .hi = 0x1F3FF },
    .{ .lo = 0xE0100, .hi = 0xE01EF },
};

const wide = [_]Range{
    .{ .lo = 0x1100, .hi = 0x115F },
    .{ .lo = 0x231A, .hi = 0x231B },
    .{ .lo = 0x23E9, .hi = 0x23EC },
    .{ .lo = 0x25FD, .hi = 0x25FE },
    .{ .lo = 0x2614, .hi = 0x2615 },
    .{ .lo = 0x2648, .hi = 0x2653 },
    .{ .lo = 0x26AA, .hi = 0x26AB },
    .{ .lo = 0x26BD, .hi = 0x26BE },
    .{ .lo = 0x26F5, .hi = 0x26F5 },
    .{ .lo = 0x26FD, .hi = 0x26FD },
    .{ .lo = 0x2705, .hi = 0x2705 },
    .{ .lo = 0x270A, .hi = 0x270B },
    .{ .lo = 0x2728, .hi = 0x2728 },
    .{ .lo = 0x274C, .hi = 0x274C },
    .{ .lo = 0x2753, .hi = 0x2755 },
    .{ .lo = 0x2795, .hi = 0x2797 },
    .{ .lo = 0x2B1B, .hi = 0x2B1C },
    .{ .lo = 0x2E80, .hi = 0x303E },
    .{ .lo = 0x3041, .hi = 0x33FF },
    .{ .lo = 0x3400, .hi = 0x4DBF },
    .{ .lo = 0x4E00, .hi = 0x9FFF },
    .{ .lo = 0xA000, .hi = 0xA4CF },
    .{ .lo = 0xAC00, .hi = 0xD7A3 },
    .{ .lo = 0xF900, .hi = 0xFAFF },
    .{ .lo = 0xFE30, .hi = 0xFE4F },
    .{ .lo = 0xFF00, .hi = 0xFF60 },
    .{ .lo = 0xFFE0, .hi = 0xFFE6 },
    .{ .lo = 0x1F300, .hi = 0x1F64F },
    .{ .lo = 0x1F680, .hi = 0x1F6FF },
    .{ .lo = 0x1F900, .hi = 0x1F9FF },
    .{ .lo = 0x1FA70, .hi = 0x1FAFF },
    .{ .lo = 0x20000, .hi = 0x3FFFD },
};

fn inRanges(cp: u21, ranges: []const Range) bool {
    for (ranges) |r| if (cp >= r.lo and cp <= r.hi) return true;
    return false;
}

pub fn codepoint(cp: u21) u2 {
    if (cp == 0) return 0;
    if (cp < 0x20 or (cp >= 0x7f and cp < 0xa0)) return 0;
    if (cp < 0x300) return 1;
    if (inRanges(cp, &zero)) return 0;
    if (inRanges(cp, &wide)) return 2;
    return 1;
}

/// columns a string takes up
pub fn str(s: []const u8) usize {
    var it = (std.unicode.Utf8View.init(s) catch return s.len).iterator();
    var w: usize = 0;
    while (it.nextCodepoint()) |cp| w += codepoint(cp);
    return w;
}

test "widths" {
    try std.testing.expectEqual(5, str("hello"));
    try std.testing.expectEqual(3, str("äöü"));
    try std.testing.expectEqual(4, str("日本"));
    try std.testing.expectEqual(1, str("e\u{301}"));
    try std.testing.expectEqual(3, str("├─●"));
}
