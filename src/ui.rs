use std::time::SystemTime;

use chrono::{DateTime, Local};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Cell, Clear, Paragraph, Row, Table},
};

use crate::{
    action::Action,
    app::{App, Menu, Mode},
    fs::{
        node::{Node, NodeKind},
        ops::TransferKind,
        owner,
    },
    keymap::{self, Binding},
    tab::TabKind,
    view::ViewItem,
};

/// rows kept visible above and below the cursor while scrolling
const SCROLL_OFF: usize = 3;

pub fn draw(frame: &mut Frame, app: &mut App) {
    app.prepare();
    let [tabs, tree, status] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    draw_tabs(frame, app, tabs);
    draw_tree(frame, app, tree);
    draw_status(frame, app, status);

    if let Mode::Help(scroll) = app.mode {
        draw_help(frame, frame.area(), scroll);
    }
}

fn draw_tabs(frame: &mut Frame, app: &App, area: Rect) {
    let mut spans = Vec::new();
    for (i, tab) in app.tabs.iter().enumerate() {
        let label = format!(" {} {} ", i + 1, tab.title());
        spans.push(if i == app.active {
            Span::styled(label, Style::new().reversed().bold())
        } else {
            Span::styled(label, Style::new().dim())
        });
    }
    let tab = app.tab();
    let jump_dir = match &app.mode {
        Mode::Jump(jump) => Some(jump.dir.and_then(|d| tab.tree.path(d))),
        _ => None,
    };
    let location = match (jump_dir, tab.kind) {
        (Some(Some(path)), _) => tilde(&path.display().to_string()),
        (Some(None), _) => "pinned".to_string(),
        (None, TabKind::Dir) => tab
            .root_path()
            .map(|p| tilde(&p.display().to_string()))
            .unwrap_or_default(),
        (None, TabKind::Pins) => format!("{} pinned", tab.tree.roots().len()),
    };
    spans.push(Span::raw("  "));
    spans.push(Span::styled(location, Style::new().fg(Color::Blue)));
    frame.render_widget(Line::from(spans), area);
}

fn draw_tree(frame: &mut Frame, app: &mut App, area: Rect) {
    let height = area.height as usize;
    app.page = height;

    if let Mode::Jump(jump) = &app.mode {
        // the jump shows one flat level, the real view stays untouched
        let count = jump.matches.len();
        let items: Vec<ViewItem> = jump
            .matches
            .iter()
            .enumerate()
            .map(|(i, h)| ViewItem {
                handle: *h,
                depth: 0,
                guides: Vec::new(),
                is_last: i + 1 == count,
            })
            .collect();
        let offset = scroll(jump.cursor, 0, items.len(), height);
        let query = jump.input.text().to_string();
        let empty = if query.is_empty() {
            "empty"
        } else {
            "no match"
        };
        draw_items(
            frame,
            app,
            area,
            &items,
            jump.cursor,
            offset,
            Some(&query),
            empty,
        );
        return;
    }

    let tab = &mut app.tabs[app.active];
    tab.offset = scroll(tab.cursor, tab.offset, tab.items.len(), height);
    let empty = match tab.kind {
        TabKind::Pins => "no pinned directories – pin one with b",
        TabKind::Dir if !tab.filter.query.is_empty() => "nothing matches",
        TabKind::Dir => "empty",
    };
    let tab = &app.tabs[app.active];
    draw_items(
        frame, app, area, &tab.items, tab.cursor, tab.offset, None, empty,
    );
}

/// new scroll offset that keeps the cursor inside the window, with some
/// context around it
fn scroll(cursor: usize, offset: usize, len: usize, height: usize) -> usize {
    let so = SCROLL_OFF.min(height.saturating_sub(1) / 2);
    let mut offset = offset;
    if cursor < offset + so {
        offset = cursor.saturating_sub(so);
    } else if cursor + so >= offset + height {
        offset = cursor + so + 1 - height;
    }
    offset.min(len.saturating_sub(height))
}

#[allow(clippy::too_many_arguments)]
fn draw_items(
    frame: &mut Frame,
    app: &App,
    area: Rect,
    items: &[ViewItem],
    cursor: usize,
    offset: usize,
    highlight: Option<&str>,
    empty: &str,
) {
    if items.is_empty() {
        frame.render_widget(Paragraph::new(empty.to_string()).dim().italic(), area);
        return;
    }
    let tab = app.tab();
    let height = area.height as usize;
    let visible = &items[offset..(offset + height).min(items.len())];
    let rows: Vec<Row> = visible
        .iter()
        .enumerate()
        .filter_map(|(i, item)| {
            let node = tab.tree.get(item.handle)?;
            Some(row(app, item, node, offset + i == cursor, highlight))
        })
        .collect();

    let mut widths = vec![Constraint::Length(2), Constraint::Fill(1)];
    if app.show_meta {
        widths.extend([
            Constraint::Length(10),
            Constraint::Length(8),
            Constraint::Length(8),
            Constraint::Length(6),
            Constraint::Length(16),
        ]);
    }
    let table = Table::new(rows, widths).column_spacing(1);
    frame.render_widget(table, area);
}

