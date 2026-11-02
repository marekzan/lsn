const std = @import("std");

const Action = @import("action.zig").Action;
const Key = @import("term/key.zig").Key;

pub const Binding = struct {
    keys: []const Key,
    action: Action,
    desc: []const u8,
};

fn k(c: u21) Key {
    return Key.char(c);
}

fn ctrl(c: u21) Key {
    return Key.ctrlChar(c);
}

fn code(comptime c: @import("term/key.zig").Code) Key {
    return Key.of(c);
}

fn b(keys: []const Key, action: Action, desc: []const u8) Binding {
    return .{ .keys = keys, .action = action, .desc = desc };
}

/// the single source of truth for key handling and the help screen
pub const normal = [_]Binding{
    b(&.{ k('j'), code(.down) }, .down, "down"),
    b(&.{ k('k'), code(.up) }, .up, "up"),
    b(&.{ k('l'), code(.right) }, .expand, "expand / into dir"),
    b(&.{ k('h'), code(.left) }, .collapse, "collapse / to parent"),
    b(&.{code(.enter)}, .open, "toggle dir / pick file"),
    b(&.{ k('g'), code(.home) }, .top, "top"),
    b(&.{ k('G'), code(.end) }, .bottom, "bottom"),
    b(&.{ ctrl('d'), code(.page_down) }, .half_page_down, "half page down"),
    b(&.{ ctrl('u'), code(.page_up) }, .half_page_up, "half page up"),
    b(&.{k('z')}, .collapse_all, "collapse all"),
    b(&.{ k('H'), code(.backspace) }, .root_up, "root: parent dir"),
    b(&.{k('L')}, .root_down, "root: dir under cursor"),
    b(&.{k('~')}, .root_home, "root: home"),
    b(&.{k(' ')}, .toggle_mark, "mark / unmark"),
    b(&.{k('*')}, .invert_marks, "invert marks (visible)"),
    b(&.{k('U')}, .clear_marks, "clear marks"),
    b(&.{k('y')}, .yank, "copy to clipboard"),
    b(&.{k('x')}, .cut, "cut to clipboard"),
    b(&.{k('p')}, .paste, "paste into dir"),
    b(&.{ k('d'), code(.delete) }, .delete, "delete"),
    b(&.{k('r')}, .rename, "rename"),
    b(&.{k('a')}, .create, "new file (dir/ for dir)"),
    b(&.{k('c')}, .chmod, "chmod"),
    b(&.{k('/')}, .search, "search"),
    b(&.{k('e')}, .{ .jump = .root }, "jump from top level"),
    b(&.{k('E')}, .{ .jump = .here }, "jump from current level"),
    b(&.{k('f')}, .filter_menu, "filter menu"),
    b(&.{k('s')}, .sort_menu, "sort menu"),
    b(&.{k('.')}, .toggle_hidden, "toggle hidden"),
    b(&.{k('i')}, .toggle_meta, "toggle metadata"),
    b(&.{ k('R'), ctrl('r') }, .refresh, "reload from disk"),
    b(&.{k('t')}, .new_tab, "new tab at dir"),
    b(&.{ctrl('w')}, .close_tab, "close tab"),
    b(&.{ code(.tab), k(']') }, .next_tab, "next tab"),
    b(&.{ code(.backtab), k('[') }, .prev_tab, "previous tab"),
    b(&.{k('1')}, .{ .go_to_tab = 0 }, "tab 1 (… 9)"),
    b(&.{k('2')}, .{ .go_to_tab = 1 }, ""),
    b(&.{k('3')}, .{ .go_to_tab = 2 }, ""),
    b(&.{k('4')}, .{ .go_to_tab = 3 }, ""),
    b(&.{k('5')}, .{ .go_to_tab = 4 }, ""),
    b(&.{k('6')}, .{ .go_to_tab = 5 }, ""),
    b(&.{k('7')}, .{ .go_to_tab = 6 }, ""),
    b(&.{k('8')}, .{ .go_to_tab = 7 }, ""),
    b(&.{k('9')}, .{ .go_to_tab = 8 }, ""),
    b(&.{k('b')}, .toggle_pin, "pin / unpin dir"),
    b(&.{k('B')}, .open_pins, "pins tab"),
    b(&.{k('o')}, .pick, "print marked/cursor, quit"),
    b(&.{k('O')}, .pick_dir, "print dir, quit (cd)"),
    b(&.{k('?')}, .help, "help"),
    b(&.{code(.esc)}, .escape, "clear search/marks, quit"),
    b(&.{ k('q'), ctrl('c') }, .quit, "quit"),
};

/// second key after `f`
pub const filter_menu = [_]Binding{
    b(&.{k('d')}, .toggle_dirs, "dirs"),
    b(&.{k('f')}, .toggle_files, "files"),
    b(&.{k('l')}, .toggle_symlinks, "symlinks"),
    b(&.{k('.')}, .toggle_hidden, "hidden"),
    b(&.{k('m')}, .toggle_only_marked, "only marked"),
    b(&.{k('r')}, .reset_filter, "reset"),
};

/// second key after `s`
pub const sort_menu = [_]Binding{
    b(&.{k('n')}, .{ .sort_by = .name }, "name"),
    b(&.{k('s')}, .{ .sort_by = .size }, "size"),
    b(&.{k('m')}, .{ .sort_by = .modified }, "modified"),
    b(&.{k('e')}, .{ .sort_by = .extension }, "extension"),
    b(&.{k('d')}, .toggle_dirs_first, "dirs first"),
    b(&.{k('r')}, .toggle_reverse, "reverse"),
};

pub fn lookup(table: []const Binding, key: Key) ?Action {
    for (table) |binding| {
        for (binding.keys) |bk| if (bk.eql(key)) return binding.action;
    }
    return null;
}

test "lookup and no duplicate keys" {
    try std.testing.expectEqual(Action.bottom, lookup(&normal, Key.char('G')).?);
    try std.testing.expectEqual(Action.half_page_down, lookup(&normal, Key.ctrlChar('d')).?);
    try std.testing.expect(lookup(&normal, Key.char('Q')) == null);

    for ([_][]const Binding{ &normal, &filter_menu, &sort_menu }) |table| {
        var keys: [128]Key = undefined;
        var n: usize = 0;
        for (table) |binding| for (binding.keys) |key| {
            keys[n] = key;
            n += 1;
        };
        for (keys[0..n], 0..) |a, i| for (keys[i + 1 .. n]) |c| {
            if (a.eql(c)) {
                std.debug.print("bound twice: {any}\n", .{a});
                return error.DuplicateKey;
            }
        };
    }
}
