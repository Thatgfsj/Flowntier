//! NWT (neuroweave-timeline) tool for the Rust agent loop.
//!
//! The chairman's reference (gfcode) embedded nwt into the
//! project so the AI agent's actions are recorded for
//! post-mortem. The desktop side already has a TypeScript
//! port at `apps/desktop/src/tools/nwt.ts` (Phase NWT-A) that
//! writes the same JSON format. This is the Rust side so the
//! in-process agent loop can also log events.
//!
//! Data-format compatible with the upstream nwt CLI:
//!   <root>/.nwt/timeline/NNNNNN.json
//! plus tags/files indices in <root>/.nwt/indices/.
//! The agent-core tool can write events that the upstream nwt
//! CLI can read, and vice-versa.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{Tool, ToolContext, ToolError, ToolOutput};

/// 6-digit zero-padded event id. Matches the upstream nwt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NwtEvent {
    pub id: String,
    pub timestamp: String,
    pub task: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
}

/// Process-wide registry of the current `root` for nwt writes.
///
/// The chairman's design: a single workspace per desktop session,
/// so the nwt root is a global. We avoid passing the workdir
/// through ToolContext for every tool call (which would require
/// plumbing through the agent loop + pipe server). When the
/// chairman's "AI 主动记录到 NWT" instruction fires, the
/// tool reads from this Mutex<PathBuf>.

/// BUG-041 + BUG-042 fix (event 000023): wraps both the index
/// read-modify-write (tags.json / files.json) and the next_id
/// allocation so concurrent tool calls from parallel workers
/// can't lose updates or collide on the same event id. The
/// lock is held briefly (just the JSON read, in-memory append,
/// and JSON write), so contention is minimal in practice.
static INDEX_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
static NWT_ROOT: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Set the workspace root for nwt writes. Called by App.tsx
/// after the workdir is configured (WorkdirSetup onConfirm) or
/// after the user picks a project from the dashboard.
pub fn set_nwt_root(root: PathBuf) {
    let mut g = NWT_ROOT.lock().expect("NWT_ROOT mutex poisoned");
    *g = Some(root);
}

/// Clear the workspace root. Called when the user clears all
/// data or changes workdir.
pub fn clear_nwt_root() {
    let mut g = NWT_ROOT.lock().expect("NWT_ROOT mutex poisoned");
    *g = None;
}

// ── File layout helpers (mirrors apps/desktop/src/tools/nwt.ts) ──

fn nwt_dir(root: &Path) -> PathBuf {
    root.join(".nwt")
}

fn event_path(root: &Path, id: &str) -> PathBuf {
    nwt_dir(root).join("timeline").join(format!("{id}.json"))
}

fn tags_index(root: &Path) -> PathBuf {
    nwt_dir(root).join("indices").join("tags.json")
}

fn files_index(root: &Path) -> PathBuf {
    nwt_dir(root).join("indices").join("files.json")
}

fn ensure_nwt(root: &Path) -> std::io::Result<PathBuf> {
    let d = nwt_dir(root);
    fs::create_dir_all(d.join("timeline"))?;
    fs::create_dir_all(d.join("indices"))?;
    Ok(d)
}

fn read_highest_id(root: &Path) -> u32 {
    let timeline = nwt_dir(root).join("timeline");
    if !timeline.is_dir() {
        return 0;
    }
    let mut max = 0u32;
    for entry in fs::read_dir(&timeline).into_iter().flatten() {
        if let Some(name) = entry.file_name().to_str() {
            if let Some(rest) = name.strip_suffix(".json") {
                if let Ok(n) = rest.parse::<u32>() {
                    if n > max {
                        max = n;
                    }
                }
            }
        }
    }
    max
}

fn next_id(root: &Path) -> String {
    // BUG-042 fix (event 000023): wrap the read-then-format
    // under INDEX_LOCK so concurrent calls don't both see the
    // same max and propose the same id. With the lock held, the
    // returned id is unique to this process's current call.
    // The actual file creation uses create_new in `log_event`
    // for atomic on-disk uniqueness across processes.
    let _guard = INDEX_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    format!("{:06}", read_highest_id(root) + 1)
}

