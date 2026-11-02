//! file system operations. everything here works on paths only and knows
//! nothing about trees or tabs, the app refreshes the affected directories
//! afterwards.

use std::{
    fs, io,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

/// what happens with the clipboard content when it is pasted
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferKind {
    Copy,
    Move,
}

/// copies or moves `src` into the directory `dest_dir` and returns the new path.
/// if the name is already taken a free one (`name_1.ext`, ...) is used.
pub fn transfer(src: &Path, dest_dir: &Path, kind: TransferKind) -> io::Result<PathBuf> {
    let name = src
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;

    if src.is_dir() && dest_dir.starts_with(src) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("cannot put {} into itself", src.display()),
        ));
    }

    let dest = dest_dir.join(name);
    // moving to the same place is a no-op
    if kind == TransferKind::Move && dest == src {
        return Ok(dest);
    }
    let dest = free_name(&dest);

    match kind {
        TransferKind::Copy => copy_recursive(src, &dest)?,
        TransferKind::Move => {
            if let Err(err) = fs::rename(src, &dest) {
                // rename does not work across file systems
                if err.kind() != io::ErrorKind::CrossesDevices {
                    return Err(err);
                }
                copy_recursive(src, &dest)?;
                remove(src)?;
            }
        }
    }
    Ok(dest)
}

/// `path` if it does not exist yet, otherwise `stem_N.ext` with the smallest free N
pub fn free_name(path: &Path) -> PathBuf {
    if fs::symlink_metadata(path).is_err() {
        return path.to_path_buf();
    }
    let parent = path.parent().unwrap_or(Path::new(""));
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    // dotfiles like `.bashrc` have no extension
    let (stem, ext) = match file_name.rfind('.') {
        Some(i) if i > 0 => (&file_name[..i], &file_name[i..]),
        _ => (file_name.as_str(), ""),
    };
    (1..)
        .map(|n| parent.join(format!("{stem}_{n}{ext}")))
        .find(|p| fs::symlink_metadata(p).is_err())
        .expect("there is always a free name")
}

fn copy_recursive(src: &Path, dest: &Path) -> io::Result<()> {
    let md = fs::symlink_metadata(src)?;
    if md.file_type().is_symlink() {
        let target = fs::read_link(src)?;
        std::os::unix::fs::symlink(target, dest)?;
    } else if md.is_dir() {
        fs::create_dir(dest)?;
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &dest.join(entry.file_name()))?;
        }
        fs::set_permissions(dest, md.permissions())?;
    } else {
        fs::copy(src, dest)?;
    }
    Ok(())
}

/// deletes a file, symlink or a whole directory
pub fn remove(path: &Path) -> io::Result<()> {
    let md = fs::symlink_metadata(path)?;
    if md.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

pub fn rename(path: &Path, new_name: &str) -> io::Result<PathBuf> {
    validate_name(new_name)?;
    let dest = path.with_file_name(new_name);
    if dest != path && fs::symlink_metadata(&dest).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{new_name} already exists"),
        ));
    }
    fs::rename(path, &dest)?;
    Ok(dest)
}

/// creates a file, or a directory when `name` ends with `/`.
/// intermediate directories (`a/b/c.txt`) are created as well.
pub fn create(dir: &Path, name: &str) -> io::Result<PathBuf> {
    let is_dir = name.ends_with('/');
    let name = name.trim_end_matches('/');
    if name.is_empty()
        || name
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
    {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid name"));
    }
    let path = dir.join(name);
    if fs::symlink_metadata(&path).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{name} already exists"),
        ));
    }
    if is_dir {
        fs::create_dir_all(&path)?;
    } else {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::File::create_new(&path)?;
    }
    Ok(path)
}

/// sets the permission bits, `mode` is octal like `755` or `0644`
pub fn chmod(path: &Path, mode: &str) -> io::Result<()> {
    let mode = parse_mode(mode).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "mode must be octal, e.g. 644")
    })?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

pub fn parse_mode(mode: &str) -> Option<u32> {
    let mode = mode.trim();
    if mode.is_empty() || mode.len() > 4 {
        return None;
    }
    u32::from_str_radix(mode, 8).ok().filter(|m| *m <= 0o7777)
}

fn validate_name(name: &str) -> io::Result<()> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid name"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn copy_dir_recursively_with_free_name() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        fs::create_dir_all(src.join("inner")).unwrap();
        fs::write(src.join("inner/f.txt"), "hi").unwrap();

        let dest = transfer(&src, dir.path(), TransferKind::Copy).unwrap();
        assert_eq!(dest, dir.path().join("src_1"));
        assert_eq!(fs::read_to_string(dest.join("inner/f.txt")).unwrap(), "hi");
        assert!(src.exists());
    }

    #[test]
    fn move_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("target")).unwrap();
        let file = dir.path().join("a.txt");
        fs::write(&file, "a").unwrap();

        let dest = transfer(&file, &dir.path().join("target"), TransferKind::Move).unwrap();
        assert_eq!(dest, dir.path().join("target/a.txt"));
        assert!(!file.exists());
    }

    #[test]
    fn move_into_itself_fails() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        fs::create_dir_all(src.join("inner")).unwrap();
        assert!(transfer(&src, &src.join("inner"), TransferKind::Move).is_err());
        assert!(transfer(&src, &src, TransferKind::Copy).is_err());
    }

    #[test]
    fn move_to_same_dir_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        fs::write(&file, "a").unwrap();
        assert_eq!(
            transfer(&file, dir.path(), TransferKind::Move).unwrap(),
            file
        );
        assert!(file.exists());
    }

    #[test]
    fn free_names() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["a.tar.gz", ".bashrc", "a.tar_1.gz"] {
            fs::write(dir.path().join(name), "").unwrap();
        }
        assert_eq!(
            free_name(&dir.path().join("a.tar.gz")),
            dir.path().join("a.tar_2.gz")
        );
        assert_eq!(
            free_name(&dir.path().join(".bashrc")),
            dir.path().join(".bashrc_1")
        );
        assert_eq!(free_name(&dir.path().join("new")), dir.path().join("new"));
    }

    #[test]
    fn create_files_and_dirs() {
        let dir = tempfile::tempdir().unwrap();
        assert!(create(dir.path(), "x/y/").unwrap().is_dir());
        assert!(create(dir.path(), "x/z.txt").unwrap().is_file());
        assert!(create(dir.path(), "x/z.txt").is_err());
        assert!(create(dir.path(), "../evil").is_err());
    }

    #[test]
    fn rename_and_chmod() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a");
        fs::write(&file, "").unwrap();
        let new = rename(&file, "b").unwrap();
        assert!(new.exists() && !file.exists());
        assert!(rename(&new, "x/y").is_err());

        chmod(&new, "640").unwrap();
        assert_eq!(
            fs::metadata(&new).unwrap().permissions().mode() & 0o7777,
            0o640
        );
        assert!(chmod(&new, "999").is_err());
    }

    #[test]
    fn remove_dir_and_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("d/e")).unwrap();
        fs::write(dir.path().join("f"), "").unwrap();
        remove(&dir.path().join("d")).unwrap();
        remove(&dir.path().join("f")).unwrap();
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}
