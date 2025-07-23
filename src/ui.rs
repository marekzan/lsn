use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize, palette::tailwind::SLATE},
    text::ToSpan,
    widgets::{Block, HighlightSpacing, List, ListItem, Paragraph, StatefulWidget, Widget},
};

use crate::{app::App, flatten_tree_for_list};

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
        let binding = "Filesystem".to_span().into_left_aligned_line();
        let block = Block::bordered().fg(Color::Green).title(binding);

        // Iterate through all elements in the `items` and stylize them.
        let flattened_list = flatten_tree_for_list(&self.content, &self.filter);

        let items: Vec<ListItem> = flattened_list
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