fn now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Convert epoch seconds to a UTC ISO 8601 timestamp.
    // We avoid pulling in chrono here for one fn; the rough
    // conversion is fine because the upstream nwt parser
    // accepts both Z and +00:00 forms.
    // (seconds-since-1970 is 10 digits; we just emit the
    // canonical form. If we need millisecond precision later
    // we'll add the chrono dep.)
    let days = (secs / 86400) as i64;
    let secs_of_day = secs % 86400;
    let h = secs_of_day / 3600;
    let m = (secs_of_day % 3600) / 60;
    let s = secs_of_day % 60;
    // (1970-01-01) + days, with leap years handled minimally.
    let mut y = 1970i64;
    let mut d = days;
    loop {
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        let dy = if leap { 366 } else { 365 };
        if d < dy {
            break;
        }
        d -= dy;
        y += 1;
    }
    // Day-of-year to month/day (good-enough for YYYY-MM-DD).
    let month_days = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let md = if leap {
        let mut m2 = [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        m2
    } else {
        month_days
    };
    let mut mo = 0usize;
    let mut rem = d as usize;
    while rem >= md[mo] {
        rem -= md[mo];
        mo += 1;
    }
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y,
        mo + 1,
        rem + 1,
        h,
        m,
        s
    )
}

// ── Index maintenance ──────────────────────────────────────────

fn update_index(path: &Path, key: &str, event_id: &str) {
    // BUG-041 fix (event 000023): take INDEX_LOCK so concurrent
    // tool calls serialise on the read-modify-write. Without
    // this, two parallel calls would both read the same map,
    // each append their id, and the later write would clobber
    // the earlier one — losing events from the index. The lock
    // is process-global; we hold it only for the JSON I/O.
    let _guard = INDEX_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut idx: std::collections::BTreeMap<String, Vec<String>> = if path.exists() {
        fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    } else {
        Default::default()
    };
    let entry = idx.entry(key.to_string()).or_default();
    if !entry.contains(&event_id.to_string()) {
        entry.push(event_id.to_string());
    }
    if let Ok(s) = serde_json::to_string_pretty(&idx) {
        let _ = fs::write(path, s);
    }
}

// ── Public API ──────────────────────────────────────────────

/// Initialize the .nwt/ directory. Idempotent.
pub fn init_workspace(root: &Path) -> std::io::Result<()> {
    ensure_nwt(root)?;
    let meta = nwt_dir(root).join("metadata.json");
    if !meta.exists() {
        let payload = serde_json::json!({
            // BUG-052 fix (event 000023): rename `"created"` →
            // `"created_at"` to match the field name used by the
            // desktop's `set_workdir_with_nwt` Tauri command and
            // the upstream nwt CLI. Before this rename, workdirs
            // initialised via the Rust agent loop had a different
            // schema key than workdirs initialised via the
            // desktop UI, breaking downstream tooling that reads
            // `metadata.json.created_at`.
            "project_name": root.file_name().and_then(|s| s.to_str()).unwrap_or("flowntier-project"),
            "created_at": now_iso(),
            "schema_version": 1,
            "nwt_cli_compat": "1.0",
        });
        fs::write(meta, serde_json::to_vec_pretty(&payload).unwrap_or_default())?;
    }
    Ok(())
}

/// Append a new event. Returns the new event id.
pub fn log_event(root: &Path, event: &NwtEvent) -> std::io::Result<String> {
    init_workspace(root)?;
    let mut id = next_id(root);
    let mut to_write = event.clone();
    to_write.id = id.clone();
    let mut path = event_path(root, &id);
    // BUG-042 fix (event 000023): use `create_new` so two
    // concurrent processes can't both write to the same file.
    // If the proposed id is already taken (because another
    // process raced us between `next_id` and `create_new`),
    // bump and retry up to 100 times. After 100 collisions,
    // give up — likely indicates a runaway writer.
    let bytes = serde_json::to_vec_pretty(&to_write).unwrap_or_default();
    let mut attempts = 0;
    loop {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut f) => {
                use std::io::Write;
                let _ = f.write_all(&bytes);
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                attempts += 1;
                if attempts >= 100 {
                    return Err(e);
                }
                // Bump id by parsing, incrementing, re-formatting.
                let n: u32 = id.parse().unwrap_or(0);
                id = format!("{:06}", n + 1);
                to_write.id = id.clone();
                path = event_path(root, &id);
            }
            Err(e) => return Err(e),
        }
    }
    if let Some(tags) = &event.tags {
        for tag in tags {
            update_index(&tags_index(root), tag, &id);
        }
    }
    if let Some(files) = &event.files {
        for f in files {
            update_index(&files_index(root), f, &id);
        }
    }
    Ok(id)
}

