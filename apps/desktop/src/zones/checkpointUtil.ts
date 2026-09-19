/**
 * Pure helpers for the v0.4.36 CheckpointsStrip, extracted so the
 * restore-confirmation math can be unit-tested without mounting
 * React or hitting the pipe-server.
 *
 * The restore flow is two-phase (see lib/api.ts restoreCheckpoint):
 * confirm=false returns a RestorePreview with the affected file
 * list. The UI must show the chairman what will change before
 * sending confirm=true. Long lists are truncated to the first
 * `maxShown` names plus a total, so the confirm panel stays a
 * "simple panel" even when a restore would touch hundreds of
 * files.
 */

/** Formatted summary of a RestorePreview.files array. */
export interface RestoreFilesSummary {
  /** First `maxShown` file paths, in preview order. */
  shown: string[];
  /** Total number of affected files (may exceed shown.length). */
  total: number;
  /** How many files were hidden by truncation (total - shown). */
  hidden: number;
  /**
   * True when truncation happened (hidden > 0). The panel renders
   * a "… and N more (N total)" line when this is set.
   */
  truncated: boolean;
}

/**
 * Summarize the affected-file list for the restore confirm panel.
 *
 * @param files     File paths from RestorePreview.files (order is
 *                  preserved — the server returns most-relevant
 *                  first for git checkpoints).
 * @param maxShown  How many names to keep in `shown`. Defaults to
 *                  10 per the v0.4.36 spec.
 */
export function summarizeRestoreFiles(
  files: readonly string[],
  maxShown = 10,
): RestoreFilesSummary {
  const safeFiles = Array.isArray(files) ? files.filter((f) => typeof f === "string") : [];
  const total = safeFiles.length;
  const cap =
    typeof maxShown === "number" && Number.isFinite(maxShown) && maxShown > 0
      ? Math.floor(maxShown)
      : 10;
  const shown = safeFiles.slice(0, cap);
  const hidden = total - shown.length;
  return { shown, total, hidden, truncated: hidden > 0 };
}

/**
 * Format a checkpoint timestamp for the strip row.
 *
 * The pipe-server emits unix SECONDS (chrono `timestamp()`), not
 * milliseconds — the first strip build rendered `new Date(seconds)`
 * and every checkpoint showed a January-1970 date. Values that are
 * already ms-sized (>= 1e12) are accepted as-is so an API bump to
 * ms can't silently reintroduce the bug in the other direction.
 *
 * @param at  Unix seconds (or ms) since epoch; non-finite/<=0 → "--".
 */
export function formatCheckpointTime(at: number): string {
  if (!Number.isFinite(at) || at <= 0) return "--";
  const ms = at >= 1e12 ? at : at * 1000;
  const d = new Date(ms);
  if (Number.isNaN(d.getTime())) return "--";
  const pad = (n: number): string => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}
