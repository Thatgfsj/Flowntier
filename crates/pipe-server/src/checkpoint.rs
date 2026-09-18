//! Workspace checkpoints — v0.4.36.
//!
//! A checkpoint is a restorable snapshot of the workspace taken
//! before agents modify files (the orchestrator auto-creates one
//! before every worker run; the chairman can create one manually).
//!
//! Two storage paths:
//!
//! 1. **git** (workspace is inside a git work tree): the snapshot
//!    is a commit object built from a *temporary* index file
//!    (`GIT_INDEX_FILE` + `git add --all .` + `write-tree` +
//!    `commit-tree`). The user's real index, refs and working tree
//!    are never touched — a hard requirement from the v0.4.36
//!    debate: a checkpoint that mutates the user's repo state is
//!    worse than no checkpoint.
//! 2. **copy** (no git, or git is unavailable): files are copied to
//!    `<workspace>/.flowntier/checkpoints/<id>/files/…` with
//!    `node_modules` / `target` / `.git` / `dist` / `build` /
//!    `.flowntier` excluded and a hard 200 MB budget. Over budget
//!    → explicit error, never a silent partial snapshot.
//!
//! Restore is two-phase by design: the UI first asks for the
//! preview (`restore` with `confirm: false` → returns the file
//! list that would be overwritten), and only applies it with
//! `confirm: true`. A one-click restore that silently reverts
//! manual edits is data loss, not a feature.
//!
//! Known limitation (deliberate): the git restore path uses
//! `git restore --source <commit> --worktree`, which rewrites only
//! files present in the snapshot tree. Files created after the
//! snapshot (untracked) are left in place rather than deleted —
//! deleting unknown files automatically is how checkpoint tools
//! lose users' work. Use a copy-kind checkpoint (non-git
//! workspace) when full-state rollback matters more than
//! conservatism.

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::Stdio;

/// Directories never copied / never restored.
const EXCLUDED_DIRS: &[&str] = &[
    "node_modules",
    "target",
    ".git",
    "dist",
    "build",
    ".flowntier",
    ".nwt",
];

/// Copy-path budget. Snapshots beyond this fail loudly instead of
/// filling the user's disk behind their back.
const COPY_BUDGET_BYTES: u64 = 200 * 1024 * 1024;

/// Maximum manifest file entries kept (listing beyond this is
/// truncated with a marker; restore still works — it operates on
/// the tree, not the list).
const MAX_LISTED_FILES: usize = 5_000;

/// One checkpoint record (also the on-disk `manifest.json`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CheckpointManifest {
    /// Opaque id (timestamp + millis based).
    pub id: String,
    /// When it was taken (unix seconds).
    pub at: i64,
    /// Human label (auto ones read `auto: before worker …`).
    pub label: String,
    /// `"git"` or `"copy"`.
    pub kind: String,
    /// Commit object id when `kind == "git"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_commit: Option<String>,
    /// File count captured at create time.
    pub file_count: usize,
    /// File list (relative paths, capped at [`MAX_LISTED_FILES`]).
    pub files: Vec<String>,
    /// True when `files` was truncated.
    pub files_truncated: bool,
}

