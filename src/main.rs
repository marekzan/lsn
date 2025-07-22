mod app;
mod node;
mod ui;

use app::{App, Sort};
use color_eyre::Result;
use node::{Node, NodeKind};

fn flatten_tree_for_list(root_node: &Node, sort: &Sort) -> Vec<String> {
    let mut list_items = Vec::new();
    build_list_recursive(root_node, &mut list_items, 0, sort);
    list_items
}

fn build_list_recursive(node: &Node, list: &mut Vec<String>, depth: usize, sort: &Sort) {
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
            let mut sorted_children = children.iter().collect::<Vec<_>>();

            sorted_children.sort_by(|a, b| match sort {
                Sort::Directory => {
                    let a_is_dir = matches!(a.kind, NodeKind::Directory { .. });
                    let b_is_dir = matches!(b.kind, NodeKind::Directory { .. });
                    b_is_dir.cmp(&a_is_dir).then_with(|| a.path.cmp(&b.path))
                }
                Sort::File => {
                    let a_is_dir = matches!(a.kind, NodeKind::Directory { .. });
                    let b_is_dir = matches!(b.kind, NodeKind::Directory { .. });
                    a_is_dir.cmp(&b_is_dir).then_with(|| a.path.cmp(&b.path))
                }
                Sort::Alphabetical => a.path.cmp(&b.path),
            });

            for child in sorted_children {
                build_list_recursive(child, list, depth + 1, sort);
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