fn row<'a>(
    app: &App,
    item: &ViewItem,
    node: &'a Node,
    selected: bool,
    highlight: Option<&str>,
) -> Row<'a> {
    let marked = app.marked.contains(&node.path);
    let clip = app
        .clipboard
        .as_ref()
        .filter(|c| c.paths.contains(&node.path))
        .map(|c| c.kind);

    let marker = match (marked, clip) {
        (true, _) => Span::styled("●", Style::new().fg(Color::Yellow).bold()),
        (_, Some(TransferKind::Copy)) => Span::styled("+", Style::new().fg(Color::Green)),
        (_, Some(TransferKind::Move)) => Span::styled("✂", Style::new().fg(Color::Red)),
        _ => Span::raw(" "),
    };

    let mut guides = String::new();
    for has_more in &item.guides {
        guides.push_str(if *has_more { "│ " } else { "  " });
    }
    guides.push_str(if item.is_last { "└─" } else { "├─" });

    let mut style = name_style(node);
    if marked {
        style = style.fg(Color::Yellow);
    }
    if clip == Some(TransferKind::Move) {
        style = style.add_modifier(Modifier::DIM | Modifier::ITALIC);
    }

    let mut name = vec![Span::styled(guides, Style::new().fg(Color::DarkGray))];
    match highlight.and_then(|q| crate::view::match_range(q, &node.name)) {
        Some((start, end)) if end > start => {
            let hl = style.fg(Color::Yellow).underlined();
            name.push(Span::styled(node.name[..start].to_string(), style));
            name.push(Span::styled(node.name[start..end].to_string(), hl));
            name.push(Span::styled(node.name[end..].to_string(), style));
        }
        _ => name.push(Span::styled(node.name.clone(), style)),
    }
    match &node.kind {
        NodeKind::Dir => name.push(Span::styled("/", style)),
        NodeKind::Symlink { target, .. } => name.push(Span::styled(
            format!(" → {}", target.display()),
            Style::new().fg(Color::DarkGray),
        )),
        NodeKind::File => {}
    }
    if node.is_dir() && !node.expanded && node.children.as_ref().is_some_and(|c| c.is_empty()) {
        name.push(Span::styled(" (empty)", Style::new().dim()));
    }
    if let Some(err) = &node.error {
        name.push(Span::styled(
            format!("  {err}"),
            Style::new().fg(Color::Red),
        ));
    }

    let mut cells = vec![Cell::from(marker), Cell::from(Line::from(name))];
    if app.show_meta {
        let meta = &node.meta;
        let dim = Style::new().fg(Color::DarkGray);
        cells.extend([
            Cell::from(Span::styled(permissions(node), dim)),
            Cell::from(Span::styled(owner::user_name(meta.uid), dim)),
            Cell::from(Span::styled(owner::group_name(meta.gid), dim)),
            Cell::from(
                Line::from(if node.is_dir() {
                    "-".into()
                } else {
                    human_size(meta.size)
                })
                .right_aligned(),
            ),
            Cell::from(Span::styled(format_time(meta.modified), dim)),
        ]);
    }

    let row = Row::new(cells);
    if selected {
        row.style(Style::new().bg(Color::Indexed(237)))
    } else {
        row
    }
}

fn name_style(node: &Node) -> Style {
    let style = match node.kind {
        NodeKind::Dir => Style::new().fg(Color::Blue).bold(),
        NodeKind::Symlink { .. } => Style::new().fg(Color::Cyan),
        NodeKind::File if node.meta.is_executable() => Style::new().fg(Color::Green),
        NodeKind::File => Style::new(),
    };
    if node.is_hidden() { style.dim() } else { style }
}

