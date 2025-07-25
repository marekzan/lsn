use log::info;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize, palette::tailwind::SLATE},
    text::ToSpan,
    widgets::{Block, HighlightSpacing, List, ListItem, Paragraph, StatefulWidget, Widget},
};

use crate::{
    Node, NodeKind,
    app::{App, Filter},
};

const SELECTED_STYLE: Style = Style::new().bg(SLATE.c800).add_modifier(Modifier::BOLD);

impl Widget for &mut App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let [main_area, footer_area] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);

        let [list_area] = Layout::vertical([Constraint::Fill(1)]).areas(main_area);

        App::render_footer(footer_area, buf);
        self.render_list(list_area, buf);
    }
}

/// Rendering logic for the app
impl App {
    fn render_footer(area: Rect, buf: &mut Buffer) {
        Paragraph::new("Use ↓↑ to move, ← to unselect, → to change status, g/G to go top/bottom.")
            .centered()
            .render(area, buf);
    }

    fn render_list(&mut self, area: Rect, buf: &mut Buffer) {
        let title = "Filesystem".to_span().into_left_aligned_line();
        let block = Block::bordered().fg(Color::Green).title(title);

        let items: Vec<ListItem> = self
            .list_view
            .iter()
            .map(|item| ListItem::new(item.as_str()))
            .collect();

        // Create a List from all list items and highlight the currently selected one
        let list = List::new(items)
            .block(block)
            .highlight_style(SELECTED_STYLE)
            .highlight_symbol(">")
            .highlight_spacing(HighlightSpacing::Always);

        // We need to disambiguate this trait method as both `Widget` and `StatefulWidget` share the
        // same method name `render`.
        StatefulWidget::render(list, area, buf, &mut self.state);
    }
}

pub fn flatten_tree_for_list(root_node: &Node, filter: &Filter) -> Vec<String> {
    info!("flattening list for view");
    let mut list_items = Vec::new();
    build_list_recursive(root_node, &mut list_items, 0, filter);
    list_items
}

fn build_list_recursive(node: &Node, list: &mut Vec<String>, depth: usize, filter: &Filter) {
    let indent = "  ".repeat(depth);
    let prefix = if let NodeKind::Directory { is_open, .. } = &node.kind {
        if *is_open { "\u{f115} " } else { "\u{e5ff} " }
    } else {
        "\u{f01a7} "
    };
    let name = node.path.file_name().unwrap_or_default().to_string_lossy();
    list.push(format!("{}{}{}", indent, prefix, name));

    if let NodeKind::Directory { children, is_open } = &node.kind
        && let Some(children) = children
    {
        if *is_open {
            for child in children {
                let mut should_display = true;

                match child.kind {
                    NodeKind::Directory { is_open, .. } => {
                        if filter.directories && !is_open {
                            should_display = false;
                        }
                    }
                    NodeKind::File => {
                        if filter.files {
                            should_display = false;
                        }

                        if filter.dotfiles {
                            if let Some(name) = child.path.file_name().and_then(|s| s.to_str()) {
                                if name.starts_with('.') {
                                    should_display = false;
                                }
                            }
                        }
                    }
                }

                if should_display {
                    build_list_recursive(child, list, depth + 1, filter);
                }
            }
        }
    }
}
