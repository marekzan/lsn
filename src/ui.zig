//! renders the app state into a frame. never changes the model, only the
//! scroll offset and the page height of the app.

const std = @import("std");

const app_mod = @import("app.zig");
const App = app_mod.App;
const Mode = app_mod.Mode;
const Node = @import("fs/node.zig").Node;
const keymap = @import("keymap.zig");
const sys = @import("sys.zig");
const Frame = @import("term/terminal.zig").Frame;
const screen = @import("term/screen.zig");
const Color = screen.Color;
const Style = screen.Style;
const width = @import("term/width.zig");
const view = @import("view.zig");
const ViewItem = view.ViewItem;

/// rows kept visible above and below the cursor while scrolling
const scroll_off = 3;

const meta_widths = [_]u16{ 10, 8, 8, 6, 16 };

pub fn draw(app: *App, frame: *Frame) !void {
    try app.prepare();
    const h = frame.height();
    if (h < 3) return;
    drawTabs(app, frame, 0);
    try drawTree(app, frame, 1, h - 2);
    try drawStatus(app, frame, h - 1);
    if (app.mode == .help) drawHelp(frame, app.mode.help);
}

fn drawTabs(app: *App, frame: *Frame, y: u16) void {
    const buf = frame.buf;
    var x: u16 = 0;
    var label: [128]u8 = undefined;
    for (app.tabs.items, 0..) |*t, i| {
        const text = std.fmt.bufPrint(&label, " {d} {s} ", .{ i + 1, t.title() }) catch continue;
        const style: Style = if (i == app.active) .{ .reverse = true, .bold = true } else .{ .dim = true };
        x += buf.print(x, y, buf.width -| x, text, style);
    }
    x += 2;

    const t = app.tabConst();
    var tmp: [4096]u8 = undefined;
    const location: []const u8 = switch (app.mode) {
        .jump => |j| if (j.dir) |d| tilde(t.tree.pathOf(d) orelse "", &tmp) else "pinned",
        else => switch (t.kind) {
            .dir => tilde(t.rootPath() orelse "", &tmp),
            .pins => std.fmt.bufPrint(&tmp, "{d} pinned", .{t.tree.roots.items.len}) catch "",
        },
    };
    _ = buf.print(x, y, buf.width -| x, location, .{ .fg = Color.blue });
}

fn drawTree(app: *App, frame: *Frame, y0: u16, height: u16) !void {
    app.page = height;
    switch (app.mode) {
        .jump => |*j| {
            // the jump shows one flat level, the real view stays untouched
            var items: std.ArrayList(ViewItem) = .empty;
            defer items.deinit(app.gpa);
            for (j.matches.items, 0..) |h, i| {
                try items.append(app.gpa, .{ .handle = h, .depth = 0, .guides = 0, .is_last = i + 1 == j.matches.items.len });
            }
            const offset = scroll(j.cursor, 0, items.items.len, height);
            const empty = if (j.input.text().len == 0) "empty" else "no match";
            drawItems(app, frame, y0, height, items.items, j.cursor, offset, j.input.text(), empty);
        },
        else => {
            const t = app.tab();
            t.offset = scroll(t.cursor, t.offset, t.items.items.len, height);
            const empty = switch (t.kind) {
                .pins => "no pinned directories – pin one with b",
                .dir => if (t.filter.query_len > 0) "nothing matches" else "empty",
            };
            drawItems(app, frame, y0, height, t.items.items, t.cursor, t.offset, null, empty);
        },
    }
}

/// new scroll offset that keeps the cursor inside the window, with some
/// context around it
fn scroll(cursor: usize, offset: usize, len: usize, height: usize) usize {
    const so = @min(scroll_off, (height -| 1) / 2);
    var o = offset;
    if (cursor < o + so) {
        o = cursor -| so;
    } else if (cursor + so >= o + height) {
        o = cursor + so + 1 - height;
    }
    return @min(o, len -| height);
}

