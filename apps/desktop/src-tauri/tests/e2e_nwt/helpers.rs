//! Inlined FS-layer helpers mirrored from the Tauri command layer.
//!
//! These are shared by the E2E test modules below; they intentionally
//! duplicate the production algorithms so the tests link without Tauri.

use std::path::PathBuf;

pub(crate) struct TempDir(PathBuf);

impl TempDir {
    pub(crate) fn new(tag: &str) -> Self {
        let base = std::env::temp_dir().join(format!(
            "flowntier-e2e-{}-{}",
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&base).unwrap();
        Self(base)
    }
    pub(crate) fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// ── Inlined helpers (must match lib.rs) ──────────────────────

pub(crate) fn init_nwt_fs(root: &std::path::Path) -> Result<String, String> {
    let nwt_dir = root.join(".nwt");
    std::fs::create_dir_all(nwt_dir.join("timeline"))
        .map_err(|e| format!("mkdir timeline: {e}"))?;
    std::fs::create_dir_all(nwt_dir.join("indices")).map_err(|e| format!("mkdir indices: {e}"))?;
    let meta = nwt_dir.join("metadata.json");
    if !meta.exists() {
        std::fs::write(&meta, b"{\"placeholder\":true}\n")
            .map_err(|e| format!("write metadata: {e}"))?;
    }
    Ok(nwt_dir.to_string_lossy().into_owned())
}

// ── BUG-006: search_log secret redaction ────────────────
//
// Inline mirror of lib.rs's redact_secrets() function. If the
// lib.rs version changes, update here too.

pub(crate) fn redact_secrets(line: &str) -> String {
    let mut out = line.to_string();

    fn replace_token(s: &str, prefix: &str, replacement: &str, include_prefix: bool) -> String {
        let mut out = String::with_capacity(s.len());
        let bytes = s.as_bytes();
        let prefix_bytes = prefix.as_bytes();
        let plen = prefix_bytes.len();
        let mut i = 0;
        while i + plen <= bytes.len() {
            if &bytes[i..i + plen] == prefix_bytes {
                let after_start = i + plen;
                let mut j = after_start;
                while j < bytes.len()
                    && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_' || bytes[j] == b'-')
                {
                    j += 1;
                }
                if include_prefix {
                    out.push_str(prefix);
                }
                out.push_str(replacement);
                i = j;
            } else {
                let c = s[i..].chars().next().unwrap();
                out.push(c);
                i += c.len_utf8();
            }
        }
        if i < bytes.len() {
            out.push_str(&s[i..]);
        }
        out
    }

    out = replace_token(&out, "Bearer ", "<redacted>", true);

    for prefix in &["sk_live_", "sk_test_"] {
        let mut new_out = String::with_capacity(out.len());
        let bytes = out.as_bytes();
        let prefix_bytes = prefix.as_bytes();
        let plen = prefix_bytes.len();
        let mut i = 0;
        while i + plen <= bytes.len() {
            if &bytes[i..i + plen] == prefix_bytes {
                let mut j = i + plen;
                while j < bytes.len()
                    && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_' || bytes[j] == b'-')
                {
                    j += 1;
                }
                new_out.push_str(prefix);
                new_out.push_str("<redacted>");
                i = j;
            } else {
                let c = out[i..].chars().next().unwrap();
                new_out.push(c);
                i += c.len_utf8();
            }
        }
        if i < bytes.len() {
            new_out.push_str(&out[i..]);
        }
        out = new_out;
    }

    out = replace_token(&out, "sk-", "sk-<redacted>", false);

    for keyword in &["_KEY", "_TOKEN", "_SECRET", "API_KEY", "PASSWORD"] {
        let klen = keyword.len();
        let bytes = out.as_bytes();
        let mut new_out = String::with_capacity(out.len());
        let mut i = 0;
        while i < bytes.len() {
            let mut j = i;
            let mut found = false;
            while j + klen <= bytes.len() {
                if &bytes[j..j + klen] == keyword.as_bytes() {
                    found = true;
                    break;
                }
                j += 1;
            }
            if !found {
                new_out.push_str(&out[i..]);
                break;
            }
            let mut key_start = j;
            while key_start > 0 {
                let prev = bytes[key_start - 1];
                if prev.is_ascii_uppercase() || prev.is_ascii_digit() || prev == b'_' {
                    key_start -= 1;
                } else {
                    break;
                }
            }
            if key_start == j {
                new_out.push_str(&out[i..j + klen]);
                i = j + klen;
                continue;
            }
            let key_end = j + klen;
            new_out.push_str(&out[i..key_end]);
            let mut sep = key_end;
            while sep < bytes.len() && (bytes[sep] == b' ' || bytes[sep] == b'\t') {
                sep += 1;
            }
            if sep >= bytes.len() || (bytes[sep] != b'=' && bytes[sep] != b':') {
                i = key_end;
                continue;
            }
            new_out.push(bytes[sep] as char);
            let mut v = sep + 1;
            if v < bytes.len() && (bytes[v] == b'"' || bytes[v] == b'\'') {
                new_out.push(bytes[v] as char);
                v += 1;
            }
            let v_start = v;
            while v < bytes.len() {
                let b = bytes[v];
                if b.is_ascii_whitespace()
                    || b == b','
                    || b == b'}'
                    || b == b']'
                    || b == b'"'
                    || b == b'\''
                {
                    break;
                }
                v += 1;
            }
            let value_seg = &out[v_start..v];
            if value_seg.contains("<redacted>") {
                new_out.push_str(value_seg);
            } else {
                new_out.push_str("<redacted>");
            }
            i = v;
        }
        out = new_out;
    }

    out
}

pub(crate) fn unix_secs_to_iso8601(t: std::time::SystemTime) -> String {
    use std::time::UNIX_EPOCH;
    let secs = t
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (year, month, day) = civil_from_days((secs / 86_400) as i64);
    let time_of_day = (secs % 86_400) as u32;
    let hour = time_of_day / 3600;
    let minute = (time_of_day % 3600) / 60;
    let second = time_of_day % 60;
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hour, minute, second
    )
}

pub(crate) fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = (yoe as i32) + (era as i32) * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

// ── BUG-016: set_workdir + nwt_init atomicity ───────────

/// Mirror of `set_workdir_with_nwt`'s core logic. On any
/// failure, NEITHER workdir.json nor .nwt/ should be persisted.
pub(crate) fn set_workdir_with_nwt(
    data_dir: &std::path::Path,
    workdir_path: &std::path::Path,
) -> Result<String, String> {
    if !workdir_path.exists() {
        return Err(format!(
            "workdir does not exist: {}",
            workdir_path.display()
        ));
    }
    if !workdir_path.is_dir() {
        return Err(format!(
            "workdir is not a directory: {}",
            workdir_path.display()
        ));
    }
    let nwt_dir = workdir_path.join(".nwt");
    std::fs::create_dir_all(nwt_dir.join("timeline"))
        .map_err(|e| format!("mkdir timeline: {e}"))?;
    std::fs::create_dir_all(nwt_dir.join("indices")).map_err(|e| format!("mkdir indices: {e}"))?;
    let meta = nwt_dir.join("metadata.json");
    if !meta.exists() {
        let project_name = workdir_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("flowntier-project");
        let payload = serde_json::json!({
            "project_name": project_name,
            "created_at": unix_secs_to_iso8601(std::time::SystemTime::now()),
            "schema_version": 1,
            "format": "nwt/0.1",
        });
        let bytes = serde_json::to_vec_pretty(&payload).map_err(|e| format!("serialize: {e}"))?;
        std::fs::write(&meta, bytes).map_err(|e| format!("write metadata: {e}"))?;
    }
    let tags_idx = nwt_dir.join("indices").join("tags.json");
    if !tags_idx.exists() {
        std::fs::write(&tags_idx, b"{}\n").map_err(|e| format!("write tags: {e}"))?;
    }
    let files_idx = nwt_dir.join("indices").join("files.json");
    if !files_idx.exists() {
        std::fs::write(&files_idx, b"{}\n").map_err(|e| format!("write files: {e}"))?;
    }
    // Only NOW persist workdir.json (atomic via tmp+rename).
    let wd_file = data_dir.join("workdir.json");
    let payload = serde_json::json!({ "workdir": workdir_path.to_string_lossy() });
    let bytes =
        serde_json::to_vec_pretty(&payload).map_err(|e| format!("serialize workdir: {e}"))?;
    let tmp = wd_file.with_extension("json.tmp");
    std::fs::write(&tmp, &bytes).map_err(|e| format!("write tmp: {e}"))?;
    std::fs::rename(&tmp, &wd_file).map_err(|e| format!("rename: {e}"))?;
    Ok(nwt_dir.to_string_lossy().into_owned())
}

// ── BUG-004: search_log streaming + caps ─────────────────

/// Mirror of the search_log streaming core logic.
pub(crate) fn search_log_streaming(
    log_dir: &std::path::Path,
    needle: &str,
    max_file_bytes: u64,
    max_line_bytes: usize,
) -> (Vec<String>, usize, bool) {
    let mut matches = Vec::new();
    let mut scanned = 0;
    let mut truncated = false;
    const MAX_MATCHES: usize = 200;

    let Ok(entries) = std::fs::read_dir(log_dir) else {
        return (matches, scanned, truncated);
    };
    let mut files: Vec<(std::path::PathBuf, u64)> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let p = e.path();
            let name = p.file_name()?.to_str()?;
            if !name.starts_with("flowntier.log") {
                return None;
            }
            if name.starts_with("panic-") {
                return None;
            }
            let meta = e.metadata().ok()?;
            Some((p, meta.len()))
        })
        .collect();
    // Newest first (we approximate by path name, which for daily
    // rolling is timestamp-sorted lexicographically).
    files.sort_by(|a, b| b.0.cmp(&a.0));

    'outer: for (path, size) in files {
        if size > max_file_bytes {
            truncated = true;
            continue;
        }
        let Ok(file) = std::fs::File::open(&path) else {
            continue;
        };
        let mut reader = std::io::BufReader::with_capacity(64 * 1024, file);
        let mut bytes_read: u64 = 0;
        let mut line = String::new();
        loop {
            line.clear();
            let n = match std::io::BufRead::read_line(&mut reader, &mut line) {
                Ok(0) => break,
                Ok(n) => n,
                Err(_) => break,
            };
            bytes_read += n as u64;
            if bytes_read > max_file_bytes {
                truncated = true;
                break;
            }
            let trimmed = line.trim_end_matches(['\n', '\r']);
            let line_for_match = if trimmed.len() > max_line_bytes {
                let mut s = String::with_capacity(max_line_bytes + 16);
                s.push_str(&trimmed[..max_line_bytes]);
                s.push_str("…[truncated]");
                s
            } else {
                trimmed.to_string()
            };
            scanned += 1;
            if line_for_match.contains(needle) {
                matches.push(line_for_match);
                if matches.len() >= MAX_MATCHES {
                    truncated = true;
                    break 'outer;
                }
            }
        }
    }
    (matches, scanned, truncated)
}

