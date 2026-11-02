use std::{
    fs::Metadata,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    time::SystemTime,
};

use crate::arena::Handle;

/// a single entry of the file system that we have seen so far.
///
/// nodes only hold data about the entry itself plus the links into the tree
/// (parent and children). ui state that is not tied to the file system
/// (selection, clipboard, filters) lives outside of the tree.
#[derive(Debug, Clone)]
pub struct Node {
    pub name: String,
    pub path: PathBuf,
    pub kind: NodeKind,
    pub meta: Meta,
    pub parent: Option<Handle>,
    /// `None` until the directory was read for the first time (lazy loading)
    pub children: Option<Vec<Handle>>,
    pub expanded: bool,
    /// set when reading the directory failed (e.g. permission denied)
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeKind {
    Dir,
    File,
    Symlink { target: PathBuf, to_dir: bool },
}

/// the parts of the metadata we display, sort or filter by
#[derive(Debug, Clone, Default)]
pub struct Meta {
    pub size: u64,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub modified: Option<SystemTime>,
}

impl Meta {
    pub fn from_metadata(md: &Metadata) -> Self {
        Self {
            size: md.len(),
            mode: md.permissions().mode(),
            uid: md.uid(),
            gid: md.gid(),
            modified: md.modified().ok(),
        }
    }

    pub fn is_executable(&self) -> bool {
        self.mode & 0o111 != 0
    }
}

impl Node {
    /// stats the given path (without following symlinks) and creates a detached node
    pub fn from_path(path: &Path, parent: Option<Handle>) -> std::io::Result<Self> {
        let md = std::fs::symlink_metadata(path)?;
        let kind = if md.file_type().is_symlink() {
            let target = std::fs::read_link(path).unwrap_or_default();
            NodeKind::Symlink {
                target,
                to_dir: path.is_dir(),
            }
        } else if md.is_dir() {
            NodeKind::Dir
        } else {
            NodeKind::File
        };

        Ok(Self {
            name: display_name(path),
            path: path.to_path_buf(),
            kind,
            meta: Meta::from_metadata(&md),
            parent,
            children: None,
            expanded: false,
            error: None,
        })
    }

    /// directories and symlinks pointing to directories can be expanded
    pub fn is_dir(&self) -> bool {
        matches!(
            self.kind,
            NodeKind::Dir | NodeKind::Symlink { to_dir: true, .. }
        )
    }

    pub fn is_hidden(&self) -> bool {
        self.name.starts_with('.')
    }

    pub fn extension(&self) -> &str {
        if self.is_dir() {
            return "";
        }
        Path::new(&self.name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
    }
}

/// the file name, or the whole path for `/`
pub fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}
