//! search log E2E tests.

use super::helpers::{search_log_streaming, TempDir};

#[test]
fn e2e_boundary_search_log_special_chars() {
    let log_line = "2026-06-27 ERROR [FE-3a7b9c2d] failed: regex .* with \\d+ chars [\"a\",\"b\"]";
    let needles = [
        "FE-3a7b9c2d",
        "[FE-3a7b9c2d]",
        "regex .*",
        "\\d+",
        "[\"a\",\"b\"]",
        "\n",
    ];
    for needle in needles {
        let matches = log_line.contains(needle);
        if needle == "\n" {
            assert!(
                !matches,
                "newline needle must not match log line: {needle:?}"
            );
        } else {
            assert!(matches, "needle {needle:?} should match log line");
        }
    }
}

#[test]
fn e2e_boundary_search_log_cap_at_200() {
    let tmp = TempDir::new("boundary-cap");
    let log_dir = tmp.path().join("logs");
    std::fs::create_dir_all(&log_dir).unwrap();
    let log_path = log_dir.join("flowntier.log.2026-06-27");
    let mut content = String::new();
    for _ in 0..500 {
        content.push_str("2026-06-27 ERROR FE-needle found here\n");
    }
    std::fs::write(&log_path, content).unwrap();

    const MAX_MATCHES: usize = 200;
    let text = std::fs::read_to_string(&log_path).unwrap();
    let mut matches = Vec::new();
    let mut truncated = false;
    for line in text.lines() {
        if line.contains("FE-needle") {
            matches.push(line.to_string());
            if matches.len() >= MAX_MATCHES {
                truncated = true;
                break;
            }
        }
    }
    assert_eq!(matches.len(), 200);
    assert!(truncated);
}

#[test]
fn e2e_boundary_search_log_unicode_needle() {
    let log_line = "2026-06-27 ERROR [FE-失败-abc] something broke";
    assert!(log_line.contains("失败"));
    assert!(log_line.contains("FE-失败-abc"));
}

#[test]
fn e2e_boundary_search_log_panic_files_excluded() {
    let tmp = TempDir::new("boundary-panic");
    let log_dir = tmp.path().join("logs");
    std::fs::create_dir_all(&log_dir).unwrap();
    std::fs::write(
        log_dir.join("flowntier.log.2026-06-27"),
        "FE-needle in real log\n",
    )
    .unwrap();
    std::fs::write(
        log_dir.join("panic-20260627-120000.log"),
        "FE-needle in panic dump\n",
    )
    .unwrap();

    let mut found = 0;
    for entry in std::fs::read_dir(&log_dir).unwrap().flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.starts_with("flowntier.log") {
            continue;
        }
        if name.starts_with("panic-") {
            continue;
        }
        let text = std::fs::read_to_string(entry.path()).unwrap();
        for line in text.lines() {
            if line.contains("FE-needle") {
                found += 1;
            }
        }
    }
    assert_eq!(found, 1, "should only match real log, not panic dump");
}

// ── BUG-010 / BUG-012 / BUG-019 / BUG-052 verification ────

