use color_eyre::Result;
use ratatui::{
    DefaultTerminal,
    buffer::Buffer,
    crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind},
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize, palette::tailwind::SLATE},
    text::{Line, ToSpan},
    widgets::{
        Block, HighlightSpacing, List, ListItem, ListState, Paragraph, StatefulWidget, Widget,
    },
};

const SELECTED_STYLE: Style = Style::new().bg(SLATE.c800).add_modifier(Modifier::BOLD);
const TEXT_FG_COLOR: Color = SLATE.c200;

fn main() -> Result<()> {
    color_eyre::install()?;
    let terminal = ratatui::init_with_options(ratatui::TerminalOptions {
        viewport: ratatui::Viewport::Inline(20),
    });
    let app_result = App::default().run(terminal);
    ratatui::restore();
    app_result
}

struct App {
    should_exit: bool,
    content: NodeList,
}

struct DirNode {
    name: String,
    status: DirStatus,
    content: Option<NodeList>,
}

struct FileNode {
    name: String,
}

enum NodeType {
    Directory(DirNode),
    File(FileNode),
}

struct NodeList {
    items: Vec<NodeType>,
    state: ListState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum DirStatus {
    Closed,
    Opened,
}

impl Default for App {
    fn default() -> Self {
        Self {
            should_exit: false,
            content: NodeList {
                items: vec![
                    NodeType::Directory(DirNode {
                        name: "hello".to_string(),
                        content: Some(NodeList {
                            items: vec![NodeType::File(FileNode {
                                name: "file1.txt".to_string(),
                            })],
                            state: ListState::default(),
                        }),
                        status: DirStatus::Closed,
                    }),
                    NodeType::Directory(DirNode {
                        name: "world".to_string(),
                        content: None,
                        status: DirStatus::Closed,
                    }),
                    NodeType::File(FileNode {
                        name: "file1.txt".to_string(),
                    }),
                    NodeType::File(FileNode {
                        name: "file2.txt".to_string(),
                    }),
                ],
                state: ListState::default(),
            },
        }
    }
}

impl App {
    fn run(mut self, mut terminal: DefaultTerminal) -> Result<()> {
        while !self.should_exit {
            terminal.draw(|frame| frame.render_widget(&mut self, frame.area()))?;
            if let Event::Key(key) = event::read()? {
                self.handle_key(key);
            };
        }
        Ok(())
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.should_exit = true,
            KeyCode::Char('h') | KeyCode::Left => self.select_none(),
            KeyCode::Char('j') | KeyCode::Down => self.select_next(),
            KeyCode::Char('k') | KeyCode::Up => self.select_previous(),
            KeyCode::Char('g') | KeyCode::Home => self.select_first(),
            KeyCode::Char('G') | KeyCode::End => self.select_last(),
            KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter => {
                self.toggle_status();
            }
            _ => {}
        }
    }

    fn select_none(&mut self) {
        self.content.state.select(None);
    }

    fn select_next(&mut self) {
        self.content.state.select_next();
    }
    fn select_previous(&mut self) {
        self.content.state.select_previous();
    }

    fn select_first(&mut self) {
        self.content.state.select_first();
    }

    fn select_last(&mut self) {
        self.content.state.select_last();
    }

    /// Changes the status of the selected list item
    fn toggle_status(&mut self) {
        if let Some(i) = self.content.state.selected() {
            match &mut self.content.items[i] {
                NodeType::Directory(dir) => {
                    dir.status = match dir.status {
                        DirStatus::Closed => DirStatus::Opened,
                        DirStatus::Opened => DirStatus::Closed,
                    }
                }
                NodeType::File(_) => (),
            }
        }
    }
}

impl Widget for &mut App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let [header_area, main_area, footer_area] = Layout::vertical([
            Constraint::Length(2),
            Constraint::Fill(1),
            Constraint::Length(1),
        ])
        .areas(area);

        let [list_area] = Layout::vertical([Constraint::Fill(1)]).areas(main_area);

        App::render_header(header_area, buf);
        App::render_footer(footer_area, buf);
        self.render_list(list_area, buf);
    }
}

/// Rendering logic for the app
impl App {
    fn render_header(area: Rect, buf: &mut Buffer) {
        Paragraph::new("LS Navigator")
            .bold()
            .centered()
            .render(area, buf);
    }

    fn render_footer(area: Rect, buf: &mut Buffer) {
        Paragraph::new("Use ↓↑ to move, ← to unselect, → to change status, g/G to go top/bottom.")
            .centered()
            .render(area, buf);
    }

    fn render_list(&mut self, area: Rect, buf: &mut Buffer) {
        let binding = "Filesystem".to_span().into_left_aligned_line();
        let block = Block::bordered().fg(Color::Green).title(binding);

        // Iterate through all elements in the `items` and stylize them.
        let items: Vec<ListItem> = self
            .content
            .items
            .iter()
            .map(|node| ListItem::from(node))
            .collect();

        // Create a List from all list items and highlight the currently selected one
        let list = List::new(items)
            .block(block)
            .highlight_style(SELECTED_STYLE)
            .highlight_symbol(">")
            .highlight_spacing(HighlightSpacing::Always);

        // We need to disambiguate this trait method as both `Widget` and `StatefulWidget` share the
        // same method name `render`.
        StatefulWidget::render(list, area, buf, &mut self.content.state);
    }
}

impl From<&NodeType> for ListItem<'_> {
    fn from(value: &NodeType) -> Self {
        let line = match value {
            NodeType::Directory(dir) => match dir.status {
                DirStatus::Opened => Line::styled(format!(" -> {}", dir.name), TEXT_FG_COLOR),
                DirStatus::Closed => Line::styled(format!(" x {}", dir.name), TEXT_FG_COLOR),
            },
            NodeType::File(file) => Line::styled(format!(" f {}", file.name), TEXT_FG_COLOR),
        };

        ListItem::new(line)
    }
}
