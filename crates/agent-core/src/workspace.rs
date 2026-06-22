//! Workspace — a project root directory the agent operates in.
//!
//! All file tools (`read`, `write`, `patch`, `grep`, `glob`) and
//! `bash` resolve paths relative to this root. The agent loop
//! holds one Workspace per run; child agents inherit it.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A workspace is just a path + optional project metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    /// Project root (absolute). All tool paths resolve under here.
    pub root: PathBuf,
    /// Display name (e.g. `"my-app"`).
    pub name: String,
}

impl Workspace {
    /// Build a workspace from an existing path.
    pub fn new(root: impl Into<PathBuf>, name: impl Into<String>) -> Self {
        Self { root: root.into(), name: name.into() }
    }

    /// Resolve a relative path against the workspace root.
    /// Returns the original path unchanged if it is already absolute.
    pub fn resolve(&self, rel: impl AsRef<Path>) -> PathBuf {
        let p = rel.as_ref();
        if p.is_absolute() { p.to_path_buf() } else { self.root.join(p) }
    }

    /// Inverse of `resolve`: produce a path relative to the root
    /// when possible. Used for stable tool-output diffs.
    pub fn relativize(&self, abs: &Path) -> PathBuf {
        abs.strip_prefix(&self.root).unwrap_or(abs).to_path_buf()
    }

    /// True if `path` lives inside this workspace.
    pub fn contains(&self, path: &Path) -> bool {
        path.starts_with(&self.root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_and_relativize_roundtrip() {
        let ws = Workspace::new("/tmp/proj", "proj");
        assert_eq!(ws.resolve("a/b.txt"), PathBuf::from("/tmp/proj/a/b.txt"));
        let abs = PathBuf::from("/tmp/proj/a/b.txt");
        assert_eq!(ws.relativize(&abs), PathBuf::from("a/b.txt"));
        assert!(ws.contains(&abs));
        assert!(!ws.contains(Path::new("/etc/passwd")));
    }
}