fn drawItems(app: *App, frame: *Frame, y0: u16, height: u16, items: []const ViewItem, cursor: usize, offset: usize, highlight: ?[]const u8, empty: []const u8) void {
    const buf = frame.buf;
    if (items.len == 0) {
        _ = buf.print(0, y0, buf.width, empty, .{ .dim = true, .italic = true });
        return;
    }
    const meta_total = blk: {
        var sum: u16 = 0;
        for (meta_widths) |w| sum += w + 1;
        break :blk sum;
    };
    const show_meta = app.show_meta and buf.width > meta_total + 20;
    const name_width = buf.width -| 3 -| (if (show_meta) meta_total else 0);

    const t = app.tabConst();
    for (items[offset..@min(offset + height, items.len)], 0..) |item, i| {
        const y = y0 + @as(u16, @intCast(i));
        const node = t.tree.get(item.handle) orelse continue;
        if (offset + i == cursor) buf.paint(0, y, buf.width, .{ .bg = .{ .index = 237 } });
        drawRow(app, frame, y, item, node, name_width, show_meta, highlight);
    }
}

fn drawRow(app: *App, frame: *Frame, y: u16, item: ViewItem, node: *const Node, name_width: u16, show_meta: bool, highlight: ?[]const u8) void {
    const buf = frame.buf;
    const marked = app.marked.contains(node.path);
    const clip = if (app.clipboard) |*cb| (if (cb.paths.contains(node.path)) cb.kind else null) else null;

    if (marked) {
        _ = buf.print(0, y, 2, "●", .{ .fg = Color.yellow, .bold = true });
    } else if (clip) |kind| switch (kind) {
        .copy => _ = buf.print(0, y, 2, "+", .{ .fg = Color.green }),
        .move => _ = buf.print(0, y, 2, "✂", .{ .fg = Color.red }),
    };

    var x: u16 = 3;
    const end = 3 + name_width;
    const guide_style: Style = .{ .fg = Color.gray };
    for (0..item.depth) |level| {
        x += buf.print(x, y, end -| x, if (item.hasGuide(level)) "│ " else "  ", guide_style);
    }
    x += buf.print(x, y, end -| x, if (item.is_last) "└─" else "├─", guide_style);

    var style = nameStyle(node);
    if (marked) style.fg = Color.yellow;
    if (clip == .move) {
        style.dim = true;
        style.italic = true;
    }

    if (highlight) |q| if (view.matchRange(q, node.name)) |r| if (r[1] > r[0]) {
        var hl = style;
        hl.fg = Color.yellow;
        hl.underline = true;
        x += buf.print(x, y, end -| x, node.name[0..r[0]], style);
        x += buf.print(x, y, end -| x, node.name[r[0]..r[1]], hl);
        x += buf.print(x, y, end -| x, node.name[r[1]..], style);
    } else {
        x += buf.print(x, y, end -| x, node.name, style);
    } else {
        x += buf.print(x, y, end -| x, node.name, style);
    } else {
        x += buf.print(x, y, end -| x, node.name, style);
    }

    switch (node.kind) {
        .dir => x += buf.print(x, y, end -| x, "/", style),
        .symlink => |s| {
            x += buf.print(x, y, end -| x, " → ", guide_style);
            x += buf.print(x, y, end -| x, s.target, guide_style);
        },
        .file => {},
    }
    if (node.isDir() and !node.expanded and node.children != null and node.children.?.items.len == 0) {
        x += buf.print(x, y, end -| x, " (empty)", .{ .dim = true });
    }
    if (node.err) |err| {
        x += buf.print(x, y, end -| x, "  ", .{});
        x += buf.print(x, y, end -| x, sys.describe(err), .{ .fg = Color.red });
    }

    if (!show_meta) return;
    var mx = end + 1;
    const dim: Style = .{ .fg = Color.gray };
    var perm: [10]u8 = undefined;
    _ = buf.print(mx, y, meta_widths[0], permissions(node, &perm), dim);
    mx += meta_widths[0] + 1;
    _ = buf.print(mx, y, meta_widths[1], app.names.user(app.gpa, node.meta.uid), dim);
    mx += meta_widths[1] + 1;
    _ = buf.print(mx, y, meta_widths[2], app.names.group(app.gpa, node.meta.gid), dim);
    mx += meta_widths[2] + 1;
    var size_buf: [16]u8 = undefined;
    const size = if (node.isDir()) "-" else humanSize(node.meta.size, &size_buf);
    // right aligned
    _ = buf.print(mx + meta_widths[3] -| @as(u16, @intCast(size.len)), y, meta_widths[3], size, .{});
    mx += meta_widths[3] + 1;
    var time_buf: [32]u8 = undefined;
    const time = if (node.meta.mtime) |t| sys.formatTime(t, &time_buf) else "";
    _ = buf.print(mx, y, meta_widths[4], time, dim);
}

