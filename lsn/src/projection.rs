use log::info;
use lsn_core::{FsNodeKind, StateManager};
use lsn_ui::{
    ViewItem, ViewItemKind,
    app::{Filter, Sort},
};
use std::path::PathBuf;

pub fn state_to_view(state: &StateManager, filter: &Filter, sort: &Sort) -> Vec<ViewItem> {
    let mut items = Vec::new();
    collect_recursive(&state.root, 0, state, &mut items, filter, sort);
    info!("{:#?}", items);
    items
}

fn collect_recursive(
    current_path: &PathBuf,
    depth: usize,
    state: &StateManager,
    items: &mut Vec<ViewItem>,
    filter: &Filter,
    sort: &Sort,
) {
    // 1. Process current entry
    let (view_kind, children) = match process_current_entry(current_path, depth, state, items) {
        Some(result) => result,
        None => return,
    };

    // 2. Recurse into children if it's an open directory
    if let ViewItemKind::Directory { is_open: true } = view_kind
        && let Some(mut children) = children
    {
        sort_children(&mut children, state, sort);

        for child_path in children {
            if should_display(&child_path, state, filter) {
                collect_recursive(&child_path, depth + 1, state, items, filter, sort);
            }
        }
    }
}

fn process_current_entry(
    path: &PathBuf,
    depth: usize,
    state: &StateManager,
    items: &mut Vec<ViewItem>,
) -> Option<(ViewItemKind, Option<Vec<PathBuf>>)> {
    let entry = state.fs_nodes.get(path)?;

    let (kind, children) = match &entry.kind {
        FsNodeKind::Directory { children, is_open } => (
            ViewItemKind::Directory { is_open: *is_open },
            Some(children.clone()),
        ),
        FsNodeKind::File => (ViewItemKind::File, None),
    };

    items.push(ViewItem {
        name: entry
            .path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string(),
        path: entry.path.clone(),
        kind: kind.clone(),
        depth,
    });

    Some((kind, children))
}

fn sort_children(children: &mut [PathBuf], state: &StateManager, sort: &Sort) {
    children.sort_by(|a_path, b_path| {
        let a = state
            .fs_nodes
            .get(a_path)
            .expect("Child path missing from fs_nodes map");
        let b = state
            .fs_nodes
            .get(b_path)
            .expect("Child path missing from fs_nodes map");

        match sort {
            Sort::Directory => {
                let a_is_dir = matches!(a.kind, FsNodeKind::Directory { .. });
                let b_is_dir = matches!(b.kind, FsNodeKind::Directory { .. });
                b_is_dir.cmp(&a_is_dir).then_with(|| a.path.cmp(&b.path))
            }
            Sort::File => {
                let a_is_dir = matches!(a.kind, FsNodeKind::Directory { .. });
                let b_is_dir = matches!(b.kind, FsNodeKind::Directory { .. });
                a_is_dir.cmp(&b_is_dir).then_with(|| a.path.cmp(&b.path))
            }
            Sort::Alphabetical => a.path.cmp(&b.path),
        }
    });
}

fn should_display(path: &PathBuf, state: &StateManager, filter: &Filter) -> bool {
    let Some(entry) = state.fs_nodes.get(path) else {
        return false;
    };

    let file_name = path.file_name().unwrap_or_default().to_string_lossy();

    if filter.dotfiles && file_name.starts_with('.') {
        return false;
    }

    match entry.kind {
        FsNodeKind::File if filter.files => false,
        FsNodeKind::Directory { is_open: false, .. } if filter.directories => false,
        _ => true,
    }
}