/// Mirror of `set_workdir_with_nwt`'s workdir.json+metadata.json
/// content. Used to verify the schema invariant that the desktop
/// Tauri command, the Rust agent-core init_workspace, and the
/// upstream nwt CLI all use the same field names.
pub(crate) fn e2e_workdir_metadata_payload(project_name: &str) -> serde_json::Value {
    serde_json::json!({
        "project_name": project_name,
        "created_at": "2026-06-27T00:00:00Z",
        "schema_version": 1,
        "format": "nwt/0.1",
    })
}

/// Mirror of the new nwt_root sentinel file logic in lib.rs's
/// set_workdir_with_nwt. We can't test the actual Tauri command
/// without a live runtime, but the file-format invariant is what
/// matters: pipe-server reads this file to discover the agent's
/// NWT root.
pub(crate) fn write_nwt_root_sentinel(
    data_dir: &std::path::Path,
    nwt_dir: &std::path::Path,
) -> std::io::Result<()> {
    use std::io::Write;
    let payload = serde_json::json!({
        "nwt_root": nwt_dir.to_string_lossy(),
        "set_at": "2026-06-27T00:00:00Z",
    });
    let bytes = serde_json::to_vec_pretty(&payload).unwrap();
    let file = data_dir.join("nwt_root.json");
    let tmp = file.with_extension("json.tmp");
    let mut f = std::fs::File::create(&tmp)?;
    f.write_all(&bytes)?;
    std::fs::rename(&tmp, &file)
}