fn nameStyle(node: *const Node) Style {
    var style: Style = switch (node.kind) {
        .dir => .{ .fg = Color.blue, .bold = true },
        .symlink => .{ .fg = Color.cyan },
        .file => if (node.meta.isExecutable()) .{ .fg = Color.green } else .{},
    };
    if (node.isHidden()) style.dim = true;
    return style;
}

fn drawStatus(app: *App, frame: *Frame, y: u16) !void {
    const buf = frame.buf;
    const t = app.tabConst();

    // the right side summarizes global and tab state
    var right_buf: [512]u8 = undefined;
    var w: std.Io.Writer = .fixed(&right_buf);
    const Span = struct { start: usize, end: usize, style: Style };
    var spans: [6]Span = undefined;
    var n: usize = 0;
    const sep = "  ";
    if (app.marked.count() > 0) {
        const s = w.end;
        w.print("● {d}", .{app.marked.count()}) catch {};
        spans[n] = .{ .start = s, .end = w.end, .style = .{ .fg = Color.yellow } };
        n += 1;
    }
    if (app.clipboard) |*cb| {
        if (n > 0) w.writeAll(sep) catch {};
        const s = w.end;
        w.print("{s} {d}", .{ if (cb.kind == .copy) "+" else "✂", cb.paths.count() }) catch {};
        spans[n] = .{ .start = s, .end = w.end, .style = .{ .fg = if (cb.kind == .copy) Color.green else Color.red } };
        n += 1;
    }
    var filter_buf: [300]u8 = undefined;
    if (t.filter.describe(&filter_buf)) |f| {
        if (n > 0) w.writeAll(sep) catch {};
        const s = w.end;
        w.print("filter: {s}", .{f}) catch {};
        spans[n] = .{ .start = s, .end = w.end, .style = .{ .fg = Color.magenta } };
        n += 1;
    }
    if (n > 0) w.writeAll(sep) catch {};
    var s = w.end;
    w.print("sort: {s}", .{t.sort.describe()}) catch {};
    spans[n] = .{ .start = s, .end = w.end, .style = .{ .dim = true } };
    n += 1;
    w.writeAll(sep) catch {};
    s = w.end;
    switch (app.mode) {
        .jump => |j| w.print("{d} match{s}", .{ j.matches.items.len, if (j.matches.items.len == 1) "" else "es" }) catch {},
        else => w.print("{d}/{d}", .{ @min(t.cursor + 1, t.items.items.len), t.items.items.len }) catch {},
    }
    spans[n] = .{ .start = s, .end = w.end, .style = .{ .dim = true } };
    n += 1;

    const right = w.buffered();
    const right_width: u16 = @intCast(@min(width.str(right), buf.width));
    const rx = buf.width - right_width;
    var x = rx;
    var last: usize = 0;
    for (spans[0..n]) |span| {
        x += buf.print(x, y, buf.width -| x, right[last..span.start], .{});
        x += buf.print(x, y, buf.width -| x, right[span.start..span.end], span.style);
        last = span.end;
    }

    // the left side shows what the user is doing right now
    const max = rx -| 1;
    switch (app.mode) {
        .prompt => |*p| {
            const label = p.kind.label();
            var lx = buf.print(0, y, max, label, .{ .bold = true });
            lx += buf.print(lx, y, max -| lx, p.input.text(), .{});
            const cx = width.str(label) + width.str(p.input.text()[0..p.input.cursor]);
            frame.cursor = .{ @intCast(@min(cx, buf.width -| 1)), y };
        },
        .jump => |*j| {
            const label = "jump ▸ ";
            var lx = buf.print(0, y, max, label, .{ .fg = Color.yellow, .bold = true });
            lx += buf.print(lx, y, max -| lx, j.input.text(), .{});
            frame.cursor = .{ @intCast(@min(lx, buf.width -| 1)), y };
            _ = buf.print(lx, y, max -| lx, "   enter open · bksp up · esc stay here", .{ .dim = true });
        },
        .confirm_delete => |paths| {
            var tmp: [512]u8 = undefined;
            var tw: std.Io.Writer = .fixed(&tmp);
            tw.writeAll("delete ") catch {};
            for (paths[0..@min(paths.len, 3)], 0..) |p, i| {
                if (i > 0) tw.writeAll(", ") catch {};
                tw.writeAll(sys.baseName(p)) catch {};
            }
            if (paths.len > 3) tw.print(" +{d}", .{paths.len - 3}) catch {};
            tw.writeAll("? [y/N]") catch {};
            _ = buf.print(0, y, max, tw.buffered(), .{ .fg = Color.red, .bold = true });
        },
        .menu => |menu| drawMenu(app, frame, y, max, menu),
        .normal, .help => if (app.status) |st| {
            _ = buf.print(0, y, max, st.text, if (st.is_error) .{ .fg = Color.red } else .{});
        } else {
            _ = buf.print(0, y, max, "? help", .{ .dim = true });
        },
    }
}

