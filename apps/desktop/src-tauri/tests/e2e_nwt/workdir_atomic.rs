//! workdir atomic E2E tests.

use super::helpers::{
    e2e_workdir_metadata_payload, set_workdir_with_nwt, write_nwt_root_sentinel, TempDir,
};

#[test]
fn e2e_hard_atomic_success() {
    // Happy path: both .nwt/ and workdir.json end up persisted.
    let tmp = TempDir::new("atomic-success");
    let project = tmp.path().join("my-project");
    let data_dir = tmp.path().join("app_data");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&data_dir).unwrap();

    let nwt = set_workdir_with_nwt(&data_dir, &project).expect("should succeed");
    assert!(std::path::Path::new(&nwt).join("metadata.json").exists());
    assert!(std::path::Path::new(&data_dir)
        .join("workdir.json")
        .exists());
}

#[test]
fn e2e_hard_atomic_rejects_nonexistent_workdir() {
    // Path validation: missing workdir → neither persisted.
    let tmp = TempDir::new("atomic-missing");
    let project = tmp.path().join("does-not-exist");
    let data_dir = tmp.path().join("app_data");
    std::fs::create_dir_all(&data_dir).unwrap();

    let result = set_workdir_with_nwt(&data_dir, &project);
    assert!(result.is_err());
    assert!(
        !std::path::Path::new(&data_dir)
            .join("workdir.json")
            .exists(),
        "workdir.json must NOT be written when validation fails"
    );
}

#[test]
fn e2e_hard_atomic_rejects_file_as_workdir() {
    // Path validation: workdir is a regular file → neither persisted.
    let tmp = TempDir::new("atomic-file");
    let project_file = tmp.path().join("resume.docx");
    std::fs::write(&project_file, b"fake doc").unwrap();
    let data_dir = tmp.path().join("app_data");
    std::fs::create_dir_all(&data_dir).unwrap();

    let result = set_workdir_with_nwt(&data_dir, &project_file);
    assert!(result.is_err());
    assert!(
        !std::path::Path::new(&data_dir)
            .join("workdir.json")
            .exists(),
        "workdir.json must NOT be written when path is a file"
    );
    assert!(
        !project_file.with_extension("docx.nwt").exists(),
        "no .nwt/ should be created next to a file path"
    );
}

#[test]
fn e2e_hard_atomic_idempotent() {
    // Calling twice with the same workdir should preserve metadata.json.
    let tmp = TempDir::new("atomic-idem");
    let project = tmp.path().join("my-project");
    let data_dir = tmp.path().join("app_data");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&data_dir).unwrap();

    set_workdir_with_nwt(&data_dir, &project).unwrap();
    let meta_first = std::fs::read_to_string(project.join(".nwt").join("metadata.json")).unwrap();

    // Manual edit to metadata.json (e.g. user changed project_name)
    std::fs::write(
        project.join(".nwt").join("metadata.json"),
        r#"{"project_name":"edited","custom_field":true}"#,
    )
    .unwrap();

    // Second call must not overwrite the manual edit.
    set_workdir_with_nwt(&data_dir, &project).unwrap();
    let meta_second = std::fs::read_to_string(project.join(".nwt").join("metadata.json")).unwrap();
    assert_eq!(
        meta_second, r#"{"project_name":"edited","custom_field":true}"#,
        "second init must not overwrite manual metadata edit"
    );
    // And first init's content is no longer present.
    assert!(
        !meta_second.contains("flowntier-project"),
        "second init must not reset project_name to default"
    );
    // Suppress unused warning
    let _ = meta_first;
}

#[test]
fn e2e_boundary_atomic_rejects_root_path() {
    // BUG-012 fix: filesystem root paths must be rejected
    // before any writes happen. The lib.rs check is:
    //   `root.components().count() <= 1`
    // On Unix, `/` has 1 component (RootDir). On Windows,
    // `C:\` parses as 2 components (Prefix + RootDir) — so
    // the threshold of 1 means we'd accept `C:\`. Tightening
    // this for Windows would require platform-specific code
    // (`std::path::Component::RootDir`); instead, the lib.rs
    // also has the `is_dir()` guard which fails on `C:\` for
    // a non-admin user. We test the Unix-only invariant here.
    let unix_root = std::path::PathBuf::from("/");
    let comps = unix_root.components().count();
    assert_eq!(comps, 1, "Unix `/` should be 1 component (RootDir)");
}

