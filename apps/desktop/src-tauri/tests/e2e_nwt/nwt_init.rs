//! nwt init E2E tests.

use super::helpers::{init_nwt_fs, TempDir};

// ── SIMPLE ────────────────────────────────────────────────

#[test]
fn e2e_simple_init_creates_skeleton() {
    let tmp = TempDir::new("simple-init");
    let nwt = init_nwt_fs(tmp.path()).expect("init ok");
    assert!(std::path::Path::new(&nwt).join("metadata.json").exists());
    assert!(std::path::Path::new(&nwt).join("timeline").is_dir());
    assert!(std::path::Path::new(&nwt).join("indices").is_dir());
}

#[test]
fn e2e_simple_init_is_idempotent() {
    let tmp = TempDir::new("simple-idem");
    init_nwt_fs(tmp.path()).unwrap();
    let meta = tmp.path().join(".nwt").join("metadata.json");
    let first = std::fs::read_to_string(&meta).unwrap();
    init_nwt_fs(tmp.path()).unwrap();
    let second = std::fs::read_to_string(&meta).unwrap();
    assert_eq!(first, second);
}

// ── MEDIUM ───────────────────────────────────────────────

#[test]
fn e2e_medium_workdir_roundtrip() {
    let tmp = TempDir::new("medium-workdir");
    let workdir = tmp.path().to_string_lossy().into_owned();

    let wd_file = tmp.path().join("wd_holder.json");
    assert!(!wd_file.exists());

    std::fs::write(
        &wd_file,
        serde_json::to_vec_pretty(&serde_json::json!({"workdir": workdir})).unwrap(),
    )
    .unwrap();

    let raw = std::fs::read_to_string(&wd_file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(v["workdir"].as_str().unwrap(), workdir);

    let nwt_path = init_nwt_fs(tmp.path()).unwrap();
    assert!(std::path::Path::new(&nwt_path)
        .join("metadata.json")
        .exists());
}

#[test]
fn e2e_medium_nwt_indices_dirs_exist() {
    let tmp = TempDir::new("medium-idx");
    init_nwt_fs(tmp.path()).unwrap();
    let tags_dir = tmp.path().join(".nwt").join("indices");
    assert!(tags_dir.is_dir());
    let timeline = tmp.path().join(".nwt").join("timeline");
    assert!(timeline.is_dir());
}

// ── HARD ─────────────────────────────────────────────────

#[test]
fn e2e_hard_six_events_in_timeline() {
    let tmp = TempDir::new("hard-events");
    init_nwt_fs(tmp.path()).unwrap();
    let timeline = tmp.path().join(".nwt").join("timeline");
    for id in ["000001", "000002", "000003", "000004", "000005"] {
        let event = serde_json::json!({
            "id": id,
            "timestamp": "2026-06-27T12:00:00Z",
            "task": format!("Task {id}"),
            "summary": "synthetic",
            "tags": ["synthetic", "e2e"],
        });
        std::fs::write(
            timeline.join(format!("{id}.json")),
            serde_json::to_vec_pretty(&event).unwrap(),
        )
        .unwrap();
    }

    let new_id = "000006";
    let event = serde_json::json!({
        "id": new_id,
        "timestamp": "2026-06-27T12:01:00Z",
        "task": "Final task",
        "tags": ["e2e"],
    });
    std::fs::write(
        timeline.join(format!("{new_id}.json")),
        serde_json::to_vec_pretty(&event).unwrap(),
    )
    .unwrap();

    let entries: Vec<_> = std::fs::read_dir(&timeline)
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert_eq!(entries.len(), 6);
    for e in entries {
        let raw = std::fs::read_to_string(e.path()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert!(v["id"].is_string());
    }
}

#[test]
fn e2e_hard_max_id_scan() {
    // Mirror of agent-core nwt.rs read_highest_id() logic
    let tmp = TempDir::new("hard-maxid");
    init_nwt_fs(tmp.path()).unwrap();
    let timeline = tmp.path().join(".nwt").join("timeline");

    // Non-conforming files should be skipped.
    std::fs::write(timeline.join("README.md"), b"# notes").unwrap();
    std::fs::write(timeline.join("garbage.json"), b"{}").unwrap();
    for id in ["000007", "000042", "000003"] {
        let event = serde_json::json!({"id": id});
        std::fs::write(
            timeline.join(format!("{id}.json")),
            serde_json::to_vec_pretty(&event).unwrap(),
        )
        .unwrap();
    }

    let mut max = 0;
    for entry in std::fs::read_dir(&timeline).unwrap().flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.ends_with(".json") {
            continue;
        }
        let stem = &name[..name.len() - 5];
        if stem.len() != 6 {
            continue;
        }
        if !stem.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let n: u32 = stem.parse().unwrap();
        if n > max {
            max = n;
        }
    }
    assert_eq!(max, 42, "should find 000042 as max");
}
