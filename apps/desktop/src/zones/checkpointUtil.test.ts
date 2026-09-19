/**
 * v0.4.36: unit tests for the checkpoint restore-confirmation
 * summary math (zones/checkpointUtil.ts).
 *
 * The restore confirm panel must show "first 10 file names plus
 * the total count". These tests pin the truncation rule so a
 * regression can't silently dump 500 file names into the strip's
 * popup (or, worse, hide the total).
 */
import { describe, expect, it } from "vitest";
import { formatCheckpointTime, summarizeRestoreFiles } from "./checkpointUtil.js";

const makeFiles = (n: number): string[] =>
  Array.from({ length: n }, (_, i) => `src/file_${String(i).padStart(3, "0")}.rs`);

describe("summarizeRestoreFiles — restore preview truncation", () => {
  it("keeps the first 10 of a long list and reports the total", () => {
    const files = makeFiles(25);
    const s = summarizeRestoreFiles(files);
    expect(s.total).toBe(25);
    expect(s.shown).toHaveLength(10);
    expect(s.hidden).toBe(15);
    expect(s.truncated).toBe(true);
    // Order preserved: first shown is the first file, last shown
    // is the 10th file (index 9).
    expect(s.shown[0]).toBe("src/file_000.rs");
    expect(s.shown[9]).toBe("src/file_009.rs");
    // Nothing beyond the cap leaks into `shown`.
    expect(s.shown).not.toContain("src/file_010.rs");
  });

  it("does not truncate short lists (no hidden files)", () => {
    const files = makeFiles(3);
    const s = summarizeRestoreFiles(files);
    expect(s.total).toBe(3);
    expect(s.shown).toEqual(files);
    expect(s.hidden).toBe(0);
    expect(s.truncated).toBe(false);
  });

  it("handles the boundary of exactly 10 files without truncation", () => {
    const files = makeFiles(10);
    const s = summarizeRestoreFiles(files);
    expect(s.total).toBe(10);
    expect(s.shown).toHaveLength(10);
    expect(s.truncated).toBe(false);
  });

  it("handles the boundary of exactly 11 files (one hidden)", () => {
    const files = makeFiles(11);
    const s = summarizeRestoreFiles(files);
    expect(s.shown).toHaveLength(10);
    expect(s.hidden).toBe(1);
    expect(s.truncated).toBe(true);
  });

  it("handles an empty preview (zero affected files)", () => {
    const s = summarizeRestoreFiles([]);
    expect(s.total).toBe(0);
    expect(s.shown).toEqual([]);
    expect(s.hidden).toBe(0);
    expect(s.truncated).toBe(false);
  });

  it("honours a custom cap (maxShown=2)", () => {
    const files = makeFiles(5);
    const s = summarizeRestoreFiles(files, 2);
    expect(s.shown).toEqual(["src/file_000.rs", "src/file_001.rs"]);
    expect(s.total).toBe(5);
    expect(s.hidden).toBe(3);
    expect(s.truncated).toBe(true);
  });

  it("is defensive against non-string entries", () => {
    // The IPC layer normally guarantees strings, but a hostile or
    // half-shaped payload must not crash the confirm panel.
    const hostile = ["a.rs", 42, null, "b.rs"] as unknown as string[];
    const s = summarizeRestoreFiles(hostile);
    expect(s.shown).toEqual(["a.rs", "b.rs"]);
    expect(s.total).toBe(2);
  });
});

describe("formatCheckpointTime — unix seconds (not ms) rendering", () => {
  it("treats server timestamps as unix SECONDS", () => {
    // 2026-09-19 00:00:00 UTC = 1789766400 seconds. The first strip
    // build passed seconds to `new Date()` directly and every
    // checkpoint rendered as January 1970.
    const seconds = 1789766400;
    const text = formatCheckpointTime(seconds);
    expect(text).toContain("2026");
    expect(text).not.toContain("1970");
  });

  it("accepts ms-sized values unchanged (forward compat)", () => {
    const ms = 1789766400000;
    expect(formatCheckpointTime(ms)).toBe(formatCheckpointTime(seconds()));
    function seconds(): number {
      return ms / 1000;
    }
  });

  it("renders -- for invalid and epoch-zero values", () => {
    expect(formatCheckpointTime(0)).toBe("--");
    expect(formatCheckpointTime(-5)).toBe("--");
    expect(formatCheckpointTime(Number.NaN)).toBe("--");
  });
});
