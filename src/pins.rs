use std::{
    io,
    path::{Path, PathBuf},
};

/// pinned folders, stored as one path per line in `$XDG_DATA_HOME/lsn/pins`
#[derive(Debug, Default)]
pub struct Pins {
    paths: Vec<PathBuf>,
    file: Option<PathBuf>,
}

impl Pins {
    pub fn load() -> Self {
        let file = default_file();
        let paths = file
            .as_deref()
            .and_then(|f| std::fs::read_to_string(f).ok())
            .map(|s| {
                s.lines()
                    .filter(|l| !l.is_empty())
                    .map(PathBuf::from)
                    .collect()
            })
            .unwrap_or_default();
        Self { paths, file }
    }

    /// pins that are never written to disk (tests)
    #[cfg(test)]
    pub fn in_memory() -> Self {
        Self::default()
    }

    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.paths.iter().any(|p| p == path)
    }

    /// returns `true` if the path is pinned now
    pub fn toggle(&mut self, path: &Path) -> io::Result<bool> {
        let pinned = if self.contains(path) {
            self.paths.retain(|p| p != path);
            false
        } else {
            self.paths.push(path.to_path_buf());
            true
        };
        self.save()?;
        Ok(pinned)
    }

    pub fn remove(&mut self, path: &Path) -> io::Result<()> {
        if self.contains(path) {
            self.paths.retain(|p| p != path);
            self.save()?;
        }
        Ok(())
    }

    fn save(&self) -> io::Result<()> {
        let Some(file) = &self.file else {
            return Ok(());
        };
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let content: String = self
            .paths
            .iter()
            .map(|p| format!("{}\n", p.display()))
            .collect();
        std::fs::write(file, content)
    }
}

fn default_file() -> Option<PathBuf> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(data.join("lsn").join("pins"))
}