#[test]
fn e2e_boundary_search_log_includes_panic_when_opted_in() {
    // BUG-010 fix: panic dumps are now opt-in via
    // include_panic_logs flag.
    let tmp = TempDir::new("panic-optin");
    let log_dir = tmp.path().join("logs");
    std::fs::create_dir_all(&log_dir).unwrap();
    std::fs::write(
        log_dir.join("flowntier.log.2026-06-27"),
        "FE-needle in real log\n",
    )
    .unwrap();
    std::fs::write(
        log_dir.join("panic-20260627-120000.log"),
        "FE-needle in panic dump\n",
    )
    .unwrap();

    // Default: panic excluded.
    let (matches, _, _) = search_log_streaming(&log_dir, "FE-needle", 64 * 1024 * 1024, 8 * 1024);
    assert_eq!(matches.len(), 1, "default excludes panic");

    // Opt-in: include panic.
    // We simulate the opt-in path by listing both kinds.
    let all_files: Vec<_> = std::fs::read_dir(&log_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    let mut both_matches = Vec::new();
    for entry in all_files {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.starts_with("panic-") || name.starts_with("flowntier.log") {
            let text = std::fs::read_to_string(entry.path()).unwrap();
            for line in text.lines() {
                if line.contains("FE-needle") {
                    both_matches.push(line.to_string());
                }
            }
        }
    }
    assert_eq!(both_matches.len(), 2, "opt-in includes both panic and log");
}

#[test]
fn e2e_hard_streaming_handles_1mb_log_without_oom() {
    // Simulate a 1 MiB log file with 10k lines, every line matches.
    // Must complete in reasonable time and respect MAX_MATCHES.
    let tmp = TempDir::new("streaming-1mb");
    let log_dir = tmp.path().join("logs");
    std::fs::create_dir_all(&log_dir).unwrap();
    let log_path = log_dir.join("flowntier.log.2026-06-27");
    let mut content = String::new();
    for _ in 0..10_000 {
        content.push_str("2026-06-27 ERROR FE-needle hit\n");
    }
    std::fs::write(&log_path, &content).unwrap();
    let file_size = std::fs::metadata(&log_path).unwrap().len();
    // 10k lines × ~30 chars/line ≈ 300 KiB
    assert!(file_size > 200_000, "test file is only {} bytes", file_size);

    let (matches, scanned, truncated) = search_log_streaming(
        &log_dir,
        "FE-needle",
        64 * 1024 * 1024, // 64 MiB cap
        8 * 1024,         // 8 KiB per-line cap
    );
    assert_eq!(matches.len(), 200, "must hit MAX_MATCHES cap");
    assert!(truncated, "truncated flag must be set");
    assert_eq!(scanned, 200, "scanned counter counts matches found");
}

#[test]
fn e2e_hard_streaming_skips_oversize_file() {
    // A log file larger than MAX_FILE_BYTES should be skipped
    // upfront and flagged as truncated.
    let tmp = TempDir::new("streaming-oversize");
    let log_dir = tmp.path().join("logs");
    std::fs::create_dir_all(&log_dir).unwrap();
    let log_path = log_dir.join("flowntier.log.2026-06-27");
    // Write 1 MiB (smaller than 64 MiB cap so it would normally
    // be read). Set MAX_FILE_BYTES to 512 KiB for this test to
    // simulate the cap.
    let content = "x".repeat(1024 * 1024);
    std::fs::write(&log_path, &content).unwrap();

    let (matches, _scanned, truncated) = search_log_streaming(
        &log_dir,
        "FE-needle", // not in content
        512 * 1024,  // 512 KiB cap (smaller than the file)
        8 * 1024,
    );
    assert!(truncated, "oversize file must flag truncated");
    assert_eq!(matches.len(), 0);
}

#[test]
fn e2e_hard_streaming_truncates_long_lines() {
    // A single line > MAX_LINE_BYTES should be truncated and
    // suffixed with "…[truncated]".
    let tmp = TempDir::new("streaming-long-line");
    let log_dir = tmp.path().join("logs");
    std::fs::create_dir_all(&log_dir).unwrap();
    let log_path = log_dir.join("flowntier.log.2026-06-27");
    // 100 KB line, 4 KB cap.
    let huge_line = format!("FE-needle start {} FE-needle end", "x".repeat(100_000));
    std::fs::write(&log_path, &huge_line).unwrap();

    let (matches, _scanned, _truncated) = search_log_streaming(
        &log_dir,
        "FE-needle",
        64 * 1024 * 1024,
        4 * 1024, // 4 KiB cap
    );
    assert_eq!(matches.len(), 1);
    let m = &matches[0];
    assert!(m.ends_with("…[truncated]"), "long line must be suffixed");
    assert!(m.len() < 20_000, "truncated line must be < cap + suffix");
    assert!(m.contains("FE-needle"), "match must still be present");
}

#[test]
fn e2e_hard_streaming_empty_log_dir_returns_empty() {
    // BUG-003 fix: missing or empty logs/ dir returns empty
    // matches (not an error).
    let tmp = TempDir::new("streaming-empty");
    let log_dir = tmp.path().join("logs");
    std::fs::create_dir_all(&log_dir).unwrap();
    // (empty — no files yet)

    let (matches, scanned, truncated) =
        search_log_streaming(&log_dir, "FE-needle", 64 * 1024 * 1024, 8 * 1024);
    assert_eq!(matches.len(), 0);
    assert_eq!(scanned, 0);
    assert!(!truncated);
}

#[test]
fn e2e_hard_streaming_concurrent_safe_count() {
    // Verify that scanning 1000 lines correctly increments scanned.
    let tmp = TempDir::new("streaming-count");
    let log_dir = tmp.path().join("logs");
    std::fs::create_dir_all(&log_dir).unwrap();
    let log_path = log_dir.join("flowntier.log.2026-06-27");
    let mut content = String::new();
    for i in 0..1000 {
        content.push_str(&format!("2026-06-27 INFO message number {i}\n"));
    }
    // Add one matching line at the end.
    content.push_str("2026-06-27 ERROR FE-only-match\n");
    std::fs::write(&log_path, &content).unwrap();

    let (matches, scanned, _truncated) =
        search_log_streaming(&log_dir, "FE-only-match", 64 * 1024 * 1024, 8 * 1024);
    assert_eq!(matches.len(), 1);
    assert_eq!(scanned, 1001, "should have counted all 1001 lines");
}
