use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, HighlightSpacing, List, ListItem, Paragraph, StatefulWidget, Widget},
};

use crate::{app::App, node::NodeKind};

const SELECTED_STYLE: Style = Style::new()
    .bg(Color::Rgb(50, 50, 50))
    .add_modifier(Modifier::BOLD);

impl Widget for &mut App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let [main_area, footer_area] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);

        App::render_footer(footer_area, buf);
        self.render_list(main_area, buf);
    }
}

impl App {
    fn render_footer(area: Rect, buf: &mut Buffer) {
        Paragraph::new("↓↑: move | ←→/Enter: open/close | g/G: top/bottom | f: filter | q: quit")
            .centered()
            .render(area, buf);
    }

    fn render_list(&mut self, area: Rect, buf: &mut Buffer) {
        let title = Line::from(" lsn ".bold()).left_aligned();
        let block = Block::bordered().title(title);

        let items: Vec<ListItem> = self
            .ui_representation
            .iter()
            .map(|path| {
                let node_id = self.path_to_id.get(path).unwrap();
                let node = self.tree.get(*node_id).unwrap();

                let indent = "  ".repeat(node.data.depth);

                let prefix = match &node.data.kind {
                    NodeKind::Directory { is_open, .. } => {
                        if *is_open {
                            " "
                        } else {
                            " "
                        }
                    }
                    NodeKind::File => " ",
                };

                let name = node
                    .data
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy();

                let line = Line::from(vec![
                    Span::raw(indent),
                    Span::styled(prefix, Style::default().fg(Color::Blue)),
                    Span::raw(name),
                ]);
                ListItem::new(line)
            })
            .collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(SELECTED_STYLE)
            .highlight_symbol(">  ")
            .highlight_spacing(HighlightSpacing::Always);

        StatefulWidget::render(list, area, buf, &mut self.state);
    }
}