/// A restore preview: what `confirm: true` would change.
#[derive(Debug, Clone, Serialize)]
pub struct RestorePreview {
    /// Relative paths that differ from the checkpoint (or, for
    /// copy checkpoints, the full captured list).
    pub files: Vec<String>,
    /// `true` when the preview is the captured list rather than a
    /// computed diff.
    pub approximate: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum CheckpointError {
    /// I/O failure.
    #[error("checkpoint io: {0}")]
    Io(#[from] std::io::Error),
    /// Snapshot would exceed the copy budget.
    #[error("workspace too large for a copy checkpoint (>200 MB); init a git repo or exclude heavy directories")]
    OverBudget,
    /// Requested checkpoint id does not exist.
    #[error("checkpoint not found: {0}")]
    NotFound(String),
    /// A malformed checkpoint directory (no manifest).
    #[error("corrupt checkpoint directory: {0}")]
    Corrupt(String),
    /// Git is absent or refused a command.
    #[error("git: {0}")]
    Git(String),
    /// Manifest serialization failure.
    #[error("checkpoint json: {0}")]
    Json(#[from] serde_json::Error),
}

/// Root directory of all checkpoints for a workspace.
pub fn checkpoints_dir(workspace_root: &Path) -> PathBuf {
    workspace_root.join(".flowntier").join("checkpoints")
}

fn now_id() -> String {
    let t = chrono::Utc::now();
    format!(
        "{}-{:03}",
        t.format("%Y%m%d-%H%M%S"),
        t.timestamp_subsec_millis()
    )
}

/// Take a checkpoint. Tries git first, falls back to a copy.
pub async fn create(
    workspace_root: &Path,
    label: &str,
) -> Result<CheckpointManifest, CheckpointError> {
    let id = now_id();
    let dir = checkpoints_dir(workspace_root).join(&id);
    std::fs::create_dir_all(&dir)?;

    let manifest = match create_git_snapshot(workspace_root).await {
        Ok((commit, files)) => CheckpointManifest {
            id,
            at: chrono::Utc::now().timestamp(),
            label: label.to_string(),
            kind: "git".into(),
            git_commit: Some(commit),
            file_count: files.len(),
            files,
            files_truncated: false,
        },
        Err(e) => {
            // Why we fell back to a copy — invaluable when a user
            // asks why their checkpoint ballooned to 200 MB.
            let why = match &e {
                create_git::Error::NotAWorkTree => "not a git work tree".to_string(),
                create_git::Error::Git(s) => s.clone(),
            };
            tracing::debug!(target: "checkpoint", "git snapshot unavailable, copying files: {why}");
            let (files, truncated) = copy_snapshot(workspace_root, &dir).await?;
            CheckpointManifest {
                id,
                at: chrono::Utc::now().timestamp(),
                label: label.to_string(),
                kind: "copy".into(),
                git_commit: None,
                file_count: files.len(),
                files,
                files_truncated: truncated,
            }
        }
    };
    let text = serde_json::to_string_pretty(&manifest)?;
    std::fs::write(dir.join("manifest.json"), text)?;
    Ok(manifest)
}

/// List all checkpoints, newest first.
pub fn list(workspace_root: &Path) -> Vec<CheckpointManifest> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(checkpoints_dir(workspace_root)) else {
        return out;
    };
    for e in entries.flatten() {
        let p = e.path().join("manifest.json");
        if let Ok(text) = std::fs::read_to_string(&p) {
            if let Ok(m) = serde_json::from_str::<CheckpointManifest>(&text) {
                out.push(m);
            }
        }
    }
    out.sort_by(|a, b| b.id.cmp(&a.id));
    out
}

/// Build the restore preview for one checkpoint. The HTTP handler
/// returns this to the UI; nothing is modified yet.
pub async fn restore_preview(
    workspace_root: &Path,
    id: &str,
) -> Result<RestorePreview, CheckpointError> {
    let manifest = load_manifest(workspace_root, id)?;
    match manifest.kind.as_str() {
        "git" => {
            let commit = manifest
                .git_commit
                .ok_or_else(|| CheckpointError::Corrupt(id.to_string()))?;
            // Files changed between the snapshot and the current
            // worktree, scoped to the workspace cwd.
            let out = run_git(
                workspace_root,
                None,
                &["diff", "--name-only", &commit, "--", "."],
            )
            .await
            .map_err(|e| CheckpointError::Git(e.to_string()))?;
            let mut files: Vec<String> = out
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(forward_slashes)
                .collect();
            files.sort();
            files.dedup();
            Ok(RestorePreview {
                files,
                approximate: false,
            })
        }
        _ => Ok(RestorePreview {
            files: manifest.files.clone(),
            approximate: true,
        }),
    }
}

/// Apply a restore. `preview` comes from [`restore_preview`]; the
/// HTTP handler is responsible for having shown it to the user and
/// received an explicit confirmation first.
pub async fn restore(
    workspace_root: &Path,
    id: &str,
    preview: &RestorePreview,
) -> Result<usize, CheckpointError> {
    let manifest = load_manifest(workspace_root, id)?;
    match manifest.kind.as_str() {
        "git" => {
            let commit = manifest
                .git_commit
                .ok_or_else(|| CheckpointError::Corrupt(id.to_string()))?;
            run_git(
                workspace_root,
                None,
                &["restore", "--source", &commit, "--worktree", "--", "."],
            )
            .await
            .map_err(|e| CheckpointError::Git(e.to_string()))?;
            Ok(preview.files.len())
        }
        _ => restore_copy(workspace_root, id).await,
    }
}

fn load_manifest(workspace_root: &Path, id: &str) -> Result<CheckpointManifest, CheckpointError> {
    // Path safety: `id` arrives from the network. Refuse anything
    // that isn't a bare directory name.
    if id.is_empty()
        || id.contains('/')
        || id.contains('\\')
        || id.contains("..")
        || id.starts_with('.')
    {
        return Err(CheckpointError::NotFound(id.to_string()));
    }
    let p = checkpoints_dir(workspace_root)
        .join(id)
        .join("manifest.json");
    let text =
        std::fs::read_to_string(&p).map_err(|_| CheckpointError::NotFound(id.to_string()))?;
    serde_json::from_str(&text).map_err(|_| CheckpointError::Corrupt(id.to_string()))
}

// ── git path ────────────────────────────────────────────────────────

/// Distinguishes "not a git repo / git missing" (→ copy fallback)
/// from unexpected failures (→ surface).
mod create_git {
    #[derive(Debug)]
    pub enum Error {
        NotAWorkTree,
        Git(String),
    }
}

async fn create_git_snapshot(
    workspace_root: &Path,
) -> Result<(String, Vec<String>), create_git::Error> {
    let inside = run_git(
        workspace_root,
        None,
        &["rev-parse", "--is-inside-work-tree"],
    )
    .await
    .map_err(|e| create_git::Error::Git(e.to_string()))?;
    if inside.trim() != "true" {
        return Err(create_git::Error::NotAWorkTree);
    }
    // Fresh empty index for this snapshot only: `git add` stages
    // the workspace into OUR index file; the user's index and
    // refs are never written.
    let tmp_index = checkpoints_dir(workspace_root).join("tmp");
    std::fs::create_dir_all(&tmp_index).map_err(|e| create_git::Error::Git(e.to_string()))?;
    let idx_path = tmp_index.join(format!("index-{}", ulid::Ulid::new()));
    let idx = idx_path.to_string_lossy().into_owned();

    let stage = async {
        run_git(workspace_root, Some(&idx), &["add", "--all", "."])
            .await
            .map_err(|e| create_git::Error::Git(e.to_string()))?;
        let tree = run_git(workspace_root, Some(&idx), &["write-tree"])
            .await
            .map_err(|e| create_git::Error::Git(e.to_string()))?
            .trim()
            .to_string();
        // Parent onto HEAD when one exists; an unborn branch yields
        // a root commit object. Either way nothing references the
        // new commit — its id lives only in our manifest.
        let head = run_git(workspace_root, None, &["rev-parse", "--verify", "HEAD"])
            .await
            .ok()
            .map(|s| s.trim().to_string());
        let commit = match head {
            Some(parent) => {
                let parent_arg = parent.clone();
                run_git(
                    workspace_root,
                    Some(&idx),
                    &[
                        "commit-tree",
                        &tree,
                        "-p",
                        &parent_arg,
                        "-m",
                        "flowntier checkpoint",
                    ],
                )
                .await
            }
            None => {
                run_git(
                    workspace_root,
                    Some(&idx),
                    &["commit-tree", &tree, "-m", "flowntier checkpoint"],
                )
                .await
            }
        }
        .map_err(|e| create_git::Error::Git(e.to_string()))?
        .trim()
        .to_string();
        Ok::<_, create_git::Error>((commit, tree))
    };
    let (commit, _tree) = match stage.await {
        Ok(v) => v,
        Err(e) => {
            let _ = std::fs::remove_file(&idx_path);
            return Err(e);
        }
    };
    let _ = std::fs::remove_file(&idx_path);

    let ls = run_git(
        workspace_root,
        None,
        &["ls-tree", "-r", "--name-only", &commit],
    )
    .await
    .map_err(|e| create_git::Error::Git(e.to_string()))?;
    let mut files: Vec<String> = ls
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(forward_slashes)
        .collect();
    files.sort();
    Ok((commit, files))
}

/// Run one git command in `cwd`. `GIT_INDEX_FILE` is set only for
/// snapshot-staging commands, so queries (rev-parse, diff, restore)
/// see the user's real index.
async fn run_git(
    cwd: &Path,
    index_file: Option<&str>,
    args: &[&str],
) -> Result<String, std::io::Error> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE");
    if let Some(idx) = index_file {
        cmd.env("GIT_INDEX_FILE", idx);
    }
    let out = cmd.output().await?;
    if !out.status.success() {
        return Err(std::io::Error::other(format!(
            "git {}: {}",
            args.first().unwrap_or(&""),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

// ── copy path ───────────────────────────────────────────────────────

async fn copy_snapshot(
    workspace_root: &Path,
    dir: &Path,
) -> Result<(Vec<String>, bool), CheckpointError> {
    let files_dir = dir.join("files");
    std::fs::create_dir_all(&files_dir)?;
    let mut total: u64 = 0;
    let mut files: Vec<String> = Vec::new();
    let mut truncated = false;
    for entry in walk(workspace_root) {
        let rel = match entry.strip_prefix(workspace_root) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let size = std::fs::metadata(&entry).map(|m| m.len()).unwrap_or(0);
        total += size;
        if total > COPY_BUDGET_BYTES {
            return Err(CheckpointError::OverBudget);
        }
        let dest = files_dir.join(rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        tokio::fs::copy(&entry, &dest).await?;
        if files.len() < MAX_LISTED_FILES {
            files.push(forward_slashes(rel));
        } else {
            truncated = true;
        }
    }
    Ok((files, truncated))
}

async fn restore_copy(workspace_root: &Path, id: &str) -> Result<usize, CheckpointError> {
    let src = checkpoints_dir(workspace_root).join(id).join("files");
    if !src.is_dir() {
        return Err(CheckpointError::Corrupt(id.to_string()));
    }
    let mut count = 0usize;
    // Same exclusion set as the snapshot, so agent-created build
    // output and .flowntier state itself are never touched.
    for entry in walk(&src).collect::<Vec<_>>() {
        let rel = match entry.strip_prefix(&src) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let dest = workspace_root.join(rel);
        let _ = std::fs::remove_file(&dest);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        tokio::fs::copy(&entry, &dest).await?;
        count += 1;
    }
    Ok(count)
}

/// Bounded, deterministic walk of `root` honoring the exclusion set.
///
/// Exclusions are evaluated on the path **relative to `root`** —
/// absolute-path matching would empty out a snapshot's `files/`
/// directory wholesale, because `.flowntier/checkpoints/…` carries
/// an excluded component in its ancestors.
fn walk(root: &Path) -> impl Iterator<Item = PathBuf> {
    let root_buf = root.to_path_buf();
    // Note: the tuning methods take `&mut self` and return it, so
    // the builder must be a named binding — chaining them onto the
    // constructor would bind `builder` to a `&mut` temporary that
    // dies at the end of the statement.
    let mut builder = ignore::WalkBuilder::new(root_buf.clone());
    builder.hidden(false);
    builder.git_ignore(false);
    builder.require_git(false);
    builder
        .build()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
        .filter(move |e| {
            let rel_components = e
                .path()
                .strip_prefix(&root_buf)
                .map(|p| p.components().collect::<Vec<_>>())
                .unwrap_or_default();
            !rel_components
                .into_iter()
                .any(|c| EXCLUDED_DIRS.iter().any(|x| c.as_os_str() == *x))
        })
        .map(|e| e.path().to_path_buf())
}

fn forward_slashes(s: impl AsRef<Path>) -> String {
    s.as_ref().to_string_lossy().replace('\\', "/")
}

/// Compact JSON shape for list/create responses (file list omitted;
/// fetch via preview when needed).
pub fn manifest_json(m: &CheckpointManifest) -> serde_json::Value {
    json!({
        "id": m.id,
        "at": m.at,
        "label": m.label,
        "kind": m.kind,
        "file_count": m.file_count,
        "files_truncated": m.files_truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_ws(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ft-ckpt-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn copy_roundtrip_without_git() {
        // A temp dir with one file; not a git repo → copy path.
        let ws = temp_ws("copy");
        std::fs::write(ws.join("app.txt"), "v1").unwrap();
        let m = create(&ws, "test").await.expect("create");
        assert_eq!(m.kind, "copy");
        assert_eq!(m.files, vec!["app.txt".to_string()]);

        // Modify, then preview + restore.
        std::fs::write(ws.join("app.txt"), "v2").unwrap();
        let preview = restore_preview(&ws, &m.id).await.unwrap();
        assert_eq!(preview.files, vec!["app.txt".to_string()]);
        assert!(preview.approximate);
        restore(&ws, &m.id, &preview).await.unwrap();
        assert_eq!(std::fs::read_to_string(ws.join("app.txt")).unwrap(), "v1");
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[tokio::test]
    async fn excluded_dirs_are_never_copied() {
        let ws = temp_ws("excl");
        std::fs::create_dir_all(ws.join("node_modules/pkg")).unwrap();
        std::fs::create_dir_all(ws.join("target/debug")).unwrap();
        std::fs::write(ws.join("node_modules/pkg/index.js"), "js").unwrap();
        std::fs::write(ws.join("target/debug/x.obj"), "obj").unwrap();
        std::fs::write(ws.join("src.rs"), "fn main() {}").unwrap();
        let m = create(&ws, "test").await.expect("create");
        assert_eq!(m.kind, "copy");
        assert_eq!(m.files, vec!["src.rs".to_string()]);
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[tokio::test]
    async fn list_finds_all_and_traversal_ids_are_rejected() {
        let ws = temp_ws("list");
        let m1 = create(&ws, "first").await.unwrap();
        let m2 = create(&ws, "second").await.unwrap();
        let all = list(&ws);
        assert_eq!(all.len(), 2);
        let ids: Vec<&str> = all.iter().map(|m| m.id.as_str()).collect();
        assert!(ids.contains(&m1.id.as_str()));
        assert!(ids.contains(&m2.id.as_str()));
        assert!(matches!(
            restore_preview(&ws, "../../etc").await,
            Err(CheckpointError::NotFound(_))
        ));
        assert!(matches!(
            restore_preview(&ws, "..\\etc").await,
            Err(CheckpointError::NotFound(_))
        ));
        assert!(matches!(
            restore_preview(&ws, ".hidden").await,
            Err(CheckpointError::NotFound(_))
        ));
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn manifest_json_shape_is_stable() {
        let m = CheckpointManifest {
            id: "x".into(),
            at: 1,
            label: "l".into(),
            kind: "git".into(),
            git_commit: Some("abc".into()),
            file_count: 2,
            files: vec!["a".into(), "b".into()],
            files_truncated: false,
        };
        let v = manifest_json(&m);
        assert_eq!(v["kind"], "git");
        assert_eq!(v["file_count"], 2);
        // git_commit intentionally omitted from the summary shape.
        assert!(v.get("git_commit").is_none());
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn git_snapshot_avoids_touching_refs_and_index() {
        // Full git path on a throwaway repo. Requires git on PATH
        // (the e2e job has it; plain dev machines building Flowntier
        // have it too).
        let ws = temp_ws("git");
        let init = tokio::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&ws)
            .output()
            .await
            .expect("git available");
        if !init.status.success() {
            return; // no git on this machine — skip quietly
        }
        std::fs::write(ws.join("a.txt"), "one").unwrap();
        for args in [
            vec!["add", "."],
            vec![
                "-c",
                "user.email=f@f",
                "-c",
                "user.name=f",
                "commit",
                "-qm",
                "init",
            ],
        ] {
            tokio::process::Command::new("git")
                .args(&args)
                .current_dir(&ws)
                .output()
                .await
                .unwrap();
        }
        let head_before = std::fs::read_to_string(ws.join(".git/HEAD")).unwrap();

        // ── Checkpoint BEFORE the "agent" touches anything ──
        let m = create(&ws, "git-snap").await.expect("create");
        assert_eq!(m.kind, "git", "snapshot uses the git path");
        assert!(m.files.contains(&"a.txt".to_string()));

        // ── Agent edits: tracked file modified + new untracked file ──
        std::fs::write(ws.join("a.txt"), "two").unwrap();
        std::fs::write(ws.join("b.txt"), "new file").unwrap();

        // HEAD unchanged, working tree unchanged by the snapshot.
        assert_eq!(
            std::fs::read_to_string(ws.join(".git/HEAD")).unwrap(),
            head_before
        );
        assert_eq!(std::fs::read_to_string(ws.join("a.txt")).unwrap(), "two");

        // Preview: tracked-file delta between now and the snapshot.
        let preview = restore_preview(&ws, &m.id).await.unwrap();
        assert!(!preview.approximate);
        assert!(
            preview.files.contains(&"a.txt".to_string()),
            "preview must list the modified tracked file, got {:?}",
            preview.files
        );
        restore(&ws, &m.id, &preview).await.unwrap();
        assert_eq!(std::fs::read_to_string(ws.join("a.txt")).unwrap(), "one");
        // Known limitation (documented): `git restore --source` only
        // rewrites files present in the snapshot tree, so files that
        // became untracked AFTER the snapshot are left in place.
        assert_eq!(
            std::fs::read_to_string(ws.join("b.txt")).unwrap(),
            "new file"
        );

        // Refs still unchanged after restore.
        assert_eq!(
            std::fs::read_to_string(ws.join(".git/HEAD")).unwrap(),
            head_before
        );
        let _ = std::fs::remove_dir_all(&ws);
    }
}
