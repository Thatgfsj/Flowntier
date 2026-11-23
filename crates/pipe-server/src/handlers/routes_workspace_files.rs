//! Workspace file + tree + checkpoint routes.

use super::*;

pub(super) fn register(d: &mut Dispatcher, state: Arc<ServerState>) {
    // ── v0.4.21 (event 000066): GET /api/workspace/tree ──
    // Body: { path?: "<relative>", depth?: 2, max_entries?: 200 }
    // Lists directory entries under the current workspace root.
    // Used by the FileTree component. Hidden files are filtered
    // out by default. Result is depth-limited and entry-limited
    // to keep the payload small.
    let s_wstree = state.clone();
    d.register("GET", "/api/workspace/tree", move |body| {
        let s = s_wstree.clone();
        let rel = body
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let depth = body
            .get("depth")
            .and_then(|v| v.as_u64())
            .unwrap_or(2)
            .min(8) as usize;
        let max_entries = body
            .get("max_entries")
            .and_then(|v| v.as_u64())
            .unwrap_or(200)
            .min(2000) as usize;
        Box::pin(async move {
            let ws = s.workspace_snapshot();
            let target = if rel.is_empty() {
                ws.root.clone()
            } else {
                let candidate = ws.resolve(&rel);
                // Safety: refuse paths that escape the workspace.
                if !ws.contains(&candidate) {
                    return Ok((
                        403,
                        json!({
                            "ok": false,
                            "error": format!("path escapes workspace: {}", candidate.display()),
                        }),
                    ));
                }
                candidate
            };
            if !target.exists() {
                return Ok((
                    404,
                    json!({
                        "ok": false,
                        "error": format!("path does not exist: {}", target.display()),
                    }),
                ));
            }
            let mut entries: Vec<Value> = Vec::new();
            let mut truncated = false;
            walk_tree(&target, depth, max_entries, &mut entries, &mut truncated);
            Ok((
                200,
                json!({
                    "ok": true,
                    "root": ws.root.to_string_lossy(),
                    "path": ws.relativize(&target).to_string_lossy(),
                    "entries": entries,
                    "truncated": truncated,
                    "count": entries.len(),
                }),
            ))
        })
    });

    // ── v0.4.36: GET /api/workspace/file ─────────────────────
    // Body: { path: "<relative>" }. Returns one file's UTF-8
    // content + mtime. This is the editor's read path; write goes
    // through POST. Same workspace containment rules as /tree.
    // 2 MiB cap keeps the JSON-RPC line protocol (1 MiB frames on
    // the pipe, 5 MiB on the HTTP bridge) and the webview sane.
    const FILE_API_MAX_BYTES: usize = 2 * 1024 * 1024;
    let s_wsfile = state.clone();
    d.register("GET", "/api/workspace/file", move |body| {
        let s = s_wsfile.clone();
        let rel = body
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        Box::pin(async move {
            if rel.is_empty() {
                return Ok((400, json!({"ok": false, "error": "missing 'path'"})));
            }
            let ws = s.workspace_snapshot();
            let candidate = ws.resolve(&rel);
            if !ws.contains(&candidate) {
                return Ok((403, json!({"ok": false, "error": "path escapes workspace"})));
            }
            let meta = match std::fs::metadata(&candidate) {
                Ok(m) => m,
                Err(e) => {
                    return Ok((
                        404,
                        json!({"ok": false, "error": format!("stat failed: {e}")}),
                    ))
                }
            };
            if !meta.is_file() {
                return Ok((400, json!({"ok": false, "error": "not a regular file"})));
            }
            if meta.len() as usize > FILE_API_MAX_BYTES {
                return Ok((
                    413,
                    json!({
                        "ok": false,
                        "error": format!(
                            "file larger than {} MiB is not editable in-app",
                            FILE_API_MAX_BYTES / (1024 * 1024)
                        ),
                    }),
                ));
            }
            match std::fs::read(&candidate) {
                Ok(bytes) => match String::from_utf8(bytes) {
                    Ok(text) => Ok((
                        200,
                        json!({
                            "ok": true,
                            "path": rel,
                            "content": text,
                            "mtime_ms": meta
                                .modified()
                                .ok()
                                .and_then(|t| {
                                    t.duration_since(std::time::UNIX_EPOCH).ok()
                                })
                                .map(|d| d.as_millis() as u64),
                            "size": text.len(),
                        }),
                    )),
                    Err(_) => Ok((
                        415,
                        json!({"ok": false, "error": "not valid UTF-8 (binary file)"}),
                    )),
                },
                Err(e) => Ok((500, json!({"ok": false, "error": format!("read: {e}")}))),
            }
        })
    });

    // ── v0.4.36: POST /api/workspace/file ────────────────────
    // Body: { path, content, expected_mtime_ms? }. Writes a file
    // inside the workspace. When `expected_mtime_ms` is supplied
    // and differs from the on-disk mtime, the write is refused
    // with `conflict: true` — the editor uses this so a stale
    // buffer never silently clobbers agent output (debate point 8).
    // Refused entirely when the workspace is in readonly mode.
    let s_wsfileput = state.clone();
    d.register("POST", "/api/workspace/file", move |body| {
        let s = s_wsfileput.clone();
        let rel = body
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let content = body
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let expected_mtime_ms = body.get("expected_mtime_ms").and_then(|v| v.as_u64());
        Box::pin(async move {
            if rel.is_empty() {
                return Ok((400, json!({"ok": false, "error": "missing 'path'"})));
            }
            if s.permission() == PermissionMode::ReadOnly {
                return Ok((
                    403,
                    json!({
                        "ok": false,
                        "error": "workspace is in readonly mode",
                        "readonly": true,
                    }),
                ));
            }
            if content.len() > FILE_API_MAX_BYTES {
                return Ok((413, json!({"ok": false, "error": "content too large"})));
            }
            let ws = s.workspace_snapshot();
            let candidate = ws.resolve(&rel);
            if !ws.contains(&candidate) {
                return Ok((403, json!({"ok": false, "error": "path escapes workspace"})));
            }
            if let Some(exp) = expected_mtime_ms {
                let current = std::fs::metadata(&candidate)
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as u64);
                if let Some(cur) = current {
                    if cur != exp {
                        return Ok((
                            409,
                            json!({
                                "ok": false,
                                "conflict": true,
                                "error": "file changed on disk since it was opened",
                                "mtime_ms": cur,
                            }),
                        ));
                    }
                }
            }
            if let Some(parent) = candidate.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    return Ok((500, json!({"ok": false, "error": format!("mkdir: {e}")})));
                }
            }
            match std::fs::write(&candidate, content.as_bytes()) {
                Ok(()) => {
                    let mtime_ms = std::fs::metadata(&candidate)
                        .ok()
                        .and_then(|m| m.modified().ok())
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_millis() as u64);
                    Ok((200, json!({"ok": true, "path": rel, "mtime_ms": mtime_ms})))
                }
                Err(e) => Ok((500, json!({"ok": false, "error": format!("write: {e}")}))),
            }
        })
    });

    // ── v0.4.36: POST /api/checkpoint/create ─────────────────
    // Body: { label? }. Snapshot the workspace (git-first, copy
    // fallback). Auto-checkpoints from the orchestrator use the
    // same routine.
    let s_ckpt_create = state.clone();
    d.register("POST", "/api/checkpoint/create", move |body| {
        let s = s_ckpt_create.clone();
        let label = body
            .get("label")
            .and_then(|v| v.as_str())
            .unwrap_or("manual")
            .to_string();
        Box::pin(async move {
            let ws = s.workspace_snapshot();
            match crate::checkpoint::create(&ws.root, &label).await {
                Ok(m) => Ok((
                    200,
                    json!({"ok": true, "checkpoint": crate::checkpoint::manifest_json(&m)}),
                )),
                Err(e) => Ok((500, json!({"ok": false, "error": e.to_string()}))),
            }
        })
    });

    // ── v0.4.36: GET /api/checkpoint/list ────────────────────
    let s_ckpt_list = state.clone();
    d.register("GET", "/api/checkpoint/list", move |_body| {
        let s = s_ckpt_list.clone();
        Box::pin(async move {
            let ws = s.workspace_snapshot();
            let rows: Vec<Value> = crate::checkpoint::list(&ws.root)
                .iter()
                .map(crate::checkpoint::manifest_json)
                .collect();
            Ok((200, json!({"ok": true, "count": rows.len(), "rows": rows})))
        })
    });

    // ── v0.4.36: POST /api/checkpoint/restore ────────────────
    // Body: { id, confirm?: bool, label? }.
    // confirm=false (default) → preview only: which files would
    // change. confirm=true → apply. The two-phase shape exists
    // because a silent restore over manual edits is data loss.
    let s_ckpt_restore = state.clone();
    d.register("POST", "/api/checkpoint/restore", move |body| {
        let s = s_ckpt_restore.clone();
        let id = body
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let confirm = body
            .get("confirm")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        Box::pin(async move {
            if s.permission() == PermissionMode::ReadOnly {
                return Ok((
                    403,
                    json!({"ok": false, "error": "workspace is in readonly mode"}),
                ));
            }
            let ws = s.workspace_snapshot();
            match crate::checkpoint::restore_preview(&ws.root, &id).await {
                Ok(preview) => {
                    if !confirm {
                        return Ok((
                            200,
                            json!({
                                "ok": true,
                                "preview": preview,
                                "applied": false,
                            }),
                        ));
                    }
                    match crate::checkpoint::restore(&ws.root, &id, &preview).await {
                        Ok(count) => Ok((
                            200,
                            json!({"ok": true, "applied": true, "files_restored": count}),
                        )),
                        Err(e) => Ok((500, json!({"ok": false, "error": e.to_string()}))),
                    }
                }
                Err(e) => Ok((404, json!({"ok": false, "error": e.to_string()}))),
            }
        })
    });
}

