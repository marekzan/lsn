//! the terminal session: raw mode, viewport and drawing.
//!
//! everything goes through `/dev/tty`, never through stdin/stdout. this way
//! stdout stays free for the picked paths (`cd "$(lsn)"`) and the ui works
//! even when stdout is a pipe.
//!
//! inline: the ui takes `height` lines below the prompt and is cleared on
//! exit, like atuin or fzf. fullscreen: the alternate screen is used.

const std = @import("std");
const Allocator = std.mem.Allocator;
const c = std.c;
const linux = std.os.linux;

const Key = @import("key.zig").Key;
const parse = @import("parse.zig");
const screen = @import("screen.zig");
const sys = @import("../sys.zig");

pub const Display = union(enum) {
    @"inline": u16,
    fullscreen,
};

/// a frame to draw into, see `Terminal.draw`
pub const Frame = struct {
    buf: *screen.Buffer,
    /// where to show the terminal cursor, hidden if `null`
    cursor: ?[2]u16 = null,

    pub fn width(self: *const Frame) u16 {
        return self.buf.width;
    }

    pub fn height(self: *const Frame) u16 {
        return self.buf.height;
    }
};

pub const Terminal = struct {
    gpa: Allocator,
    fd: c_int,
    original: c.termios,
    display: Display,
    /// screen size
    cols: u16 = 0,
    rows: u16 = 0,
    /// first screen row of the viewport (0 in fullscreen)
    top: u16 = 0,
    current: screen.Buffer = .{},
    previous: screen.Buffer = .{},
    /// repaint everything on the next draw (start, resize)
    full: bool = true,
    out: std.ArrayList(u8) = .empty,
    pending: [256]u8 = undefined,
    pending_len: usize = 0,

    pub fn start(gpa: Allocator, display: Display) !*Terminal {
        const fd = c.open("/dev/tty", .{ .ACCMODE = .RDWR, .CLOEXEC = true });
        if (fd < 0) return error.NoTty;
        errdefer sys.close(fd);

        var original: c.termios = undefined;
        if (c.tcgetattr(fd, &original) != 0) return error.NoTty;

        const self = try gpa.create(Terminal);
        errdefer gpa.destroy(self);
        self.* = .{ .gpa = gpa, .fd = fd, .original = original, .display = display };

        var raw = original;
        raw.iflag.BRKINT = false;
        raw.iflag.ICRNL = false;
        raw.iflag.INPCK = false;
        raw.iflag.ISTRIP = false;
        raw.iflag.IXON = false;
        raw.oflag.OPOST = false;
        raw.cflag.CSIZE = .CS8;
        raw.lflag.ECHO = false;
        raw.lflag.ICANON = false;
        raw.lflag.IEXTEN = false;
        raw.lflag.ISIG = false;
        raw.cc[@backingInt(linux.V.MIN)] = 1;
        raw.cc[@backingInt(linux.V.TIME)] = 0;
        if (c.tcsetattr(fd, .FLUSH, &raw) != 0) return error.NoTty;
        active = self;
        errdefer self.restore();

        self.querySize();
        switch (display) {
            .fullscreen => try self.write("\x1b[?1049h\x1b[?25l\x1b[2J"),
            .@"inline" => |wanted| {
                const h = self.viewportHeight(wanted);
                const row = self.queryCursorRow() orelse self.rows -| 1;
                // make room below the prompt: new lines scroll the screen
                // up if the viewport does not fit below the cursor
                try self.write("\x1b[?25l\r");
                for (1..h) |_| try self.write("\n");
                self.top = if (row + h <= self.rows) row else self.rows - h;
            },
        }
        try self.resizeBuffers();
        return self;
    }

    /// restores the terminal; the inline viewport is wiped so no trace is left
    pub fn stop(self: *Terminal) void {
        switch (self.display) {
            .fullscreen => self.write("\x1b[0m\x1b[?1049l\x1b[?25h") catch {},
            .@"inline" => {
                var buf: [32]u8 = undefined;
                const s = std.fmt.bufPrint(&buf, "\x1b[0m\x1b[{d};1H\x1b[J\x1b[?25h", .{self.top + 1}) catch "";
                self.write(s) catch {};
            },
        }
        self.restore();
        self.current.deinit(self.gpa);
        self.previous.deinit(self.gpa);
        self.out.deinit(self.gpa);
        sys.close(self.fd);
        self.gpa.destroy(self);
    }

    fn restore(self: *Terminal) void {
        _ = c.tcsetattr(self.fd, .FLUSH, &self.original);
        if (active == self) active = null;
    }

    fn write(self: *Terminal, bytes: []const u8) !void {
        try sys.writeAll(self.fd, bytes);
    }

    fn viewportHeight(self: *const Terminal, wanted: u16) u16 {
        return std.math.clamp(wanted, 3, @max(self.rows, 3));
    }

    fn querySize(self: *Terminal) void {
        var ws: std.posix.winsize = undefined;
        if (c.ioctl(self.fd, @bitCast(@as(u32, linux.T.IOCGWINSZ)), &ws) == 0 and ws.col > 0 and ws.row > 0) {
            self.cols = ws.col;
            self.rows = ws.row;
        } else {
            self.cols = 80;
            self.rows = 24;
        }
    }

    /// 0 based row of the cursor, asked from the terminal
    fn queryCursorRow(self: *Terminal) ?u16 {
        self.write("\x1b[6n") catch return null;
        var buf: [64]u8 = undefined;
        var len: usize = 0;
        while (len < buf.len) {
            if (!self.waitReadable(500)) return null;
            const n = c.read(self.fd, buf[len..].ptr, buf.len - len);
            if (n <= 0) return null;
            len += @intCast(n);
            if (parse.cursorReport(buf[0..len])) |report| {
                // keep keys the user typed meanwhile
                const rest = buf[report.len..len];
                @memcpy(self.pending[0..rest.len], rest);
                self.pending_len = rest.len;
                return report.row -| 1;
            }
        }
        return null;
    }

    fn resizeBuffers(self: *Terminal) !void {
        const h = switch (self.display) {
            .fullscreen => self.rows,
            .@"inline" => |wanted| self.viewportHeight(wanted),
        };
        try self.current.resize(self.gpa, self.cols, h);
        try self.previous.resize(self.gpa, self.cols, h);
        self.full = true;
    }

    /// checks the terminal size, returns whether it changed
    pub fn checkResize(self: *Terminal) !bool {
        const cols = self.cols;
        const rows = self.rows;
        self.querySize();
        if (cols == self.cols and rows == self.rows) return false;
        switch (self.display) {
            .fullscreen => try self.write("\x1b[2J"),
            .@"inline" => |wanted| {
                const h = self.viewportHeight(wanted);
                if (self.top + h > self.rows) self.top = self.rows - h;
                // the old content may have been reflowed, start clean
                var buf: [32]u8 = undefined;
                try self.write(try std.fmt.bufPrint(&buf, "\x1b[{d};1H\x1b[J", .{self.top + 1}));
            },
        }
        try self.resizeBuffers();
        return true;
    }

    /// calls `drawFn(ctx, frame)` on an empty frame and writes the changes
    pub fn draw(self: *Terminal, ctx: anytype, comptime drawFn: fn (@TypeOf(ctx), *Frame) anyerror!void) !void {
        self.current.clear();
        var frame: Frame = .{ .buf = &self.current };
        try drawFn(ctx, &frame);

        self.out.clearRetainingCapacity();
        try self.out.appendSlice(self.gpa, "\x1b[?25l");
        try screen.diff(self.gpa, &self.out, &self.previous, &self.current, self.top, self.full);
        if (frame.cursor) |pos| {
            try self.out.print(self.gpa, "\x1b[{d};{d}H\x1b[?25h", .{ self.top + pos[1] + 1, pos[0] + 1 });
        }
        try self.write(self.out.items);
        self.full = false;
        std.mem.swap(screen.Buffer, &self.current, &self.previous);
    }

    fn waitReadable(self: *Terminal, timeout_ms: i32) bool {
        var fds = [_]c.pollfd{.{ .fd = self.fd, .events = c.POLL.IN, .revents = 0 }};
        return c.poll(&fds, 1, timeout_ms) > 0;
    }

    /// the next key, `null` if nothing arrived within `timeout_ms`
    pub fn readKey(self: *Terminal, timeout_ms: i32) !?Key {
        while (true) {
            if (self.pending_len > 0) {
                switch (parse.parse(self.pending[0..self.pending_len])) {
                    .key => |k| {
                        self.consume(k.len);
                        return k.key;
                    },
                    .skip => |n| {
                        self.consume(n);
                        continue;
                    },
                    .incomplete => {
                        // a lone escape is the escape key once nothing follows
                        if (!self.waitReadable(30)) {
                            if (self.pending[0] == 0x1b) {
                                self.consume(1);
                                return Key.of(.esc);
                            }
                            self.consume(self.pending_len);
                            continue;
                        }
                    },
                }
            } else if (!self.waitReadable(timeout_ms)) {
                return null;
            }
            if (self.pending_len == self.pending.len) self.pending_len = 0;
            const n = c.read(self.fd, self.pending[self.pending_len..].ptr, self.pending.len - self.pending_len);
            if (n < 0) {
                if (std.posix.errno(n) == .INTR) continue;
                return error.ReadFailed;
            }
            if (n == 0) return error.EndOfInput;
            self.pending_len += @intCast(n);
        }
    }

    fn consume(self: *Terminal, n: usize) void {
        std.mem.copyForwards(u8, self.pending[0 .. self.pending_len - n], self.pending[n..self.pending_len]);
        self.pending_len -= n;
    }
};

/// the running terminal, so a panic can restore it
var active: ?*Terminal = null;

/// leaves the terminal usable when we crash
pub fn restoreOnPanic() void {
    const t = active orelse return;
    const reset = switch (t.display) {
        .fullscreen => "\x1b[0m\x1b[?1049l\x1b[?25h",
        .@"inline" => "\x1b[0m\r\n\x1b[?25h",
    };
    sys.writeAll(t.fd, reset) catch {};
    t.restore();
}
