//! nwt boundary E2E tests.

use super::helpers::{init_nwt_fs, TempDir};

// ── BOUNDARY CASES ───────────────────────────────────────

#[test]
fn e2e_boundary_unicode_workdir() {
    let tmp = TempDir::new("boundary-uni");
    let project = tmp.path().join("我的项目-αβγ");
    std::fs::create_dir_all(&project).unwrap();
    let nwt = init_nwt_fs(&project).unwrap();
    assert!(std::path::Path::new(&nwt).join("metadata.json").exists());
}

#[test]
fn e2e_boundary_spaces_in_workdir() {
    let tmp = TempDir::new("boundary-spaces");
    let project = tmp.path().join("my project with spaces");
    std::fs::create_dir_all(&project).unwrap();
    let nwt = init_nwt_fs(&project).unwrap();
    assert!(std::path::Path::new(&nwt).join("timeline").is_dir());
}

#[test]
fn e2e_boundary_empty_timeline_reads_zero() {
    let tmp = TempDir::new("boundary-empty-tl");
    init_nwt_fs(tmp.path()).unwrap();
    let timeline = tmp.path().join(".nwt").join("timeline");
    let entries: Vec<_> = std::fs::read_dir(&timeline).unwrap().collect();
    assert_eq!(entries.len(), 0);
}

#[test]
fn e2e_boundary_metadata_json_written_once() {
    // First init writes metadata.json; second init must NOT
    // overwrite it (preserves created_at).
    let tmp = TempDir::new("boundary-meta");
    init_nwt_fs(tmp.path()).unwrap();
    let meta = tmp.path().join(".nwt").join("metadata.json");
    let _ = std::fs::read_to_string(&meta).unwrap();
    // Manually corrupt the file to detect if init overwrites.
    std::fs::write(&meta, "{\"placeholder\":false,\"manual_edit\":true}\n").unwrap();
    init_nwt_fs(tmp.path()).unwrap();
    let after = std::fs::read_to_string(&meta).unwrap();
    assert_eq!(
        after, "{\"placeholder\":false,\"manual_edit\":true}\n",
        "second init must not overwrite existing metadata.json"
    );
}

#[test]
fn e2e_boundary_nested_nwt_in_nwt() {
    // BUG-003 candidate: what if the workdir already contains
    // a .nwt/ subdirectory from a parent project? Must not
    // recursively nest.
    let tmp = TempDir::new("boundary-nested");
    let project = tmp.path().join("a").join("b").join("c");
    std::fs::create_dir_all(&project).unwrap();
    init_nwt_fs(&project).unwrap();
    let nwt = project.join(".nwt");
    assert!(nwt.join("timeline").is_dir());
    assert!(nwt.join("indices").is_dir());
    // And it should NOT have created .nwt/.nwt/
    assert!(
        !project.join(".nwt").join(".nwt").exists(),
        "must not create nested .nwt/.nwt/"
    );
}