/// v0.4.21 (event 000066): depth-limited recursive directory
/// walker for `GET /api/workspace/tree`. Filters hidden files
/// (`.` prefix on Unix, also `node_modules` / `.git` / `target`
/// on all platforms — these are noise for the chairman's
/// project browser). Caps the result at `max_entries` and
/// flips `truncated = true` when the cap kicks in so the UI
/// can show a "…more" footer.
fn walk_tree(
    dir: &std::path::Path,
    depth: usize,
    max_entries: usize,
    out: &mut Vec<serde_json::Value>,
    truncated: &mut bool,
) {
    if depth == 0 || out.len() >= max_entries {
        if out.len() >= max_entries {
            *truncated = true;
        }
        return;
    }
    let read = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return, // unreadable — skip silently
    };
    let mut entries: Vec<_> = read
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            // Skip hidden + known-noisy dirs.
            !(name.starts_with('.') || name == "node_modules" || name == "target" || name == "dist")
        })
        .collect();
    entries.sort_by_key(|e| {
        let name = e.file_name().to_string_lossy().to_string();
        // Directories first, then alphabetical.
        (e.file_type().map(|t| !t.is_dir()).unwrap_or(false), name)
    });
    for entry in entries {
        if out.len() >= max_entries {
            *truncated = true;
            return;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let ft = entry.file_type().ok();
        let is_dir = ft.as_ref().map(|t| t.is_dir()).unwrap_or(false);
        let is_file = ft.as_ref().map(|t| t.is_file()).unwrap_or(false);
        let size = if is_file {
            entry.metadata().ok().map(|m| m.len())
        } else {
            None
        };
        let mut node = json!({
            "name": name,
            "path": entry.path().to_string_lossy(),
            "is_dir": is_dir,
            "is_file": is_file,
        });
        if let Some(s) = size {
            node["size"] = json!(s);
        }
        if is_dir && depth > 1 {
            let mut children = Vec::new();
            let mut sub_truncated = false;
            walk_tree(
                &entry.path(),
                depth - 1,
                max_entries - out.len() - 1,
                &mut children,
                &mut sub_truncated,
            );
            node["children"] = json!(children);
            if sub_truncated {
                *truncated = true;
            }
        }
        out.push(node);
    }
}