fn drawMenu(app: *App, frame: *Frame, y: u16, max: u16, menu: app_mod.Menu) void {
    const buf = frame.buf;
    const t = app.tabConst();
    const table: []const keymap.Binding, const title = switch (menu) {
        .filter => .{ &keymap.filter_menu, "filter: " },
        .sort => .{ &keymap.sort_menu, "sort: " },
    };
    var x = buf.print(0, y, max, title, .{ .bold = true });
    for (table) |binding| {
        const active = switch (binding.action) {
            .toggle_dirs => t.filter.show_dirs,
            .toggle_files => t.filter.show_files,
            .toggle_symlinks => t.filter.show_symlinks,
            .toggle_hidden => t.filter.show_hidden,
            .toggle_only_marked => t.filter.only_marked,
            .sort_by => |by| t.sort.by == by,
            .toggle_dirs_first => t.sort.dirs_first,
            .toggle_reverse => t.sort.reverse,
            else => false,
        };
        var key_buf: [16]u8 = undefined;
        var label: [24]u8 = undefined;
        const k = std.fmt.bufPrint(&label, "[{s}]", .{binding.keys[0].label(&key_buf)}) catch "";
        x += buf.print(x, y, max -| x, k, .{ .fg = Color.yellow });
        x += buf.print(x, y, max -| x, binding.desc, if (active) .{ .fg = Color.green, .bold = true } else .{ .dim = true });
        x += buf.print(x, y, max -| x, " ", .{});
    }
}

const HelpEntry = struct { keys: [32]u8, keys_len: usize, desc: []const u8 };

fn drawHelp(frame: *Frame, scroll_by: u16) void {
    const buf = frame.buf;
    var entries: [96]HelpEntry = undefined;
    var n: usize = 0;
    for (keymap.normal) |binding| {
        if (binding.desc.len == 0) continue;
        var e: HelpEntry = .{ .keys = undefined, .keys_len = 0, .desc = binding.desc };
        var w: std.Io.Writer = .fixed(&e.keys);
        for (binding.keys, 0..) |k, i| {
            var kb: [16]u8 = undefined;
            if (i > 0) w.writeByte(' ') catch {};
            w.writeAll(k.label(&kb)) catch {};
        }
        e.keys_len = w.end;
        entries[n] = e;
        n += 1;
    }
    inline for (.{ .{ "f", &keymap.filter_menu }, .{ "s", &keymap.sort_menu } }) |menu| {
        for (menu[1]) |binding| {
            var e: HelpEntry = .{ .keys = undefined, .keys_len = 0, .desc = binding.desc };
            var kb: [16]u8 = undefined;
            const text = std.fmt.bufPrint(&e.keys, "{s} {s}", .{ menu[0], binding.keys[0].label(&kb) }) catch "";
            e.keys_len = text.len;
            entries[n] = e;
            n += 1;
        }
    }

    var key_width: usize = 0;
    var desc_width: usize = 0;
    for (entries[0..n]) |e| {
        key_width = @max(key_width, width.str(e.keys[0..e.keys_len]));
        desc_width = @max(desc_width, e.desc.len);
    }
    const col_width = key_width + 2 + desc_width + 3;
    // as many columns as fit, whatever does not fit in height can be scrolled
    const cols = @max(buf.width / col_width, 1);
    const rows = (n + cols - 1) / cols;
    const total_lines = rows + 1;
    const top = @min(scroll_by, total_lines -| buf.height);

    buf.clear();
    var line: usize = 0;
    while (line < buf.height) : (line += 1) {
        const l = line + top;
        const y: u16 = @intCast(line);
        if (l == 0) {
            _ = buf.print(1, y, buf.width, "keys  (j/k scroll, any other key closes)", .{ .bold = true });
            continue;
        }
        const r = l - 1;
        if (r >= rows) break;
        var c: usize = 0;
        while (c < cols) : (c += 1) {
            const i = c * rows + r;
            if (i >= n) break;
            const e = entries[i];
            const keys = e.keys[0..e.keys_len];
            const x: u16 = @intCast(1 + c * col_width);
            const pad: u16 = @intCast(key_width - width.str(keys));
            _ = buf.print(x + pad, y, @intCast(key_width), keys, .{ .fg = Color.yellow });
            _ = buf.print(@intCast(x + key_width + 2), y, @intCast(desc_width), e.desc, .{});
        }
    }
}

