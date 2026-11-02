const std = @import("std");

pub const Code = union(enum) {
    /// a unicode codepoint. shifted letters arrive as uppercase.
    char: u21,
    enter,
    esc,
    tab,
    backtab,
    backspace,
    delete,
    up,
    down,
    left,
    right,
    home,
    end,
    page_up,
    page_down,
    f: u8,
};

pub const Key = struct {
    code: Code,
    ctrl: bool = false,
    alt: bool = false,

    pub fn char(c: u21) Key {
        return .{ .code = .{ .char = c } };
    }

    pub fn ctrlChar(c: u21) Key {
        return .{ .code = .{ .char = c }, .ctrl = true };
    }

    pub fn of(code: Code) Key {
        return .{ .code = code };
    }

    pub fn eql(a: Key, b: Key) bool {
        if (a.ctrl != b.ctrl or a.alt != b.alt) return false;
        return switch (a.code) {
            .char => |c| b.code == .char and b.code.char == c,
            .f => |n| b.code == .f and b.code.f == n,
            else => std.meta.activeTag(a.code) == std.meta.activeTag(b.code),
        };
    }

    /// short name for the help screen
    pub fn label(self: Key, buf: []u8) []const u8 {
        var w: std.Io.Writer = .fixed(buf);
        if (self.ctrl) w.writeAll("C-") catch {};
        if (self.alt) w.writeAll("M-") catch {};
        switch (self.code) {
            .char => |c| if (c == ' ') w.writeAll("space") catch {} else {
                var tmp: [4]u8 = undefined;
                const n = std.unicode.utf8Encode(c, &tmp) catch 0;
                w.writeAll(tmp[0..n]) catch {};
            },
            .f => |n| w.print("F{d}", .{n}) catch {},
            .enter => w.writeAll("enter") catch {},
            .esc => w.writeAll("esc") catch {},
            .tab => w.writeAll("tab") catch {},
            .backtab => w.writeAll("S-tab") catch {},
            .backspace => w.writeAll("bksp") catch {},
            .delete => w.writeAll("del") catch {},
            .up => w.writeAll("↑") catch {},
            .down => w.writeAll("↓") catch {},
            .left => w.writeAll("←") catch {},
            .right => w.writeAll("→") catch {},
            .home => w.writeAll("home") catch {},
            .end => w.writeAll("end") catch {},
            .page_up => w.writeAll("pgup") catch {},
            .page_down => w.writeAll("pgdn") catch {},
        }
        return w.buffered();
    }
};