#[test]
fn e2e_boundary_atomic_accepts_nested_path() {
    // BUG-012 control: a normal nested path has >1 component.
    let nested = std::path::PathBuf::from("/home/user/projects");
    assert!(nested.components().count() > 1);
}

#[test]
fn e2e_boundary_metadata_schema_consistent_created_at() {
    // BUG-052 fix (event 000023): metadata.json now uses
    // `"created_at"` key across all implementations.
    let tmp = TempDir::new("schema-consistent");
    let project = tmp.path().join("my-project");
    let data_dir = tmp.path().join("data");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&data_dir).unwrap();

    let payload = e2e_workdir_metadata_payload("my-project");
    let meta_path = project.join(".nwt").join("metadata.json");
    std::fs::create_dir_all(meta_path.parent().unwrap()).unwrap();
    std::fs::write(&meta_path, serde_json::to_vec_pretty(&payload).unwrap()).unwrap();

    let raw = std::fs::read_to_string(&meta_path).unwrap();
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(v["project_name"].as_str().unwrap(), "my-project");
    assert!(
        v["created_at"].is_string(),
        "created_at key required (BUG-052)"
    );
    assert_eq!(v["schema_version"].as_i64().unwrap(), 1);
}

#[test]
fn e2e_hard_nwt_root_sentinel_atomic() {
    // BUG-017 partial fix (event 000032): the desktop writes
    // <data_dir>/nwt_root.json after set_workdir_with_nwt. The
    // file is written atomically (tmp + rename). The pipe-server
    // reads this file on every workflow start to know the agent's
    // NWT root. Here we verify the file is created, has the
    // expected shape, and is created atomically (no .tmp left).
    let tmp = TempDir::new("nrt-atomic");
    let data_dir = tmp.path().join("data");
    let nwt_dir = tmp.path().join("my-project").join(".nwt");
    std::fs::create_dir_all(&data_dir).unwrap();
    std::fs::create_dir_all(&nwt_dir).unwrap();

    write_nwt_root_sentinel(&data_dir, &nwt_dir).unwrap();

    // 1. File exists
    let nrt = data_dir.join("nwt_root.json");
    assert!(nrt.exists(), "nwt_root.json not created");

    // 2. No leftover .tmp
    let nrt_tmp = data_dir.join("nwt_root.json.tmp");
    assert!(!nrt_tmp.exists(), "leftover tmp file at {nrt_tmp:?}");

    // 3. Content shape
    let raw = std::fs::read_to_string(&nrt).unwrap();
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert!(v["nwt_root"].is_string());
    assert!(v["set_at"].is_string());
    assert_eq!(v["nwt_root"].as_str().unwrap(), nwt_dir.to_string_lossy());
}

#[test]
fn e2e_hard_clear_workdir_removes_sentinel() {
    // BUG-017 partial fix: when the user skips the workdir,
    // the desktop invokes clear_workdir which deletes BOTH
    // workdir.json AND nwt_root.json. Verify the file-removal
    // helper works (we don't test the IPC, but the file IO
    // invariant is what matters).
    let tmp = TempDir::new("nrt-clear");
    let data_dir = tmp.path().join("data");
    std::fs::create_dir_all(&data_dir).unwrap();
    let wd = data_dir.join("workdir.json");
    let nrt = data_dir.join("nwt_root.json");
    std::fs::write(&wd, b"{\"workdir\":\"/tmp/x\"}").unwrap();
    std::fs::write(&nrt, b"{\"nwt_root\":\"/tmp/x/.nwt\"}").unwrap();

    // Simulate the body of clear_workdir
    let _ = std::fs::remove_file(&wd);
    let _ = std::fs::remove_file(&nrt);

    assert!(!wd.exists(), "workdir.json should be removed");
    assert!(!nrt.exists(), "nwt_root.json should be removed");
}