fn draw_status(frame: &mut Frame, app: &App, area: Rect) {
    // the left side shows what the user is doing right now
    let left: Line = match &app.mode {
        Mode::Prompt(prompt) => {
            let label = prompt.kind.label();
            let text = prompt.input.text();
            let cursor_x = area.x
                + (label.chars().count()
                    + text[..byte_offset(text, prompt.input.cursor())]
                        .chars()
                        .count()) as u16;
            frame.set_cursor_position((cursor_x.min(area.right().saturating_sub(1)), area.y));
            Line::from(vec![
                Span::styled(label, Style::new().bold()),
                Span::raw(text),
            ])
        }
        Mode::ConfirmDelete(paths) => {
            let names: Vec<String> = paths
                .iter()
                .take(3)
                .map(|p| crate::fs::node::display_name(p))
                .collect();
            let more = if paths.len() > 3 {
                format!(" +{}", paths.len() - 3)
            } else {
                String::new()
            };
            Line::from(Span::styled(
                format!("delete {}{more}? [y/N]", names.join(", ")),
                Style::new().fg(Color::Red).bold(),
            ))
        }
        Mode::Menu(menu) => menu_line(app, *menu),
        Mode::Jump(jump) => {
            let label = "jump ▸ ";
            let text = jump.input.text();
            let x = area.x + (label.chars().count() + text.chars().count()) as u16;
            frame.set_cursor_position((x.min(area.right().saturating_sub(1)), area.y));
            Line::from(vec![
                Span::styled(label, Style::new().fg(Color::Yellow).bold()),
                Span::raw(text.to_string()),
                Span::styled(
                    "   enter open · bksp up · esc stay here",
                    Style::new().dim(),
                ),
            ])
        }
        Mode::Normal | Mode::Help(_) => match &app.status {
            Some(status) if status.error => Line::from(Span::styled(
                status.text.clone(),
                Style::new().fg(Color::Red),
            )),
            Some(status) => Line::from(status.text.clone()),
            None => Line::from(Span::styled("? help", Style::new().dim())),
        },
    };

    // the right side summarizes global and tab state
    let tab = app.tab();
    let mut right = Vec::new();
    if !app.marked.is_empty() {
        right.push(Span::styled(
            format!("● {}", app.marked.len()),
            Style::new().fg(Color::Yellow),
        ));
    }
    if let Some(clip) = &app.clipboard {
        let (sign, color) = match clip.kind {
            TransferKind::Copy => ("+", Color::Green),
            TransferKind::Move => ("✂", Color::Red),
        };
        right.push(Span::styled(
            format!("{sign} {}", clip.paths.len()),
            Style::new().fg(color),
        ));
    }
    if let Some(filter) = tab.filter.describe() {
        right.push(Span::styled(
            format!("filter: {filter}"),
            Style::new().fg(Color::Magenta),
        ));
    }
    right.push(Span::styled(
        format!("sort: {}", tab.sort.describe()),
        Style::new().dim(),
    ));
    let position = match &app.mode {
        Mode::Jump(jump) => match jump.matches.len() {
            1 => "1 match".to_string(),
            n => format!("{n} matches"),
        },
        _ => format!(
            "{}/{}",
            (tab.cursor + 1).min(tab.items.len()),
            tab.items.len()
        ),
    };
    right.push(Span::styled(position, Style::new().dim()));

    let mut spans = Vec::new();
    for (i, span) in right.into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(span);
    }
    let right = Line::from(spans).right_aligned();
    let right_width = right.width() as u16;

    let [l, r] = Layout::horizontal([Constraint::Min(0), Constraint::Length(right_width)])
        .spacing(1)
        .areas(area);
    frame.render_widget(left, l);
    frame.render_widget(right, r);
}

fn menu_line(app: &App, menu: Menu) -> Line<'static> {
    let tab = app.tab();
    let (title, table) = match menu {
        Menu::Filter => ("filter", keymap::FILTER_MENU),
        Menu::Sort => ("sort", keymap::SORT_MENU),
    };
    let mut spans = vec![Span::styled(format!("{title}: "), Style::new().bold())];
    for binding in table {
        let active = match binding.action {
            Action::ToggleDirs => tab.filter.show_dirs,
            Action::ToggleFiles => tab.filter.show_files,
            Action::ToggleSymlinks => tab.filter.show_symlinks,
            Action::ToggleHidden => tab.filter.show_hidden,
            Action::ToggleOnlyMarked => tab.filter.only_marked,
            Action::SortBy(by) => tab.sort.by == by,
            Action::ToggleDirsFirst => tab.sort.dirs_first,
            Action::ToggleReverse => tab.sort.reverse,
            _ => false,
        };
        let key = keymap::label(&binding.keys[0]);
        let style = if active {
            Style::new().fg(Color::Green).bold()
        } else {
            Style::new().dim()
        };
        spans.push(Span::styled(
            format!("[{key}]"),
            Style::new().fg(Color::Yellow),
        ));
        spans.push(Span::styled(format!("{} ", binding.desc), style));
    }
    Line::from(spans)
}

