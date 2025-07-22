mod app;
mod node;
mod ui;

use app::{App, Filter};
use color_eyre::Result;
use node::{Node, NodeKind};

fn flatten_tree_for_list(root_node: &Node, filter: &Filter) -> Vec<String> {
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

fn main() -> Result<()> {
    color_eyre::install()?;
    let terminal = ratatui::init_with_options(ratatui::TerminalOptions {
        viewport: ratatui::Viewport::Inline(50),
    });
    let app_result = App::new().unwrap().run(terminal);
    ratatui::restore();
    app_result
}
