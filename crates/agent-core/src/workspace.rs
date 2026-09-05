//! Workspace — a project root directory the agent operates in.
//!
//! All file tools (`read`, `write`, `patch`, `grep`, `glob`) and
//! `bash` resolve paths relative to this root. The agent loop
//! holds one Workspace per run; child agents inherit it.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Normalize a path for cross-platform comparison.
/// Strips Windows UNC verbatim prefixes (`\\?\` and `\\?\UNC\`),
/// normalizes disk letter casing, and resolves standard components.
pub fn normalize_path(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    let stripped = if s.len() >= 8 && s[..8].eq_ignore_ascii_case(r"\\?\UNC\") {
        format!(r"\\{}", &s[8..])
    } else if s.len() >= 4 && s[..4].eq_ignore_ascii_case(r"\\?\") {
        s[4..].to_string()
    } else {
        s.into_owned()
    };

    let p = PathBuf::from(stripped);
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::Prefix(_prefix) => {
                #[cfg(windows)]
                {
                    use std::path::Prefix;
                    let norm = match _prefix.kind() {
                        Prefix::Disk(d) | Prefix::VerbatimDisk(d) => {
                            let upper = (d as char).to_ascii_uppercase() as u8;
                            format!("{}:", upper as char)
                        }
                        _ => _prefix.as_os_str().to_string_lossy().into_owned(),
                    };
                    out.push(norm);
                }
                #[cfg(not(windows))]
                {
                    out.push(c.as_os_str());
                }
            }
            _ => out.push(c.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        out
    }
}

/// A workspace is just a path + optional project metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    /// Project root (absolute). All tool paths resolve under here.
    pub root: PathBuf,
    /// Display name (e.g. `"my-app"`).
    pub name: String,
}

impl Default for Workspace {
    fn default() -> Self {
        Self {
            root: PathBuf::from("."),
            name: ".".into(),
        }
    }
}

impl Workspace {
    /// Build a workspace from an existing path.
    pub fn new(root: impl Into<PathBuf>, name: impl Into<String>) -> Self {
        let root = normalize_path(&root.into());
        Self {
            root,
            name: name.into(),
        }
    }

    /// Resolve a relative path against the workspace root.
    /// Returns the normalized path if it is already absolute.
    pub fn resolve(&self, rel: impl AsRef<Path>) -> PathBuf {
        let p = rel.as_ref();
        if p.is_absolute() {
            normalize_path(p)
        } else {
            normalize_path(&self.root.join(p))
        }
    }

    /// Inverse of `resolve`: produce a path relative to the root
    /// when possible. Used for stable tool-output diffs.
    pub fn relativize(&self, abs: &Path) -> PathBuf {
        let norm_abs = normalize_path(abs);
        let norm_root = normalize_path(&self.root);
        if let Ok(rel) = norm_abs.strip_prefix(&norm_root) {
            return rel.to_path_buf();
        }
        #[cfg(windows)]
        {
            if self.contains(&norm_abs) {
                let r_comps: Vec<_> = norm_root.components().collect();
                let a_comps: Vec<_> = norm_abs.components().collect();
                if a_comps.len() >= r_comps.len() {
                    let mut rel = PathBuf::new();
                    for c in &a_comps[r_comps.len()..] {
                        rel.push(c.as_os_str());
                    }
                    return rel;
                }
            }
        }
        abs.to_path_buf()
    }

    /// True if `path` lives inside this workspace.
    ///
    /// Handles Windows verbatim prefixes (`\\?\`), case-insensitivity
    /// on Windows, and protects against prefix-overlap attacks (e.g.
    /// `/tmp/proj-evil` is NOT inside `/tmp/proj`).
    pub fn contains(&self, path: &Path) -> bool {
        let root = normalize_path(&self.root);
        let target = normalize_path(path);

        let mut a = root.components();
        let mut b = target.components();
        loop {
            match (a.next(), b.next()) {
                (Some(x), Some(y)) => {
                    if !components_equal(&x, &y) {
                        return false;
                    }
                }
                (None, _) => return true, // root is a proper prefix
                _ => return false,
            }
        }
    }