fn draw_help(frame: &mut Frame, area: Rect, scroll: u16) {
    let entries: Vec<(String, &str)> = keymap::NORMAL
        .iter()
        .filter(|b| !b.desc.is_empty())
        .map(|b: &Binding| {
            let keys: Vec<String> = b.keys.iter().map(keymap::label).collect();
            (keys.join(" "), b.desc)
        })
        .chain(menu_help("f", keymap::FILTER_MENU))
        .chain(menu_help("s", keymap::SORT_MENU))
        .collect();

    let key_width = entries
        .iter()
        .map(|(k, _)| k.chars().count())
        .max()
        .unwrap_or(0);
    let col_width = key_width + 2 + entries.iter().map(|(_, d)| d.len()).max().unwrap_or(0) + 3;
    // as many columns as fit, whatever does not fit in height can be scrolled
    let cols = (area.width as usize / col_width).max(1);
    let rows = entries.len().div_ceil(cols);

    let mut lines = vec![Line::from(Span::styled(
        " keys  (j/k scroll, any other key closes)",
        Style::new().bold(),
    ))];
    for r in 0..rows {
        let mut spans = vec![Span::raw(" ")];
        for (key, desc) in entries.iter().skip(r).step_by(rows) {
            spans.push(Span::styled(
                format!("{key:>key_width$}  "),
                Style::new().fg(Color::Yellow),
            ));
            spans.push(Span::raw(format!(
                "{desc:<width$}",
                width = col_width - key_width - 2
            )));
        }
        lines.push(Line::from(spans));
    }
    // never scroll past the last line
    let scroll = scroll.min((lines.len() as u16).saturating_sub(area.height));
    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), area);
}

fn menu_help(
    prefix: &'static str,
    table: &'static [Binding],
) -> impl Iterator<Item = (String, &'static str)> {
    table
        .iter()
        .map(move |b| (format!("{prefix} {}", keymap::label(&b.keys[0])), b.desc))
}

fn permissions(node: &Node) -> String {
    let kind = match node.kind {
        NodeKind::Dir => 'd',
        NodeKind::Symlink { .. } => 'l',
        NodeKind::File => '-',
    };
    let mode = node.meta.mode;
    let mut s = String::with_capacity(10);
    s.push(kind);
    for shift in [6, 3, 0] {
        let bits = (mode >> shift) & 0o7;
        s.push(if bits & 4 != 0 { 'r' } else { '-' });
        s.push(if bits & 2 != 0 { 'w' } else { '-' });
        s.push(if bits & 1 != 0 { 'x' } else { '-' });
    }
    s
}

fn human_size(size: u64) -> String {
    const UNITS: [&str; 5] = ["B", "K", "M", "G", "T"];
    let mut value = size as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{size}B")
    } else if value < 10.0 {
        format!("{value:.1}{}", UNITS[unit])
    } else {
        format!("{value:.0}{}", UNITS[unit])
    }
}

fn format_time(time: Option<SystemTime>) -> String {
    time.map(|t| {
        DateTime::<Local>::from(t)
            .format("%Y-%m-%d %H:%M")
            .to_string()
    })
    .unwrap_or_default()
}

fn tilde(path: &str) -> String {
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && path.starts_with(&home) => {
            format!("~{}", &path[home.len()..])
        }
        _ => path.to_string(),
    }
}

fn byte_offset(text: &str, chars: usize) -> usize {
    text.char_indices()
        .nth(chars)
        .map(|(i, _)| i)
        .unwrap_or(text.len())
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend};

    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(human_size(512), "512B");
        assert_eq!(human_size(1536), "1.5K");
        assert_eq!(human_size(50 * 1024 * 1024), "50M");
    }

    #[test]
    fn renders_tree_and_status() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/main.rs"), "").unwrap();
        std::fs::write(dir.path().join("README.md"), "").unwrap();
        let mut app =
            App::new(&[dir.path().to_path_buf()], crate::pins::Pins::in_memory()).unwrap();
        app.apply(crate::action::Action::Expand);

        let mut terminal = Terminal::new(TestBackend::new(60, 6)).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let lines: Vec<String> = terminal
            .backend()
            .buffer()
            .content()
            .chunks(60)
            .map(|row| {
                row.iter()
                    .map(|c| c.symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect();

        assert!(lines[0].starts_with(" 1 .tmp"), "{lines:?}");
        assert_eq!(lines[1], "   ├─src/");
        assert_eq!(lines[2], "   │ └─main.rs");
        assert_eq!(lines[3], "   └─README.md");
        assert!(lines[5].starts_with("? help"), "{lines:?}");
        assert!(lines[5].ends_with("sort: name  1/3"), "{lines:?}");
    }
}
