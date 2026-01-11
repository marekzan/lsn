pub mod app;
pub mod ui;

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewItem {
    pub name: String,
    pub path: PathBuf,
    pub kind: ViewItemKind,
    pub depth: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewItemKind {
    Directory { is_open: bool },
    File,
}