    /// Verify that `path` is within the workspace, both lexically and physically
    /// (resolving any symlinks / junction points).
    /// If the path or its existing parent canonicalizes outside the workspace root,
    /// returns an Err with a detailed security denial message.
    pub fn verify_path(&self, path: &Path) -> Result<PathBuf, String> {
        let abs = if path.is_absolute() {
            normalize_path(path)
        } else {
            normalize_path(&self.root.join(path))
        };

        if !self.contains(&abs) {
            return Err(format!(
                "Path '{}' escapes workspace root '{}'",
                abs.display(),
                self.root.display()
            ));
        }

        // If the path exists, canonicalize it and check if its canonical target is inside canonical root.
        if abs.exists() {
            if let (Ok(canon_target), Ok(canon_root)) =
                (abs.canonicalize(), self.root.canonicalize())
            {
                let norm_target = normalize_path(&canon_target);
                let norm_root = normalize_path(&canon_root);
                let target_ws = Workspace {
                    root: norm_root,
                    name: self.name.clone(),
                };
                if !target_ws.contains(&norm_target) {
                    return Err(format!(
                        "Symlink / junction '{}' resolves outside workspace root to '{}'",
                        abs.display(),
                        norm_target.display()
                    ));
                }
            }
        } else {
            // Check the nearest existing ancestor
            let mut curr = abs.as_path();
            while let Some(parent) = curr.parent() {
                if parent.exists() {
                    if let (Ok(canon_parent), Ok(canon_root)) =
                        (parent.canonicalize(), self.root.canonicalize())
                    {
                        let norm_parent = normalize_path(&canon_parent);
                        let norm_root = normalize_path(&canon_root);
                        let target_ws = Workspace {
                            root: norm_root,
                            name: self.name.clone(),
                        };
                        if !target_ws.contains(&norm_parent) {
                            return Err(format!(
                                "Path ancestor '{}' resolves outside workspace root to '{}'",
                                parent.display(),
                                norm_parent.display()
                            ));
                        }
                    }
                    break;
                }
                curr = parent;
            }
        }

        Ok(abs)
    }
}

fn components_equal(a: &std::path::Component, b: &std::path::Component) -> bool {
    #[cfg(windows)]
    {
        use std::path::Component;
        match (a, b) {
            (Component::Prefix(px), Component::Prefix(py)) => {
                use std::path::Prefix;
                match (px.kind(), py.kind()) {
                    (
                        Prefix::Disk(dx) | Prefix::VerbatimDisk(dx),
                        Prefix::Disk(dy) | Prefix::VerbatimDisk(dy),
                    ) => dx.eq_ignore_ascii_case(&dy),
                    _ => px
                        .as_os_str()
                        .to_string_lossy()
                        .eq_ignore_ascii_case(&py.as_os_str().to_string_lossy()),
                }
            }
            (Component::Normal(nx), Component::Normal(ny)) => nx
                .to_string_lossy()
                .eq_ignore_ascii_case(&ny.to_string_lossy()),
            (x, y) => x == y,
        }
    }
    #[cfg(not(windows))]
    {
        a == b
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

    #[test]
    fn contains_rejects_prefix_overlap_attack() {
        // /tmp/proj-evil must NOT be considered inside /tmp/proj.
        let ws = Workspace::new(
            if cfg!(windows) {
                r"C:\proj"
            } else {
                "/tmp/proj"
            },
            "proj",
        );
        let evil = if cfg!(windows) {
            PathBuf::from(r"C:\proj-evil\passwd")
        } else {
            PathBuf::from("/tmp/proj-evil/passwd")
        };
        assert!(!ws.contains(&evil), "{evil:?} leaked into {ws:?}");
    }

    #[test]
    fn contains_accepts_root_itself_and_deeper() {
        let ws = Workspace::new(
            if cfg!(windows) {
                r"C:\proj"
            } else {
                "/tmp/proj"
            },
            "proj",
        );
        let root = if cfg!(windows) {
            PathBuf::from(r"C:\proj")
        } else {
            PathBuf::from("/tmp/proj")
        };
        assert!(ws.contains(&root));
        let deeper = if cfg!(windows) {
            PathBuf::from(r"C:\proj\src\main.rs")
        } else {
            PathBuf::from("/tmp/proj/src/main.rs")
        };
        assert!(ws.contains(&deeper));
    }

    #[test]
    #[cfg(windows)]
    fn contains_handles_windows_verbatim_and_case() {
        let ws = Workspace::new(r"\\?\F:\111\aiclaw\Flowntier", "Flowntier");
        // Standard uppercase absolute path
        assert!(ws.contains(Path::new(r"F:\111\aiclaw\Flowntier\src\main.rs")));
        // Lowercase drive letter
        assert!(ws.contains(Path::new(r"f:\111\aiclaw\flowntier\src\main.rs")));
        // Forward slashes
        assert!(ws.contains(Path::new("f:/111/aiclaw/Flowntier/src/main.rs")));
        // Outside path
        assert!(!ws.contains(Path::new(r"C:\Windows\System32\cmd.exe")));
        assert!(!ws.contains(Path::new(r"F:\111\aiclaw\Flowntier-other\main.rs")));
    }
}
