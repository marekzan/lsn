use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, HighlightSpacing, List, ListItem, Paragraph, StatefulWidget, Widget},
};

use crate::{ViewItem, ViewItemKind, app::Ui};

const SELECTED_STYLE: Style = Style::new()
    .bg(Color::Rgb(50, 50, 50))
    .add_modifier(Modifier::BOLD);

pub fn render(app: &mut Ui, items: &[ViewItem], area: Rect, buf: &mut Buffer) {
    let [main_area, footer_area] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);

    render_footer(footer_area, buf);
    render_list(app, items, main_area, buf);
}

fn render_footer(area: Rect, buf: &mut Buffer) {
    Paragraph::new("↓↑: move | ←→/Enter: open/close | g/G: top/bottom | f: filter | q: quit")
        .centered()
        .render(area, buf);
}

fn render_list(app: &mut Ui, items: &[ViewItem], area: Rect, buf: &mut Buffer) {
    let title = Line::from(" lsn ".bold()).left_aligned();
    let block = Block::bordered().title(title);

    let list_items: Vec<ListItem> = items
        .iter()
        .map(|item| {
            let indent = "  ".repeat(item.depth);

            let prefix = match &item.kind {
                ViewItemKind::Directory { is_open } => {
                    if *is_open {
                        " "
                    } else {
                        " "
                    }
                }
                ViewItemKind::File => " ",
            };

            let line = Line::from(vec![
                Span::raw(indent),
                Span::styled(prefix, Style::default().fg(Color::Blue)),
                Span::raw(&item.name),
            ]);
            ListItem::new(line)
        })
        .collect();

    let list = List::new(list_items)
        .block(block)
        .highlight_style(SELECTED_STYLE)
        .highlight_symbol(">  ")
        .highlight_spacing(HighlightSpacing::Always);

    StatefulWidget::render(list, area, buf, &mut app.state);
}
