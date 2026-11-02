const std = @import("std");
const build_options = @import("build_options");

const App = @import("app.zig").App;
const Pins = @import("pins.zig").Pins;
const sys = @import("sys.zig");
const terminal = @import("term/terminal.zig");
const ui = @import("ui.zig");

const usage =
    \\Explore files in the terminal via a tree view.
    \\
    \\lsn runs inline below your prompt by default. Picked paths (enter on a
    \\file, o, O) are printed to stdout, so it can be used in scripts:
    \\
    \\    cd "$(lsn ~/projects)"      # press O on a directory
    \\    vim $(lsn)                  # mark files with space, press o
    \\
    \\Usage: lsn [OPTIONS] [DIR]...
    \\
    \\Arguments:
    \\  [DIR]...  Directories to open, one tab each [default: .]
    \\
    \\Options:
    \\  -f, --fullscreen      Use the whole terminal (alternate screen) instead of running inline
    \\  -H, --height <LINES>  Height of the inline viewport in lines, capped to the terminal height [default: 20]
    \\  -h, --help            Print help
    \\  -V, --version         Print version
    \\
;

/// restores the terminal before the default panic output
pub const panic = std.debug.FullPanic(onPanic);

fn onPanic(msg: []const u8, first_trace_addr: ?usize) noreturn {
    terminal.restoreOnPanic();
    std.debug.defaultPanic(msg, first_trace_addr);
}

const Args = struct {
    dirs: []const []const u8,
    display: terminal.Display,
};

fn parseArgs(arena: std.mem.Allocator, argv: []const [:0]const u8) !?Args {
    var dirs: std.ArrayList([]const u8) = .empty;
    var fullscreen = false;
    var height: u16 = 20;
    var i: usize = 1;
    while (i < argv.len) : (i += 1) {
        const arg: []const u8 = argv[i];
        if (std.mem.eql(u8, arg, "-h") or std.mem.eql(u8, arg, "--help")) {
            try sys.writeAll(1, usage);
            return null;
        } else if (std.mem.eql(u8, arg, "-V") or std.mem.eql(u8, arg, "--version")) {
            try sys.writeAll(1, "lsn " ++ build_options.version ++ "\n");
            return null;
        } else if (std.mem.eql(u8, arg, "-f") or std.mem.eql(u8, arg, "--fullscreen")) {
            fullscreen = true;
        } else if (std.mem.eql(u8, arg, "-H") or std.mem.eql(u8, arg, "--height")) {
            i += 1;
            if (i >= argv.len) return fail("missing value for {s}", .{arg});
            height = std.fmt.parseInt(u16, argv[i], 10) catch return fail("invalid height: {s}", .{argv[i]});
        } else if (std.mem.startsWith(u8, arg, "--height=")) {
            height = std.fmt.parseInt(u16, arg["--height=".len..], 10) catch return fail("invalid height: {s}", .{arg});
        } else if (arg.len > 1 and arg[0] == '-') {
            return fail("unknown option: {s}", .{arg});
        } else {
            try dirs.append(arena, arg);
        }
    }
    if (dirs.items.len == 0) try dirs.append(arena, ".");
    return .{
        .dirs = dirs.items,
        .display = if (fullscreen) .fullscreen else .{ .@"inline" = height },
    };
}

fn fail(comptime fmt: []const u8, args: anytype) error{InvalidArgs} {
    var buf: [512]u8 = undefined;
    const msg = std.fmt.bufPrint(&buf, "error: " ++ fmt ++ "\n\nFor more information, try '--help'.\n", args) catch "error\n";
    sys.writeAll(2, msg) catch {};
    return error.InvalidArgs;
}

pub fn main(init: std.process.Init) !u8 {
    const gpa = init.gpa;
    const arena = init.arena.allocator();
    const argv = try init.minimal.args.toSlice(arena);
    const args = (parseArgs(arena, argv) catch return 2) orelse return 0;

    var app = App.init(gpa, args.dirs, try Pins.load(gpa)) catch |err| {
        var buf: [512]u8 = undefined;
        sys.writeAll(2, std.fmt.bufPrint(&buf, "error: could not open directory: {s}\n", .{sys.describe(err)}) catch "error\n") catch {};
        return 1;
    };
    defer app.deinit();

    const term = terminal.Terminal.start(gpa, args.display) catch |err| {
        sys.writeAll(2, if (err == error.NoTty) "error: lsn needs a terminal\n" else "error: could not set up the terminal\n") catch {};
        return 1;
    };
    const result = run(term, &app);
    term.stop();
    try result;

    // only print once the ui is gone
    for (app.output.items) |path| {
        try sys.writeAll(1, path);
        try sys.writeAll(1, "\n");
    }
    return 0;
}

fn drawFrame(app: *App, frame: *terminal.Frame) anyerror!void {
    return ui.draw(app, frame);
}

/// the event loop: draw, wait for a key, update. the ui is only redrawn when
/// something happened (a key or a resize).
fn run(term: *terminal.Terminal, app: *App) !void {
    while (!app.should_quit) {
        try term.draw(app, drawFrame);
        while (true) {
            if (try term.readKey(250)) |key| {
                try app.handleKey(key);
                break;
            }
            // there is no signal handler, the size is checked while idle
            if (try term.checkResize()) break;
        }
    }
}

test {
    _ = @import("sys.zig");
    _ = @import("arena.zig");
    _ = @import("pathset.zig");
    _ = @import("fs/node.zig");
    _ = @import("fs/tree.zig");
    _ = @import("fs/ops.zig");
    _ = @import("view.zig");
    _ = @import("input.zig");
    _ = @import("keymap.zig");
    _ = @import("tab.zig");
    _ = @import("jump.zig");
    _ = @import("pins.zig");
    _ = @import("app.zig");
    _ = @import("term/parse.zig");
    _ = @import("term/width.zig");
    _ = @import("term/screen.zig");
    _ = @import("term/terminal.zig");
    _ = @import("ui.zig");
}

test "args" {
    var arena_state = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena_state.deinit();
    const arena = arena_state.allocator();
    const a = (try parseArgs(arena, &.{ "lsn", "-f", "src", "docs" })).?;
    try std.testing.expect(a.display == .fullscreen);
    try std.testing.expectEqual(2, a.dirs.len);
    const b = (try parseArgs(arena, &.{ "lsn", "-H", "12" })).?;
    try std.testing.expectEqual(12, b.display.@"inline");
    try std.testing.expectEqualStrings(".", b.dirs[0]);
}