// ── Tool wrapper ────────────────────────────────────────────

/// The `nwt_log` tool exposes one operation to the agent loop:
/// record a new event. The agent supplies task + summary +
/// optional reason/files/tags. The tool assigns id + timestamp
/// (matches the upstream nwt CLI's behaviour).
pub struct NwtLogTool;

impl std::fmt::Debug for NwtLogTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NwtLogTool").finish()
    }
}

#[async_trait]
impl Tool for NwtLogTool {
    fn name(&self) -> &'static str {
        "nwt_log"
    }

    fn schema(&self) -> serde_json::Value {
        NwtLogTool::schema()
    }

    fn description(&self) -> &'static str {
        "Record an event to the project's neuroweave-timeline \
         (.nwt/timeline/). The event captures what was done \
         (summary), why (reason), which files were touched, and \
         tags for later search. Use this at the end of any \
         meaningful step (a file edit, a refactor, a config \
         change, a successful build) so post-mortem analysis is \
         possible without re-running the agent."
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let task = args
            .get("task")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing 'task' (string)".into()))?
            .to_string();
        let summary = args
            .get("summary")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing 'summary' (string)".into()))?
            .to_string();
        let reason = args.get("reason").and_then(|v| v.as_str()).map(String::from);
        let files = args.get("files").and_then(|v| v.as_array()).map(|a| {
            a.iter().filter_map(|x| x.as_str().map(String::from)).collect()
        });
        let tags = args.get("tags").and_then(|v| v.as_array()).map(|a| {
            a.iter().filter_map(|x| x.as_str().map(String::from)).collect()
        });
        let parent = args.get("parent").and_then(|v| v.as_str()).map(String::from);

        // We use the global NWT root (set by the desktop shell
        // when the workdir is configured), not the ToolContext's
        // workspace. The agent's per-task workspace is inside the
        // nwt root, and the agent should record events at the
        // nwt-root level so the upstream nwt CLI can navigate
        // them.
        let root = {
            let g = NWT_ROOT.lock().expect("NWT_ROOT mutex poisoned");
            g.clone()
        };
        let Some(root) = root else {
            return Err(ToolError::Refused(
                "no nwt root configured (the user has not yet \
                 set a workdir in the WorkdirSetup dialog). \
                 Use Settings > About > Change workdir to set \
                 one before recording events."
                    .into(),
            ));
        };

        let event = NwtEvent {
            id: String::new(), // assigned by log_event
            timestamp: String::new(), // ditto
            task,
            summary,
            reason,
            files,
            tags,
            parent,
        };

        let id = log_event(&root, &event).map_err(ToolError::Io)?;
        Ok(ToolOutput {
            content: format!("logged nwt event {id}"),
            preview: format!("nwt {id}"),
        })
    }
}

// ── Schema (required by Tool trait) ───────────────────────────

impl NwtLogTool {
    /// JSON-Schema for the nwt_log tool's arguments. Mirrors
    /// the upstream nwt CLI's "nwt log" interface: required
    /// task + summary; optional reason / files / tags / parent.
    pub fn schema() -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "task": {
                    "type": "string",
                    "description": "Short imperative title (e.g. 'Fix login bug')"
                },
                "summary": {
                    "type": "string",
                    "description": "What was done, in 1-2 sentences"
                },
                "reason": {
                    "type": "string",
                    "description": "WHY it was done — the motivation / context"
                },
                "files": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Project-relative file paths touched"
                },
                "tags": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Free-form tags for later search (e.g. 'bugfix', 'refactor')"
                },
                "parent": {
                    "type": "string",
                    "description": "6-digit id of a previous event this builds on (optional)"
                }
            },
            "required": ["task", "summary"]
        })
    }
}