fn permissions(node: *const Node, out: *[10]u8) []const u8 {
    out[0] = switch (node.kind) {
        .dir => 'd',
        .symlink => 'l',
        .file => '-',
    };
    const mode = node.meta.mode;
    for ([_]u5{ 6, 3, 0 }, 0..) |shift, i| {
        const bits = (mode >> shift) & 0o7;
        out[1 + i * 3] = if (bits & 4 != 0) 'r' else '-';
        out[2 + i * 3] = if (bits & 2 != 0) 'w' else '-';
        out[3 + i * 3] = if (bits & 1 != 0) 'x' else '-';
    }
    return out;
}

fn humanSize(size: u64, buf: *[16]u8) []const u8 {
    const units = [_][]const u8{ "B", "K", "M", "G", "T" };
    var value: f64 = @floatFromInt(size);
    var unit: usize = 0;
    while (value >= 1024 and unit < units.len - 1) {
        value /= 1024;
        unit += 1;
    }
    if (unit == 0) return std.fmt.bufPrint(buf, "{d}B", .{size}) catch "";
    if (value < 10) return std.fmt.bufPrint(buf, "{d:.1}{s}", .{ value, units[unit] }) catch "";
    return std.fmt.bufPrint(buf, "{d:.0}{s}", .{ value, units[unit] }) catch "";
}

fn tilde(path: []const u8, buf: []u8) []const u8 {
    const home = sys.getenv("HOME") orelse return path;
    if (home.len == 0 or !sys.isWithin(path, home)) return path;
    return std.fmt.bufPrint(buf, "~{s}", .{path[home.len..]}) catch path;
}

const testing = std.testing;

test "sizes" {
    var buf: [16]u8 = undefined;
    try testing.expectEqualStrings("512B", humanSize(512, &buf));
    try testing.expectEqualStrings("1.5K", humanSize(1536, &buf));
    try testing.expectEqualStrings("50M", humanSize(50 * 1024 * 1024, &buf));
}

test "renders tree and status" {
    const gpa = testing.allocator;
    var dir = try @import("testutil.zig").TmpDir.init(gpa);
    defer dir.deinit();
    try dir.mkdir("src");
    try dir.write("src/main.rs", "");
    try dir.write("README.md", "");
    var app = try App.init(gpa, &.{dir.root}, @import("pins.zig").Pins.inMemory(gpa));
    defer app.deinit();
    app.show_meta = false;
    try app.apply(.expand);

    var buf = try screen.Buffer.init(gpa, 60, 6);
    defer buf.deinit(gpa);
    var frame: Frame = .{ .buf = &buf };
    try draw(&app, &frame);

    var lines: [6][]const u8 = undefined;
    var storage: [6][256]u8 = undefined;
    for (0..6) |y| {
        var len: usize = 0;
        for (0..60) |x| {
            const cell = buf.cell(@intCast(x), @intCast(y));
            if (cell.continuation) continue;
            @memcpy(storage[y][len..][0..cell.len], cell.text());
            len += cell.len;
        }
        lines[y] = std.mem.trimEnd(u8, storage[y][0..len], " ");
    }
    try testing.expect(std.mem.startsWith(u8, lines[0], " 1 lsn-"));
    try testing.expectEqualStrings("   ├─src/", lines[1]);
    try testing.expectEqualStrings("   │ └─main.rs", lines[2]);
    try testing.expectEqualStrings("   └─README.md", lines[3]);
    try testing.expect(std.mem.startsWith(u8, lines[5], "? help"));
    try testing.expect(std.mem.endsWith(u8, lines[5], "sort: name  1/3"));
}
